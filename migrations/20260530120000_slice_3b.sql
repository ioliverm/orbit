-- Slice 3b T37 — user_tax_preferences sidecar + vesting_events sell-to-cover
-- additions.
--
-- Traces to:
--   - ADR-018 §1 (authoritative DDL for user_tax_preferences; additive
--     columns on vesting_events for the sell-to-cover track).
--   - docs/requirements/slice-3b-acceptance-criteria.md §4
--     (user_tax_preferences requirements), §5 (vesting_events additions
--     + CHECK coherence).
--   - ADR-016 §1 (modelo_720_user_inputs close-and-create pattern reused
--     verbatim as the sidecar's transactional shape).
--   - ADR-017 §1 (vesting_events override-flag coherence CHECK shape
--     reused here for is_sell_to_cover_override + sell_to_cover_overridden_at).
--   - ADR-014 §1 (touch_updated_at + tenant_isolation policy convention).
--
-- Scope: one user-scoped sidecar table (partial UNIQUE on the open row),
-- five additive columns on vesting_events, two cross-field CHECK
-- constraints. No new extensions required. No new trigger on
-- vesting_events (the Slice-3 vesting_events_touch_updated_at trigger
-- already gates every UPDATE).

-- USER_TAX_PREFERENCES --------------------------------------------------
-- AC-4.1..4.6 (Slice-3b AC doc). Time-series sidecar: at most one open
-- row per user at a time; history accumulates as the user updates their
-- country / percent / sell-to-cover defaults over time. Close-and-create
-- semantics mirror modelo_720_user_inputs (ADR-016 §1); same-day save
-- updates the open row in place so a user who mistypes the percent does
-- not produce a 1-day zero-span row in the history table.
--
-- country_iso2 is ISO-3166 alpha-2 (ES, PT, FR, GB, ...). The curated
-- list of accepted values is handler-gated; the DDL constrains only
-- length + uppercase posture so a future expansion of the list is a
-- one-line handler change, not a schema change. The column is
-- NOT NULL — country is required on every save (the AC doc's AC-4.1.3
-- "empty first-render" state is a UI default, not a persisted state —
-- the user cannot save a row without picking a country).
--
-- rendimiento_del_trabajo_percent is stored as a fraction in [0, 1]
-- (e.g., the user types 45 in the input; the handler converts to
-- 0.4500 before INSERT). NULLABLE — the field is hidden for non-Spain
-- countries per AC-4.2.2, and the UI stores NULL for those rows.
-- NUMERIC(5,4) gives us four decimal places (45,0000 % as the canonical
-- display form) while pinning to fraction semantics.
--
-- sell_to_cover_enabled is NOT NULL and has NO DEFAULT — the server
-- rejects a body that omits the field. Rationale: the toggle has three
-- legal states from the UI's perspective (on, off, neutral-first-render),
-- but neutral is a client-only display state; every save commits to
-- one of the two booleans. Making the column NOT NULL without a DEFAULT
-- forces the handler to validate presence, rather than silently writing
-- a false default when the client forgot the key.
--
-- from_date / to_date is the close-and-create window. NULL to_date
-- means "still the currently-open row" (AC-4.4 semantics). The partial
-- UNIQUE index enforces at most one open row per user.
--
-- updated_at is maintained by a dedicated trigger (below) so the
-- same-day-in-place UPDATE bumps the timestamp. No OCC is wired in
-- Slice 3b (AC-9.4: Profile saves are last-write-wins by design — the
-- history table preserves all prior writes regardless), but the column
-- is present so a future slice can add OCC without a schema change.
CREATE TABLE user_tax_preferences (
  id                              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  country_iso2                    TEXT NOT NULL
                                    CHECK (length(country_iso2) = 2
                                           AND country_iso2 = upper(country_iso2)),
  rendimiento_del_trabajo_percent NUMERIC(5,4)
                                    CHECK (rendimiento_del_trabajo_percent IS NULL
                                           OR (rendimiento_del_trabajo_percent >= 0
                                               AND rendimiento_del_trabajo_percent <= 1)),
  sell_to_cover_enabled           BOOLEAN NOT NULL,
  from_date                       DATE NOT NULL,
  to_date                         DATE,
  created_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Partial UNIQUE: one open row per user at a time (AC-4.4.4).
-- A close-and-create writer first closes the prior row (sets to_date)
-- in the same transaction, so the successor INSERT sees an empty
-- filtered index and succeeds. A racing second writer trips the index
-- with unique_violation and the caller's transaction rolls back.
CREATE UNIQUE INDEX user_tax_preferences_open_row_idx
  ON user_tax_preferences (user_id)
  WHERE to_date IS NULL;

-- History lookup: the Profile page reads prior closed rows ordered
-- descending by from_date for the history table (AC-4.5.1).
CREATE INDEX user_tax_preferences_user_from_date_idx
  ON user_tax_preferences (user_id, from_date DESC);

-- touch_updated_at trigger — reuses the shared function from
-- migrations/20260425120000_slice_1.sql §touch_updated_at.
CREATE TRIGGER user_tax_preferences_touch_updated_at
  BEFORE UPDATE ON user_tax_preferences
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- RLS — tenant_isolation, same shape as residency_periods +
-- modelo_720_user_inputs (SEC-020..023).
ALTER TABLE user_tax_preferences ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON user_tax_preferences
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- VESTING_EVENTS — additive columns for the sell-to-cover track ----------
-- AC-5.1.1 + AC-5.2.*. All five columns are additive and NULLABLE (except
-- is_sell_to_cover_override which is NOT NULL DEFAULT false, matching
-- the Slice-3 is_user_override shape). Existing Slice-1/-2/-3 rows carry
-- tax_withholding_percent = NULL, share_sell_price = NULL,
-- share_sell_currency = NULL, is_sell_to_cover_override = false,
-- sell_to_cover_overridden_at = NULL (AC-5.1.2 — no retroactive back-fill).
--
-- No new trigger: the Slice-3 vesting_events_touch_updated_at trigger
-- already bumps updated_at on every UPDATE, which is the OCC token for
-- both Slice-3 FMV edits and Slice-3b sell-to-cover edits (AC-7.4.4).
ALTER TABLE vesting_events
  ADD COLUMN tax_withholding_percent     NUMERIC(5,4)
    CHECK (tax_withholding_percent IS NULL
           OR (tax_withholding_percent >= 0
               AND tax_withholding_percent <= 1)),
  ADD COLUMN share_sell_price            NUMERIC(20,6)
    CHECK (share_sell_price IS NULL OR share_sell_price > 0),
  ADD COLUMN share_sell_currency         TEXT
    CHECK (share_sell_currency IS NULL
           OR share_sell_currency IN ('USD','EUR','GBP')),
  ADD COLUMN is_sell_to_cover_override   BOOLEAN NOT NULL DEFAULT false,
  ADD COLUMN sell_to_cover_overridden_at TIMESTAMPTZ;

-- Cross-field CHECKs:
--   (1) All-or-none on the sell-to-cover triplet (AC-5.2.1).
--       A row carries either all three of (tax_withholding_percent,
--       share_sell_price, share_sell_currency) or none of them. Any
--       partial fill is rejected at the DB level. The handler layer
--       validates this first so the user sees an envelope-shaped 422;
--       the CHECK is defense-in-depth.
--   (2) Override-flag coherence (AC-5.2.2). Mirrors the Slice-3
--       override_flag_coherent CHECK pattern for the Slice-3b track.
ALTER TABLE vesting_events
  ADD CONSTRAINT sell_to_cover_triplet_coherent
    CHECK ((tax_withholding_percent IS NULL
            AND share_sell_price IS NULL
            AND share_sell_currency IS NULL)
        OR (tax_withholding_percent IS NOT NULL
            AND share_sell_price IS NOT NULL
            AND share_sell_currency IS NOT NULL)),
  ADD CONSTRAINT sell_to_cover_override_flag_coherent
    CHECK (is_sell_to_cover_override
           = (sell_to_cover_overridden_at IS NOT NULL));

-- Ownership (mirrors 20260425120000_slice_1.sql §Ownership + Slice 2/3).
ALTER TABLE user_tax_preferences OWNER TO orbit_migrate;

-- Grants — orbit_app.
-- User-scoped table — full DML; RLS constrains visible rows.
GRANT SELECT, INSERT, UPDATE, DELETE ON user_tax_preferences TO orbit_app;
-- vesting_events already grants DML to orbit_app per
-- 20260425120000_slice_1.sql; the column adds inherit those grants.
