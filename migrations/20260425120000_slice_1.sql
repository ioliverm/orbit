-- Slice 1 additions migration.
--
-- Traces to:
--   - ADR-014 §1 (authoritative DDL for residency_periods, grants,
--     vesting_events, including RLS policies, CHECK constraints, cross-field
--     constraints, and the touch_updated_at trigger).
--   - docs/requirements/slice-1-acceptance-criteria.md §4.1 (residency),
--     §4.2 (grant form), §4.3 (vesting derivation).
--
-- Scope: the three user-scoped tables that Slice 1 needs — residency_periods,
-- grants, vesting_events. Every table is `ENABLE ROW LEVEL SECURITY` with a
-- `tenant_isolation` policy keyed off `app.user_id` (SEC-020..023). The
-- `pgcrypto` and `citext` extensions are created in
-- 20260418120000_init.sql; do not redeclare.

-- migrations/20260425120000_slice_1.sql (Slice 1 additions)

-- RESIDENCY PERIODS ------------------------------------------------------
CREATE TABLE residency_periods (
  id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  jurisdiction       TEXT NOT NULL CHECK (jurisdiction IN ('ES','UK')),
  sub_jurisdiction   TEXT,                               -- autonomía code 'ES-MD', 'ES-PV', ...
  from_date          DATE NOT NULL,
  to_date            DATE,                               -- NULL = current
  regime_flags       TEXT[] NOT NULL DEFAULT '{}'        -- 'beckham_law','foral_pais_vasco','foral_navarra'
                     CHECK (regime_flags <@ ARRAY['beckham_law','foral_pais_vasco','foral_navarra']::text[]),
  created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX residency_periods_user_current_idx
  ON residency_periods (user_id) WHERE to_date IS NULL;

ALTER TABLE residency_periods ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON residency_periods
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- GRANTS -----------------------------------------------------------------
CREATE TABLE grants (
  id                              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  instrument                      TEXT NOT NULL CHECK (instrument IN ('rsu','nso','espp','iso_mapped_to_nso')),
  grant_date                      DATE NOT NULL,
  share_count                     NUMERIC(20,4) NOT NULL CHECK (share_count > 0),
  strike_amount                   NUMERIC(20,6),
  strike_currency                 TEXT CHECK (strike_currency IN ('USD','EUR','GBP')),
  vesting_start                   DATE NOT NULL,
  vesting_total_months            INTEGER NOT NULL CHECK (vesting_total_months > 0 AND vesting_total_months <= 240),
  cliff_months                    INTEGER NOT NULL DEFAULT 0 CHECK (cliff_months >= 0),
  vesting_cadence                 TEXT NOT NULL CHECK (vesting_cadence IN ('monthly','quarterly')),
  double_trigger                  BOOLEAN NOT NULL DEFAULT false,
  liquidity_event_date            DATE,
  double_trigger_satisfied_by     TEXT CHECK (double_trigger_satisfied_by IN ('ipo','acquisition','tender_offer_transacted')),
  employer_name                   TEXT NOT NULL CHECK (length(employer_name) BETWEEN 1 AND 256),
  ticker                          TEXT CHECK (ticker ~ '^[A-Z0-9.\-]{1,8}$'),
  notes                           TEXT CHECK (notes IS NULL OR length(notes) <= 2048),
  created_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now(),
  -- Cross-field constraints
  CONSTRAINT cliff_not_greater_than_total CHECK (cliff_months <= vesting_total_months),
  CONSTRAINT strike_required_for_options CHECK (
    (instrument IN ('nso','iso_mapped_to_nso') AND strike_amount IS NOT NULL AND strike_currency IS NOT NULL)
    OR instrument IN ('rsu','espp')
  ),
  CONSTRAINT double_trigger_only_on_rsu CHECK (
    double_trigger = false OR instrument = 'rsu'
  )
);
CREATE INDEX grants_user_id_idx ON grants (user_id);

ALTER TABLE grants ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON grants
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- VESTING EVENTS (derived, cached per grant) ------------------------------
CREATE TABLE vesting_events (
  id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  grant_id             UUID NOT NULL REFERENCES grants(id) ON DELETE CASCADE,
  vest_date            DATE NOT NULL,
  shares_vested_this_event NUMERIC(20,4) NOT NULL,
  cumulative_shares_vested NUMERIC(20,4) NOT NULL,
  state                TEXT NOT NULL CHECK (state IN ('upcoming','time_vested_awaiting_liquidity','vested')),
  computed_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (grant_id, vest_date)
);
CREATE INDEX vesting_events_grant_id_date_idx ON vesting_events (grant_id, vest_date);

ALTER TABLE vesting_events ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON vesting_events
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- TRIGGER: updated_at on grants
CREATE OR REPLACE FUNCTION touch_updated_at()
RETURNS TRIGGER AS $$ BEGIN NEW.updated_at := now(); RETURN NEW; END; $$ LANGUAGE plpgsql;
CREATE TRIGGER grants_touch_updated_at
  BEFORE UPDATE ON grants FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- ---------------------------------------------------------------------------
-- Ownership — mirrors 20260418120000_init.sql §4, so subsequent migrations
-- running as orbit_migrate can ALTER these tables.
-- ---------------------------------------------------------------------------
ALTER TABLE residency_periods OWNER TO orbit_migrate;
ALTER TABLE grants            OWNER TO orbit_migrate;
ALTER TABLE vesting_events    OWNER TO orbit_migrate;
ALTER FUNCTION touch_updated_at() OWNER TO orbit_migrate;

-- ---------------------------------------------------------------------------
-- Grants — orbit_app
--   Full DML on the user-scoped tables; RLS policies above constrain the
--   visible row set to the owning user (SEC-020..023).
-- ---------------------------------------------------------------------------
GRANT SELECT, INSERT, UPDATE, DELETE ON residency_periods TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON grants            TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON vesting_events    TO orbit_app;
