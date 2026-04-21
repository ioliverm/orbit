-- Slice 3 T28 — fx_rates reference-data + vesting_events FMV columns
-- + ticker_current_prices + grant_current_price_overrides DDL.
--
-- Traces to:
--   - ADR-017 §1 (authoritative DDL for fx_rates, ticker_current_prices,
--     grant_current_price_overrides; additive columns on vesting_events).
--   - docs/requirements/slice-3-acceptance-criteria.md §4 (ECB pipeline),
--     §5 (paper-gains tile), §6 (M720 threshold), §8 (FMV capture + edit).
--   - ADR-007 (ECB ingestion semantics — fetch, walkback, bootstrap).
--   - ADR-014 §1 (touch_updated_at + tenant_isolation policy convention).
--
-- Scope: one shared reference-data table (fx_rates — NOT RLS-scoped,
-- append-only for orbit_app), two user-scoped tables, five additive
-- columns on vesting_events. No new extensions required.

-- FX_RATES --------------------------------------------------------------
-- ADR-007 authoritative. Slice 3 writes only source='ecb' rows; the
-- 'user_override' source is reserved for Slice-4's calculation-scoped
-- FX-override surface (NOT written in Slice 3; documented here so the
-- CHECK constraint does not require a backwards-incompatible ALTER).
--
-- Unique key is (base, quote, rate_date, source): per ADR-007 the daily
-- fetch is idempotent on the same ECB publication day. `published_at`
-- is our fetch-timestamp for operational forensics; it is NOT the ECB
-- publication moment (that's `rate_date` — the XML's <Cube time>).
--
-- NOT RLS-scoped. Shared reference data across users; same posture as
-- audit_log's append-only GRANT minus UPDATE/DELETE (SEC-103).
CREATE TABLE fx_rates (
  id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  base           TEXT NOT NULL CHECK (base = 'EUR'),
  quote          TEXT NOT NULL CHECK (length(quote) = 3),
  rate_date      DATE NOT NULL,
  rate           NUMERIC(20,10) NOT NULL CHECK (rate > 0),
  source         TEXT NOT NULL CHECK (source IN ('ecb','user_override')),
  published_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (base, quote, rate_date, source)
);

-- Walkback lookup: (quote, rate_date DESC) — the ADR-007 lookup_rate
-- helper scans "give me the most recent rate for USD up to date D".
CREATE INDEX fx_rates_quote_rate_date_idx
  ON fx_rates (quote, rate_date DESC);

-- VESTING_EVENTS — additive columns for FMV capture + override state ----
-- AC-8.2..8.7. The four override columns are additive; Slice-1 rows carry
-- fmv_at_vest = NULL, is_user_override = false, overridden_at = NULL.
-- The fifth column (updated_at) backs AC-10.5 optimistic concurrency;
-- the touch_updated_at trigger on vesting_events is new in Slice 3
-- (Slice 1 only ran this trigger on grants), and it explicitly does NOT
-- touch overridden_at. Handler code sets overridden_at on every override
-- write; the trigger touches updated_at for every write (override or
-- derivation). See ADR-017 §1 rationale.
ALTER TABLE vesting_events
  ADD COLUMN fmv_at_vest     NUMERIC(20,6)
    CHECK (fmv_at_vest IS NULL OR fmv_at_vest > 0),
  ADD COLUMN fmv_currency    TEXT
    CHECK (fmv_currency IS NULL OR fmv_currency IN ('USD','EUR','GBP')),
  ADD COLUMN is_user_override BOOLEAN NOT NULL DEFAULT false,
  ADD COLUMN overridden_at   TIMESTAMPTZ,
  ADD COLUMN updated_at      TIMESTAMPTZ NOT NULL DEFAULT now();

-- Cross-field CHECKs (AC-8.2.4 + AC-8.4.1):
--   (1) fmv_at_vest and fmv_currency must be set together or both NULL.
--   (2) is_user_override TRUE iff overridden_at IS NOT NULL.
-- The second invariant is what enables the handler to query "show me
-- all overrides touched since T" without ambiguity.
ALTER TABLE vesting_events
  ADD CONSTRAINT fmv_pair_coherent
    CHECK ((fmv_at_vest IS NULL) = (fmv_currency IS NULL)),
  ADD CONSTRAINT override_flag_coherent
    CHECK (is_user_override = (overridden_at IS NOT NULL));

-- touch_updated_at on vesting_events. DOES NOT touch overridden_at —
-- that column is handler-owned. The function itself is unchanged
-- (ADR-014 §1 function reused); only the trigger is new.
CREATE TRIGGER vesting_events_touch_updated_at
  BEFORE UPDATE ON vesting_events
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- TICKER_CURRENT_PRICES -------------------------------------------------
-- AC-5.2.1..AC-5.2.6 (Q1). One row per (user, ticker). "Ticker" is
-- stored UPPER-cased + trimmed (handler normalizes on write); the
-- CHECK mirrors grants.ticker's regex verbatim so we never admit a
-- different universe of symbols than the parent grant.
--
-- The unique key on (user_id, ticker) is the upsert target for
-- PUT /api/v1/current-prices/:ticker; a blank save (DELETE) removes
-- the row entirely rather than writing a NULL price. Per AC-5.2.6 no
-- audit_log row is written for current-price edits — current prices
-- are user workspace data, not regulated inputs.
CREATE TABLE ticker_current_prices (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  ticker      TEXT NOT NULL CHECK (ticker ~ '^[A-Z0-9.\-]{1,8}$'),
  price       NUMERIC(20,6) NOT NULL CHECK (price > 0),
  currency    TEXT NOT NULL CHECK (currency IN ('USD','EUR','GBP')),
  entered_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (user_id, ticker)
);

CREATE INDEX ticker_current_prices_user_idx
  ON ticker_current_prices (user_id);

ALTER TABLE ticker_current_prices ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON ticker_current_prices
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- GRANT_CURRENT_PRICE_OVERRIDES ----------------------------------------
-- AC-5.3.1..AC-5.3.5 (Q1). Per-grant override affordance. The UNIQUE
-- key is (grant_id) — one override per grant. user_id is kept on the
-- row for the RLS predicate (the policy cannot read through grant_id
-- without a subquery, which would defeat RLS's point). Handler writes
-- user_id explicitly from app.user_id on every insert; cross-tenant
-- writes are caught by the RLS WITH CHECK predicate, not by a
-- subquery-in-CHECK (which PG disallows).
CREATE TABLE grant_current_price_overrides (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  grant_id    UUID NOT NULL REFERENCES grants(id) ON DELETE CASCADE,
  price       NUMERIC(20,6) NOT NULL CHECK (price > 0),
  currency    TEXT NOT NULL CHECK (currency IN ('USD','EUR','GBP')),
  entered_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (grant_id)
);

CREATE INDEX grant_current_price_overrides_user_idx
  ON grant_current_price_overrides (user_id);

ALTER TABLE grant_current_price_overrides ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON grant_current_price_overrides
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- Ownership (mirrors 20260425120000_slice_1.sql §Ownership + Slice 2).
ALTER TABLE fx_rates                        OWNER TO orbit_migrate;
ALTER TABLE ticker_current_prices           OWNER TO orbit_migrate;
ALTER TABLE grant_current_price_overrides   OWNER TO orbit_migrate;

-- Grants — orbit_app.
--
-- fx_rates is reference data. Handlers and the ECB worker read it;
-- the worker INSERTs on every successful fetch. No UPDATE, no DELETE.
-- Same posture as audit_log (SEC-103 — append-only from orbit_app).
GRANT SELECT, INSERT ON fx_rates TO orbit_app;

-- User-scoped tables — full DML; RLS constrains visible rows.
GRANT SELECT, INSERT, UPDATE, DELETE ON ticker_current_prices         TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON grant_current_price_overrides TO orbit_app;
-- vesting_events already grants DML to orbit_app per
-- 20260425120000_slice_1.sql; the column adds inherit those grants.
