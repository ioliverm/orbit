-- Slice 2 T20 — espp_purchases / art_7p_trips / modelo_720_user_inputs DDL
-- + additive sessions.country_iso2 column.
--
-- Traces to:
--   - ADR-016 §1 (authoritative DDL for espp_purchases, art_7p_trips,
--     modelo_720_user_inputs; additive column sessions.country_iso2).
--   - docs/requirements/slice-2-acceptance-criteria.md §4 (ESPP purchases),
--     §5 (Art. 7.p trips), §6 (Modelo 720 inputs), §7 (sessions UI).
--   - ADR-014 §1 for the reused touch_updated_at() function and the
--     tenant_isolation RLS policy-name convention.
--
-- Scope: three user-scoped tables + one column add. Every new table is
-- ENABLE ROW LEVEL SECURITY with a `tenant_isolation` policy keyed off
-- `app.user_id` (SEC-020..023). No new extensions required.

-- ESPP PURCHASES ---------------------------------------------------------
-- AC-4.1..AC-4.5. One row per ESPP purchase window; the parent grant
-- must have instrument='espp' — enforced via a BEFORE-INSERT/UPDATE
-- trigger (see espp_purchases_enforce_grant_instrument below). PostgreSQL
-- subqueries are not allowed in CHECK constraints, so a trigger is the
-- correct mechanism for the "grant_id references an ESPP grant" assertion.
CREATE TABLE espp_purchases (
  id                          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  grant_id                    UUID NOT NULL REFERENCES grants(id) ON DELETE CASCADE,
  offering_date               DATE NOT NULL,
  purchase_date               DATE NOT NULL,
  fmv_at_purchase             NUMERIC(20,6) NOT NULL CHECK (fmv_at_purchase > 0),
  purchase_price_per_share    NUMERIC(20,6) NOT NULL CHECK (purchase_price_per_share > 0),
  shares_purchased            NUMERIC(20,4) NOT NULL CHECK (shares_purchased > 0),
  currency                    TEXT NOT NULL CHECK (currency IN ('USD','EUR','GBP')),
  fmv_at_offering             NUMERIC(20,6)
                              CHECK (fmv_at_offering IS NULL OR fmv_at_offering > 0),
  employer_discount_percent   NUMERIC(5,2)
                              CHECK (employer_discount_percent IS NULL
                                     OR (employer_discount_percent >= 0
                                         AND employer_discount_percent <= 100)),
  notes                       TEXT CHECK (notes IS NULL OR length(notes) <= 2048),
  created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
  CONSTRAINT purchase_after_offering CHECK (purchase_date >= offering_date)
);

CREATE INDEX espp_purchases_user_grant_date_idx
  ON espp_purchases (user_id, grant_id, purchase_date DESC);

ALTER TABLE espp_purchases ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON espp_purchases
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

CREATE TRIGGER espp_purchases_touch_updated_at
  BEFORE UPDATE ON espp_purchases
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- Trigger: parent grant must be an ESPP grant (AC-4.4.1).
-- Why a trigger, not a CHECK: PostgreSQL disallows subqueries in CHECK
-- constraints. A partial unique index cannot express the predicate
-- either. A trigger is the boring, correct mechanism and its cost is one
-- row lookup per insert/update, which is dominated by the FK check
-- Postgres is already performing. The function is STABLE and
-- SECURITY INVOKER (no definer bypass — the trigger fires under the
-- caller's privileges, which already hold `app.user_id` = owner).
CREATE OR REPLACE FUNCTION espp_purchases_enforce_grant_instrument()
RETURNS TRIGGER AS $$
DECLARE
  parent_instrument TEXT;
BEGIN
  SELECT instrument INTO parent_instrument
    FROM grants
   WHERE id = NEW.grant_id;
  IF parent_instrument IS NULL THEN
    RAISE EXCEPTION 'espp_purchases: parent grant % not found', NEW.grant_id
      USING ERRCODE = 'foreign_key_violation';
  END IF;
  IF parent_instrument <> 'espp' THEN
    RAISE EXCEPTION
      'espp_purchases: parent grant % has instrument=%, expected espp',
      NEW.grant_id, parent_instrument
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql
   STABLE;

CREATE TRIGGER espp_purchases_enforce_grant_instrument_trg
  BEFORE INSERT OR UPDATE OF grant_id ON espp_purchases
  FOR EACH ROW EXECUTE FUNCTION espp_purchases_enforce_grant_instrument();

-- ART. 7.P TRIPS ---------------------------------------------------------
-- AC-5.1..AC-5.3. Five fact fields + the five-criterion eligibility
-- checklist stored as JSONB to preserve Slice-4 flexibility (the
-- requirements-analyst may add a sixth criterion without a schema
-- migration). The column CHECK asserts object shape; the handler
-- validates keys and value types before writing (SEC-163). Allowed keys
-- (handler-enforced, non-normative at the DB): services_outside_spain,
-- non_spanish_employer, not_tax_haven, no_double_exemption,
-- within_annual_cap. Each value is `true`, `false`, or `null`.
CREATE TABLE art_7p_trips (
  id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  destination_country    TEXT NOT NULL CHECK (length(destination_country) = 2),
  from_date              DATE NOT NULL,
  to_date                DATE NOT NULL CHECK (to_date >= from_date),
  employer_paid          BOOLEAN NOT NULL,
  purpose                TEXT CHECK (purpose IS NULL OR length(purpose) <= 1024),
  eligibility_criteria   JSONB NOT NULL DEFAULT '{}'::jsonb
                          CHECK (jsonb_typeof(eligibility_criteria) = 'object'),
  created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON COLUMN art_7p_trips.eligibility_criteria IS
  'Five-key JSONB object: services_outside_spain, non_spanish_employer, not_tax_haven, no_double_exemption, within_annual_cap. Each value is true, false, or null. Handler (orbit-api) validates shape before write (SEC-163); the DB only enforces jsonb_typeof = object.';

CREATE INDEX art_7p_trips_user_from_date_idx
  ON art_7p_trips (user_id, from_date DESC);

ALTER TABLE art_7p_trips ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON art_7p_trips
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

CREATE TRIGGER art_7p_trips_touch_updated_at
  BEFORE UPDATE ON art_7p_trips
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- MODELO 720 USER INPUTS -------------------------------------------------
-- AC-6.1..AC-6.3. Time-series with close-and-create semantics (same
-- pattern as residency_periods). Two categories, one row per
-- (user, category, from_date). Only the securities category is NOT
-- represented here — it is computed from grants via FX in Slice 3 and
-- the UI stubs it per AC-6.1.5.
--
-- `category` is an enum-like TEXT column; currently two values; a
-- future 'securities_manual_override' variant is easy to add.
CREATE TABLE modelo_720_user_inputs (
  id                         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  category                   TEXT NOT NULL
                              CHECK (category IN ('bank_accounts','real_estate')),
  amount_eur                 NUMERIC(20,2) NOT NULL CHECK (amount_eur >= 0),
  reference_date             DATE NOT NULL,
  from_date                  DATE NOT NULL,
  to_date                    DATE,
  created_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
  CONSTRAINT m720_to_after_from CHECK (to_date IS NULL OR to_date >= from_date)
);

-- One open row per (user, category) at a time.
CREATE UNIQUE INDEX modelo_720_user_inputs_open_idx
  ON modelo_720_user_inputs (user_id, category) WHERE to_date IS NULL;

-- Scan pattern: list history by user + category, newest first.
CREATE INDEX modelo_720_user_inputs_user_category_from_idx
  ON modelo_720_user_inputs (user_id, category, from_date DESC);

ALTER TABLE modelo_720_user_inputs ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON modelo_720_user_inputs
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);
-- No updated_at trigger: the row is append-only from the handler's POV
-- (close-and-create). Only `to_date` is ever mutated, by the same
-- transaction that inserts the successor row; that is an audited lift,
-- not an in-place edit.

-- SESSIONS — additive column for creation-time country lookup ------------
-- AC-7.1.3. The sessions UI surfaces a coarse geo hint; raw IP is never
-- displayed or serialized. Because `ip_hash` is HMAC-SHA256 and not
-- reversible, the country lookup must happen at session-creation time
-- (the only point in the pipeline where a raw IP still exists in RAM;
-- SEC-054). We store the ISO 3166-1 alpha-2 code on the session row and
-- expose it verbatim to the UI; the raw IP and the ip_hash are never
-- read by any list handler.
ALTER TABLE sessions
  ADD COLUMN country_iso2 TEXT
    CHECK (country_iso2 IS NULL OR length(country_iso2) = 2);

-- Ownership (mirrors 20260425120000_slice_1.sql §Ownership).
ALTER TABLE espp_purchases            OWNER TO orbit_migrate;
ALTER TABLE art_7p_trips              OWNER TO orbit_migrate;
ALTER TABLE modelo_720_user_inputs    OWNER TO orbit_migrate;
ALTER FUNCTION espp_purchases_enforce_grant_instrument() OWNER TO orbit_migrate;

-- Grants — orbit_app (full DML; RLS constrains visible rows).
GRANT SELECT, INSERT, UPDATE, DELETE ON espp_purchases         TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON art_7p_trips           TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON modelo_720_user_inputs TO orbit_app;
-- sessions already grants DML to orbit_app per 20260418120000_init.sql §5;
-- the column add inherits those grants.
