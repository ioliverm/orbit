# ADR-017: Slice-3 technical design

- **Status:** Proposed
- **Date:** 2026-04-19
- **Deciders:** Ivan (owner)
- **Traces to:** `docs/requirements/slice-3-acceptance-criteria.md` (authoritative for this slice), `docs/requirements/v1-slice-plan.md` v1.4 (Slice 3 non-goals — in particular Q2 deferring GeoIP to Slice 9 and Q1/Q3/Q4 product-owner locks), ADR-005 (entity outline — `fx_rates`, `nso_exercises`), ADR-007 (ECB pipeline — authoritative for fetch, walkback, 90-day bootstrap, fetch-on-demand), ADR-009 (frontend), ADR-010 (API envelope + route prefix — unchanged), ADR-014 (Slice-1 DDL — `grants`, `vesting_events`, `residency_periods`, `users`, `sessions`, `audit_log`), ADR-015 (local-only), ADR-016 (Slice-2 DDL — `espp_purchases`, `art_7p_trips`, `modelo_720_user_inputs`, `sessions.country_iso2`; tenant_isolation convention; touch_updated_at trigger), SEC-020..SEC-026 (RLS), SEC-050 (log allowlist), SEC-054 (ip_hash HMAC), SEC-100..SEC-103 (audit log), SEC-160..SEC-163 (rate limit + validation), spec `docs/specs/orbit-v1-persona-b-spain.md` §US-007 + L319/L334 (RSU basis = FMV-at-vest). UX refs `dashboard.html` (paper-gains tile target + M720 banner slot), `grant-detail.html` ("Precios de vesting" section host + per-grant override), `profile.html` (Modelo 720 panel + threshold banner).

## Context

Slice 3's boundary is explicit per the AC doc's header: **ECB FX ingestion pipeline · dashboard paper-gains tile (EUR, bands 0 % / 1.5 % / 3 %) · Modelo 720 threshold alert (US-007 ACs 1/2/4 — banner only) · rule-set chip in footer on FX-dependent surfaces · per-vest FMV capture on `vesting_events` · editable past vesting events on grant-detail with override preservation · user-entered current price per ticker + per-grant override.** No Stripe, no billing, no tax math, no IRPF projection, no Modelo 720 worksheet PDF, no scenarios, no sell-now, no market-data vendor, no NSO exercise-FMV, no Art. 7.p eligibility evaluation, no `sessions.country_iso2` population.

Slice 3 is implementation-ready on the requirements side but leaves concrete DDL for four net-new database surfaces, the override-preservation rules for `orbit_core::vesting::derive_vesting_events`, the first real `orbit-worker` job (the ECB fetcher), the paper-gains pure function (backend authoritative + frontend parity mirror), the full API surface across seven endpoint families, and the sequence-diagram shape of the two load-bearing flows (scheduled ECB fetch and override-then-bulk-fill). This ADR produces all of those, traces every AC to a component or decision, and enumerates exactly what Slice 3 defers so the implementation engineer never sees a TBD.

Five load-bearing inputs from Slices 1–2 and from the v1.4 plan carry forward unchanged:

- **C-4 retired in this slice.** EUR conversion lands now. Share counts continue to flow through the `vesting_events` / `espp_purchases` / `grants` surface unchanged; the paper-gains tile is the only Slice-3 surface that materializes EUR numbers from FX and FMV together (the M720 threshold banner reuses the same derivation).
- **Slice-2 stacked envelope unchanged** (AC-8.2.8 Slice 2). Slice 3 adds no new algorithmic surface to the multi-grant dashboard beyond the paper-gains tile that sits above it. The envelope renders whatever `vesting_events.shares_vested_this_event` says; overrides therefore flow through the envelope without special-casing per AC-8.5.4.
- **ADR-007 is authoritative** for ECB pipeline semantics. ADR-017 translates those semantics into concrete DDL, worker code shape, and handler contracts; it does not re-decide fetch cadence, walkback depth, bootstrap trigger, or fetch-on-demand timeout.
- **Q2 supersedes ADR-016 §9.2.** `sessions.country_iso2` is *reserved* in Slice 2 and *populated* in Slice 9. ADR-017 does not touch the column; no Slice-3 code path writes it. The Slice-2 schema is complete.
- **Q1/Q3/Q4 locks** are design inputs: (Q1) per-ticker current price with per-grant override is the shape of the input surface — two new tables, not one; (Q3) partial-data banner ships a partial result rather than an all-or-nothing fallback; (Q4) bulk-fill SKIPS rows that already carry FMV — no overwrite.

One architectural-compromise retirement carries forward from Slice 2 into Slice 3: the Slice-1 `grants.notes` ESPP lift specified in ADR-016 §2 continues to work unchanged. Slice 3 does not touch `grants.notes`. The ESPP basis surface is unambiguously `espp_purchases.fmv_at_purchase` (Slice 2).

## Decision

### 1. Slice-3 DDL (concrete)

All migrations live under `migrations/`. Numbering is `YYYYMMDDHHMMSS_label.sql` and must sort strictly after `20260516120000_slice_2.sql`. Slice 3 appends one migration: `20260523120000_slice_3.sql` (ISO timestamp chosen one week after Slice-2).

Slice 3 adds **one shared reference-data table** (`fx_rates`), **two user-scoped tables** (`ticker_current_prices`, `grant_current_price_overrides`), **four additive columns on `vesting_events`** (`fmv_at_vest`, `fmv_currency`, `is_user_override`, `overridden_at`), and an `updated_at` column on `vesting_events` to back optimistic concurrency per AC-10.5. All other Slice-1/-2 tables are left untouched.

```sql
-- migrations/20260523120000_slice_3.sql (Slice 3 additions)
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
-- audit_log's FOR ALL TO orbit_app GRANT minus UPDATE/DELETE (SEC-103).
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
-- AC-8.2..8.7. The four columns are additive; Slice-1 rows carry
-- fmv_at_vest = NULL, is_user_override = false, overridden_at = NULL.
-- The derivation algorithm (orbit_core::vesting::derive_vesting_events)
-- gains a preservation rule keyed on is_user_override — see §2.
--
-- updated_at is added to back AC-10.5 optimistic concurrency; the
-- touch_updated_at trigger on vesting_events is new (Slice 1 only
-- ran this trigger on grants), and it explicitly does NOT touch
-- overridden_at. Handler code sets overridden_at on every override
-- write; the trigger touches updated_at for every write (override
-- or derivation). See §2 for the rationale.
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
-- that column is handler-owned. The function itself is untouched
-- (ADR-014 §1 function reused); the trigger is new.
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
GRANT SELECT, INSERT, UPDATE, DELETE ON ticker_current_prices        TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON grant_current_price_overrides TO orbit_app;
-- vesting_events already grants DML to orbit_app per 20260425120000_slice_1.sql;
-- the column adds inherit those grants.
```

**RLS policy naming convention (inherited).** The two new user-scoped tables each carry one policy named `tenant_isolation`. The SEC-020 CI `pg_policies` introspection test extends with the two new table names in the expected-set fixture; `fx_rates` is explicitly NOT in the set (reference data, SEC-023 cross-tenant probe is not applicable — §8 below).

**Why `fx_rates.source` is a CHECK and not an enum.** ADR-007 will, in Slice 4, add a `user_override` row path when the tax engine's calculation-scoped FX override lands. Storing `source` as `TEXT` + `CHECK IN (...)` mirrors the `grants.instrument` and `modelo_720_user_inputs.category` shape inherited from Slice 1/2; growing the allowed set is a one-line `ALTER TABLE ... DROP CONSTRAINT ... ADD CONSTRAINT ...` migration with no data rewrite. Boring, reversible, consistent.

**Why `fx_rates` is NOT RLS-scoped.** ECB FX rates are public reference data. Every user sees the same rate for 2026-04-17. Applying RLS would require a user_id column on every row (there is no owner — the rate is a global fact) or an unconditional `USING (true)` policy (pointless). The boring answer is "no RLS, no user_id, GRANT SELECT + INSERT to orbit_app so the handler can run a fetch-on-demand fallback per AC-4.4.1 without needing the worker's `orbit_migrate` privileges." The reference-data posture is the same as `rule_sets` will be in Slice 4.

**Why append-only on `fx_rates`.** Two reasons: (1) ADR-007 idempotency — a repeated fetch on the same day is a no-op via the unique key, not an UPDATE; (2) reproducibility — Slice 4's calculation stamping will reference `fx_rates.id` on every calculation row, and UPDATE would invalidate that reference. `GRANT SELECT, INSERT` (no UPDATE, no DELETE) on orbit_app codifies the intent at the DB level. Same pattern as audit_log (SEC-103).

**Why two tables (`ticker_current_prices` + `grant_current_price_overrides`) and not one `current_prices` table with nullable `grant_id`.** The per-ticker surface is primary (AC-5.2.1 — the dialog lists tickers, not grants); the per-grant override is secondary (AC-5.3.1 — an escape-hatch on grant-detail). Two tables keep the UNIQUE key semantics trivial — `(user_id, ticker)` vs `(grant_id)` — and avoid a partial-unique-index shape that has to express "either ticker is set and grant_id is NULL or vice versa". The cost (two repos, two handlers) is minimal; the benefit (no overloaded-row confusion) is clear. See §10 Alternatives.

**Why `fmv_at_vest NUMERIC(20,6)` and not `NUMERIC(20,4)` like `shares_vested_this_event`.** The display contract asks for "up to 4 decimal places" (AC-8.2.4, AC-G-13) but FMV is a price-per-share, not a share count; precision headroom matters for (price − basis) × shares arithmetic where a 5th-decimal in `price` survives the subtract if `basis` is 4-decimal and shares are 4-decimal. Six decimals matches `espp_purchases.fmv_at_purchase` (Slice 2) — one less novel thing. The frontend renders 4 decimals and the handler accepts up to 4 on input; the column headroom is purely for arithmetic.

**Why `updated_at` on `vesting_events` (new in Slice 3).** AC-10.5 specifies optimistic concurrency on vesting-event edits. The check is a `WHERE id = $1 AND updated_at = $2` predicate in the UPDATE statement (implementation pattern below); the trigger maintains `updated_at` on every write. The column has a `NOT NULL DEFAULT now()` to keep the backfill story trivial — Slice-1 rows acquire a post-migration `updated_at = now()` at migration time, and the first override edit sees a matching `updated_at` (no pre-existing 409s on legacy rows).

**Why a separate `overridden_at` instead of reusing `updated_at`.** The two columns carry different semantics. `updated_at` is "when the row was last touched by any writer" (handler override, bulk-fill handler, grant-re-derivation handler — the last specifically writes non-overridden rows without marking them `is_user_override = true`). `overridden_at` is "when the user last explicitly overrode this row" — it is the timeline a Slice-4 audit view would display. Coupling them would muddle both: a grant-edit's re-derivation path writes non-overridden rows (which updates `updated_at`) but must not imply the user edited them. The cross-field CHECK `is_user_override = (overridden_at IS NOT NULL)` makes the invariant explicit and testable.

**Why the `touch_updated_at` trigger on `vesting_events` does NOT touch `overridden_at`.** The default `touch_updated_at` function writes `NEW.updated_at := now()`; it does not touch other columns. Reusing it verbatim (not forking a `vesting_events_specific_touch` function) keeps the function definition shared across tables — Slice-4/5/6 tables can adopt it without change. Handler code sets `overridden_at = now()` in the `UPDATE ... SET` list explicitly on every override write; the `clearOverride: true` path sets it to `NULL` explicitly. The `override_flag_coherent` CHECK catches any handler bug that forgets to pair the two.

### 2. `orbit_core::vesting::derive_vesting_events` extension

The load-bearing logic change in Slice 3. The function's signature grows one parameter; its behavior in the no-override case is unchanged; its behavior in the override case is pinned here and tested via the shared fixture.

**Signature extension.**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VestingEventOverride {
    pub vest_date: NaiveDate,
    pub shares_vested_this_event: Shares,
    pub fmv_at_vest: Option<Fmv>,         // None when user cleared FMV
    pub fmv_currency: Option<Currency>,   // paired with fmv_at_vest per CHECK
    pub original_derivation_index: usize, // the position this row held
                                          // in the previous derivation output,
                                          // used for deterministic ordering
                                          // when vest_date ties (§ below)
}

pub fn derive_vesting_events(
    grant: &GrantInput,
    today: NaiveDate,
    existing_overrides: &[VestingEventOverride],
) -> Result<Vec<VestingEvent>, VestingError> { ... }
```

`Fmv` is a thin wrapper around `rust_decimal::Decimal` (or the existing scaled-integer shape used for shares — TBD by the implementation engineer; the important property is parity with the frontend's `decimal.js` representation). `Currency` is the existing `{USD, EUR, GBP}` enum. The frontend parity type mirrors this shape.

**Preservation rules.**

1. **No overrides → no change.** When `existing_overrides.is_empty()`, the function is bit-identical to its Slice-1 form: pure derivation from `grant`, cumulative invariant asserted (`debug_assert_eq!(cumulative, total)` survives).

2. **Any overrides present → preservation dominates.**
   - Every row in `existing_overrides` is returned **verbatim** at its overridden `vest_date`, carrying its overridden `shares_vested_this_event`, `fmv_at_vest`, and `fmv_currency`. The returned `VestingEvent`'s `state` is recomputed against `today` (an override whose `vest_date` is still in the future renders as `Upcoming`; one that has passed renders as `Vested` / `TimeVestedAwaitingLiquidity`).
   - For every date slot the derivation algorithm would produce **that is in the past** (`vest_date ≤ today`) and **that is NOT overridden**, the row is NOT re-derived against the current grant params — it is retained as the existing event row held in the DB (the caller passes the existing rows alongside `existing_overrides` via the same `events_by_grant` read the re-derivation handler performs). See AC-8.4.4: past non-overridden rows remain computed from the derivation algorithm **as it ran at their original-write time**. In practice the caller passes the current `vesting_events` rows to this function and the function identifies which rows it can legitimately overwrite (non-overridden AND future).
   - For every date slot the derivation algorithm produces **that is in the future** (`vest_date > today`), the function computes the row from current grant params. This is the "re-derive futures" branch.

3. **Cumulative invariant relaxed when any override exists.** Per AC-8.5.2: when `existing_overrides.is_empty() == false`, `SUM(shares_vested_this_event) MAY differ from grant.share_count`. The Slice-1 `debug_assert_eq!` is gated on the override-empty case; when overrides are present, the function **does not assert** and **does not rebalance** futures to meet the share-count target. The user's edits win. A prominent `// AC-8.5.2: invariant relaxed when overrides present` comment pins this at the call site.

4. **Deterministic ordering.** The returned `Vec<VestingEvent>` is sorted by `(vest_date ASC, original_derivation_index ASC)`. Overridden rows carry an `original_derivation_index` (the index of the event they replaced in the previous derivation); re-derived futures carry monotonic indices past the highest overridden index. The tie-break prevents two overrides on the same `vest_date` from swapping render order between runs.

**Pseudo-code sketch** (Rust-flavored; the one-line shape for the report):

```text
events = []
for each slot in derive_algorithm(grant, today):
    if slot matches an override in existing_overrides:
        events.push(override.into_event(today))  # verbatim + state_for()
    elif slot.vest_date <= today:
        events.push(existing_row_from_db(slot))  # retain past as-is
    else:
        events.push(slot.into_event(today))      # re-derive futures
sort events by (vest_date, original_derivation_index)
return events
```

The "matches an override" check keys on the override's `vest_date` — that is what makes the override durable against grant-param changes (if the user overrode the 2025-09-15 row and later the grant's `vesting_start` shifts by a month, the 2025-09-15 override survives even if the derivation would now put no event there; see AC-8.4.2 "preserved in place"). When a grant-param change causes the derivation to produce fewer events than the override set (e.g., `vesting_total_months` shortened), the override rows still survive — the algorithm cannot delete them — but a UI banner per AC-8.5.3 tells the user the curve is manually-adjusted.

**Shared JSON fixture.** The Slice-1 `vesting_cases.json` grows a companion `vesting_override_cases.json` in `backend/crates/orbit-core/tests/fixtures/`. Six to eight cases, each with input grant params, input overrides, `today`, and expected output:

- **override-in-middle** — one override on a middle past row; the rest derived.
- **override-all** — every past row overridden; futures derived.
- **override-outside-window** — an override whose `vest_date` falls outside the current `[grant_date, grant_date + vesting_total_months]` window (e.g., user shifted the date by grant-edit later); override survives; banner condition is true.
- **override-currency-mismatch** — two past overrides, one `USD` and one `GBP`; handler-layer validation gates this at write-time but the derivation function itself is currency-agnostic.
- **override-preserved-across-shortening** — `vesting_total_months` shortened below the overridden rows' positions; all overrides survive; invariant-relaxation banner fires.
- **override-preserved-across-cliff-change** — `cliff_months` bumped; overrides survive; futures re-derive against new cliff.
- **override-preserved-across-cadence-change** — monthly → quarterly; overrides survive; futures re-derive on new cadence.
- **override-with-fmv-only** — a future-row override where only `fmv_at_vest` changed (AC-8.3.1); `vest_date` and `shares` remain the derivation's output but the row is marked `is_user_override = true` (the "FMV-only" branch is in the handler, not in `derive_vesting_events` per se — this fixture case exercises the function receiving an override whose `vest_date` and `shares` equal the algorithm's output).

**Property test** (in addition to the fixture tests). `derive_vesting_events` is **stable under re-derivation**: calling it twice on the same `(grant, today, existing_overrides)` input produces bit-identical output, and calling it with the output of the first call as the `existing_overrides` of the second (after promoting every past row to an override) also produces bit-identical output. The Rust `proptest` test uses a small generator covering `share_count`, `vesting_total_months`, `cliff_months`, `cadence`, and a random override-masking on past rows.

### 3. API contract additions

Concrete for Slice 3. Path-relative to `/api/v1`. Notation inherited from ADR-010 §9 and ADR-016 §3: `[A]` = authenticated; `[V]` = CSRF-validated state change. All mutation endpoints go through `Tx::for_user(user_id)` per SEC-022. `fx_rates` endpoints are `[A]` but **not** RLS-scoped (reference data — a handler uses `Tx::anonymous_read()` or a direct pool query; the implementation engineer picks — but the audit_log and app.user_id settings do NOT flow here).

**FX-facing**

| Method | Path | Notes |
|---|---|---|
| `GET` | `/fx/rate?quote=USD&on=YYYY-MM-DD` `[A]` | Reference-data read (not RLS-scoped). `on` defaults to today if omitted. Handler calls `orbit_db::fx_rates::lookup_walkback(pool, quote, on, max_walkback_days = 7)`; returns `{ quote: "USD", rateDate: "YYYY-MM-DD", rate: "1.0823", walkback: 0..=7, staleness: "fresh" \| "walkback" \| "stale" \| "unavailable" }`. `"stale"` fires at `walkback ≥ 3` per AC-4.5.3; `"walkback"` at `1..=2`. On `walkback = 7` with no row returns `{ ..., staleness: "unavailable", rate: null }` (no body-level error; this is the AC-4.2.3 / AC-4.5.4 branch). No audit row. Rate-limited as a read (SEC-160 defaults). |
| `GET` | `/fx/latest?quote=USD` `[A]` | Convenience wrapper; equivalent to `/fx/rate?quote=USD&on=<today>`. |

**Ticker current prices (Q1)**

| Method | Path | Notes |
|---|---|---|
| `GET` | `/current-prices` `[A]` | Returns `{ prices: [{ ticker, price, currency, enteredAt }] }`, sorted by `ticker` ASC. Read from `ticker_current_prices` scoped by RLS. |
| `PUT` | `/current-prices/:ticker` `[A]` `[V]` | Upsert. Body: `{ price: "45.00", currency: "USD" }`. Ticker in path is normalized (UPPER + trim); mismatched body ticker → 422. Validator: AC-5.2.4 (`price > 0`), currency ∈ {USD, EUR, GBP}, ticker matches `^[A-Z0-9.\-]{1,8}$`. `INSERT ... ON CONFLICT (user_id, ticker) DO UPDATE SET price = EXCLUDED.price, currency = EXCLUDED.currency, entered_at = now()`. Response: `200 { ticker, price, currency, enteredAt }`. **No audit row** per AC-5.2.6. |
| `DELETE` | `/current-prices/:ticker` `[A]` `[V]` | Remove. 204. No audit row. Subsequent dashboard render treats all grants on this ticker as "price unknown" (AC-5.2.5). |
| `GET` | `/grants/:id/current-price-override` `[A]` | Returns `{ override: { price, currency, enteredAt } \| null }`. 404 if the grant is out of the caller's RLS scope. |
| `PUT` | `/grants/:id/current-price-override` `[A]` `[V]` | Upsert. Body: `{ price, currency }`. Same validators as the ticker endpoint. No audit row. |
| `DELETE` | `/grants/:id/current-price-override` `[A]` `[V]` | Remove. 204. |

**Paper-gains summary**

| Method | Path | Notes |
|---|---|---|
| `GET` | `/dashboard/paper-gains` `[A]` | Server-side computed summary. Response: `{ perGrant: [ { grantId, employer, instrument, complete: bool, nativeCurrency, gainNative: "…" \| null, gainEurBand: { low, mid, high } \| null, missingReason: "fmv_missing" \| "no_current_price" \| "nso_deferred" \| "double_trigger_pre_liquidity" \| null } ], combinedEurBand: { low, mid, high } \| null, incompleteGrants: [ { grantId, employer } ], stalenessFx: "fresh" \| "walkback" \| "stale" \| "unavailable", fxDate: "YYYY-MM-DD" \| null }`. The `incompleteGrants` drives the partial-data banner (AC-5.5.1). `combinedEurBand` is `null` only in the AC-5.5.4 ECB-unavailable state or the AC-5.5.5 all-excluded state; otherwise the low/mid/high aggregate across the "complete" grants. See §5 for the full pure-function specification. |

**Modelo 720 threshold**

| Method | Path | Notes |
|---|---|---|
| `GET` | `/dashboard/modelo-720-threshold` `[A]` | Response: `{ bankAccountsEur: "25000.00" \| null, realEstateEur: "0.00" \| null, securitiesEur: "…" \| null, perCategoryBreach: bool, aggregateBreach: bool, thresholdEur: "50000.00", fxSensitivityBand: { low, mid, high } \| null, fxDate: "YYYY-MM-DD" \| null }`. `securitiesEur = null` when FMV data is incomplete for any eligible grant (AC-6.1.1 — a footnote in the M720 panel already prepares the user for this). `perCategoryBreach = true` fires the AC-6.2.1 "per-category" banner variant; `aggregateBreach = true` without `perCategoryBreach` fires the AC-6.2.2 "aggregate" variant. `fxSensitivityBand` is computed when any category is within ±5 % of €50 000 (AC-6.2.4). |

**Vesting-event overrides (the v1.4 meat)**

| Method | Path | Notes |
|---|---|---|
| `PUT` | `/grants/:grantId/vesting-events/:eventId` `[A]` `[V]` | Body (camelCase DTO): `{ vestDate?: "YYYY-MM-DD", sharesVested?: "…", fmvAtVest?: "…" \| null, fmvCurrency?: "USD" \| "EUR" \| "GBP" \| null, clearOverride?: bool, updatedAt: "YYYY-MM-DDTHH:MM:SS.sssZ" }`. Validators: AC-8.2.2 (vest_date in window + `<= today + 365d`), AC-8.2.3 (shares > 0, ≤ `grants.share_count`), AC-8.2.4 (fmv > 0, 4 dp), AC-8.3.1 (future rows reject `vestDate`/`sharesVested` changes — 422 with `code = "vesting_event.future_row.immutable_schedule"`). Optimistic-concurrency predicate: `updated_at` must match the DB row's current `updated_at` (AC-10.5 — 409 with `code = "resource.stale_client_state"`). On success: sets `is_user_override = true`, `overridden_at = now()`, and the fields in the body. Audit `vesting_event.override` per G-32 with `payload_summary = { grant_id, fields_changed: ["vest_date" \| "shares" \| "fmv"] }` — the array lists only the fields that actually changed. **`clearOverride: true` semantics** (AC-8.7.1): server reverts `vest_date` and `shares_vested_this_event` to the algorithm's current output for that row; preserves `fmv_at_vest` and `fmv_currency`; sets `is_user_override = false` iff `fmv_at_vest IS NULL` after the revert (else leaves `is_user_override = true` because FMV is still a manual edit). Audit `vesting_event.clear_override` with `payload_summary = { grant_id, cleared_fields: ["vest_date", "shares"], preserved: ["fmv"] \| [] }`. |
| `POST` | `/grants/:grantId/vesting-events/bulk-fmv` `[A]` `[V]` | Body: `{ fmv: "40.00", currency: "USD" }`. Validator: `fmv > 0`, 4 dp; currency in set. Handler: single transaction; updates all `vesting_events` for the grant where `fmv_at_vest IS NULL`, setting `fmv_at_vest = fmv`, `fmv_currency = currency`, `is_user_override = true`, `overridden_at = now()`. Rows with `fmv_at_vest IS NOT NULL` are **skipped** (Q4 — even if `is_user_override = false`; the non-null state is the gate, not the flag). Response: `200 { appliedCount: X, skippedCount: Y }` matching AC-8.6.3 copy inputs. One `vesting_event.override` audit row per modified row (AC-8.6.4). Partial-transaction-failure rolls back everything (AC-10.7). Rate-limited per §8 (10/min/user). |

**Rule-set chip data source**

| Method | Path | Notes |
|---|---|---|
| `GET` | `/rule-set-chip` `[A]` | Response: `{ fxDate: "YYYY-MM-DD" \| null, stalenessDays: 0..7 \| null, engineVersion: "0.3.0" }`. `fxDate` and `stalenessDays` come from `lookup_walkback(tx, "USD", today, 7)`; `engineVersion` is the backend-compile-time const `env!("CARGO_PKG_VERSION")` of `orbit-api`. `fxDate = null` + `stalenessDays = null` maps to the AC-4.5.4 chip-suppressed state. Rate-limited as a read. |

**Error envelope.** Unchanged from ADR-010 §7 and ADR-016 §3. Every validator error surfaces via the `errors: [{ field, code, message, messageEn }]` multi-field shape. New Slice-3 `code` values: `fx.rate.out_of_range` (quote currency not in Slice-3 set), `current_price.invalid.price`, `current_price.invalid.currency`, `current_price.invalid.ticker`, `vesting_event.future_row.immutable_schedule` (AC-8.3.1), `vesting_event.out_of_window` (AC-8.2.2), `vesting_event.shares_exceed_grant` (AC-8.2.3), `vesting_event.fmv_pair_incoherent` (CHECK passthrough — handler validator should gate this first; included for defense-in-depth), `grant.share_count_below_overrides` (AC-8.9.1), `resource.stale_client_state` (AC-10.5).

**Rate-limit headers.** Unchanged (SEC-160). New per-endpoint limits in §8.

### 4. ECB worker — first real `orbit-worker` job

> **Amendment (Slice 3 T33, 2026-04-21).** Payload shapes in this
> section were reconciled to the shipped worker schema. The pre-T33
> draft described `fx.fetch_success = { kind, rate_count, oldest_date,
> newest_date }` and an `attempt_number` key on `fx.fetch_failure`;
> neither shipped. The actual shapes are pinned below (also asserted
> by the T31 audit-allowlist sweep in
> `backend/crates/orbit-api/tests/audit_allowlist_sweep_slice_3.rs`).
> Code is unchanged — this is a doc-only edit.

The `orbit-worker` crate currently carries a `lib.rs` stub (Slice-0a scaffold). Slice 3 wires the first real job.

**CLI shape.** Two entry points on a single `WorkerCli` struct:

```text
orbit worker                 # long-running; runs the scheduler loop.
orbit worker --once fx       # ad-hoc; runs one fetch, exits with 0/1.
orbit worker --once bootstrap  # ad-hoc; runs the 90-day bootstrap, exits.
```

The `--once` flags are for developer use (demo-script step 2 runs `--once bootstrap` for the first startup; step 3 fast-forwards by running `--once fx`). The long-running command is what prod ops deploys at Slice 9.

**Scheduler.** In-process, `tokio::time::sleep_until` against a daily-at-17:00-Europe/Madrid target. The computed wake-up is `next_17_00_madrid(now)`; on wake, the fetch runs, then the scheduler sleeps until the next 17:00. No external cron, no `tokio-cron-scheduler` dep. Simpler, boring, easier to reason about (§7 of ADR-007's considered options argued against external cron; we pick the simplest thing that works for an in-process scheduler too). The implementation engineer is free to swap to `tokio-cron-scheduler` if the sleep-until approach develops subtle drift problems, but the ADR picks the simpler shape and flags drift as an acceptable risk at Slice-3 scale (one scheduled fetch per day).

**Fetch.** `reqwest::Client::builder().timeout(Duration::from_secs(5)).build()`, then `client.get("https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml").send().await?.text().await?`. No custom User-Agent beyond reqwest's default (G-28). Parse with `quick-xml`: the daily file is a tiny XML with a root `<gesmes:Envelope>` containing `<Cube><Cube time="YYYY-MM-DD"><Cube currency="USD" rate="X.XXXX"/> …</Cube></Cube>`. Defensive parser: extract `time` from the inner `<Cube>` and iterate every `<Cube currency= rate=>` child; rate is `rate_date = parsed_time`, `rate = parsed_rate`, `base = "EUR"`, `quote = parsed_currency`, `source = "ecb"`.

**Upsert.** `orbit_db::fx_rates::insert_daily(tx, rows) -> Result<usize, DbError>` wraps `INSERT INTO fx_rates (...) VALUES (...) ON CONFLICT (base, quote, rate_date, source) DO NOTHING` and returns the number of rows inserted. The idempotency gate is the unique key; a same-day repeat fetch writes zero rows (AC-4.1.3). This is the Slice-1 audit_log insert pattern — no UPSERT-with-update; "already there" is the null result.

**Bootstrap.** On worker startup, `SELECT COUNT(*) FROM fx_rates WHERE quote = 'USD' AND rate_date >= (CURRENT_DATE - INTERVAL '90 days')`. If the count is below a threshold (ADR-017 picks **`< 30`** — the 90-day file typically yields ~60 business-day rows, so <30 means a cold DB or a very-stale restart), the worker fetches `https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist-90d.xml` once, parses it (same structure, just with many `<Cube time="…">` inner nodes), and bulk-inserts via the same idempotent helper. Emits `fx.bootstrap_success` audit row with `payload_summary = { kind: "bootstrap", quote_currencies: ["USD"], rows_inserted, publication_date?, span_days?, historical_file: "eurofxref-hist-90d" }` (optional fields present when at least one row landed). AC-4.3.3 (warm-restart-is-a-no-op): the threshold check runs on every startup; a warm DB (count ≥ 30) short-circuits without a network call.

**Failure policy.** Retry with exponential backoff: 1s, 5s, 25s (three attempts, five-second per-attempt timeout). On all three failing, write `fx.fetch_failure` audit row with `payload_summary = { reason, kind, attempted_at_minute: "HH:MM" }` — `reason` classifies into `{ "timeout", "network", "parse", "db" }` per the worker's `FetchError::classify`. Per ADR-007 §ECB unreachable, on two consecutive failed fetch runs a log line at WARN level is emitted (`orbit_log::event!(WARN, "fx.fetch_persistent_failure", run_count = N)`) — no alerting route ships in Slice 3 (G-34 notes the metric is log-only). The walkback logic handles the resulting data gap; users see the AC-4.5.2/4.5.3 staleness chip. The symmetric `fx.bootstrap_failure` row uses the same `{ reason, kind, attempted_at_minute }` shape.

**Walkback helper.** `orbit_db::fx_rates::lookup_walkback(pool, quote: &str, on: NaiveDate, max_walkback_days: u32) -> Result<(Decimal, NaiveDate, u32), FxError>`. Implementation:

```text
SELECT rate, rate_date
  FROM fx_rates
 WHERE quote = $1
   AND base = 'EUR'
   AND source = 'ecb'
   AND rate_date BETWEEN ($2::date - make_interval(days => $3::int)) AND $2
 ORDER BY rate_date DESC
 LIMIT 1;
```

The caller receives `(rate, actual_rate_date, walkback_days)` where `walkback_days = on - actual_rate_date`. Staleness tier (per AC-4.5.1/4.5.2/4.5.3):

- `walkback_days == 0` → `fresh`
- `1 <= walkback_days <= 2` → `walkback` (chip shows "stale N día(s)")
- `3 <= walkback_days <= 7` → `stale` (dashboard banner fires per AC-4.5.3)
- no row within 7 days → `unavailable` (AC-4.5.4)

`FxError` is a narrow enum: `{ DbError(sqlx::Error), RateUnavailable }`. The handler catches `RateUnavailable` and renders the AC-5.5.4 unavailable state; `DbError` bubbles to the 500 path.

**Fetch-on-demand (AC-4.4.1).** Separate from the scheduled worker. When the paper-gains handler finds `lookup_walkback(...).walkback_days > 0` AND `now > 17:00 Madrid` AND `rate_date < today`, it triggers exactly one `GET /stats/eurofxref/eurofxref-daily.xml` synchronously (5-second timeout per ADR-007), inserts the result, and retries the walkback lookup. The per-request budget (AC-4.4.3) is one on-demand fetch per handler invocation; subsequent FX lookups in the same render cycle read the now-fresh row. The on-demand fetch writes an `fx.fetch_success` audit row with `rows_inserted: 1` (or 0 on a race with the scheduler). This path lives in `orbit_api` (handler crate), not in `orbit_worker` — the worker doesn't know about request contexts.

**Health reporting.** `GET /healthz` is unchanged. The worker reports its own status via audit_log rows; a Slice-9 monitoring surface reads those rows. Slice 3 does not ship a `/worker-status` endpoint.

**SEC-050 log allowlist.** Every log line in the worker goes through `orbit_log::event!`; no `tracing::info!` direct calls. New allowlisted event names: `fx.fetch_start`, `fx.fetch_success`, `fx.fetch_failure`, `fx.fetch_persistent_failure`, `fx.bootstrap_start`, `fx.bootstrap_success`, `fx.scheduler_tick`. CI lint rejects any new `event!` name not in the allowlist.

### 5. Paper-gains algorithm (pure function + parity mirror)

Backend authoritative, frontend parity mirror — same discipline as the Slice-2 stacked-cumulative algorithm (ADR-016 §4).

**Backend location.** `orbit_core::paper_gains::compute(...)` — a pure function in the shared core crate, testable without a DB.

```rust
pub struct PaperGainsInput<'a> {
    pub grants: &'a [GrantWithEvents],    // each carries Vec<VestingEvent>
                                          // + Option<Vec<EsppPurchase>>
    pub ticker_prices: &'a [TickerCurrentPrice],
    pub grant_overrides: &'a [GrantCurrentPriceOverride],
    pub fx_rate: FxLookupResult,          // today's EUR/USD rate (or stale)
    pub today: NaiveDate,
}

pub struct PaperGainsResult {
    pub per_grant: Vec<PerGrantGains>,
    pub combined_eur_band: Option<EurBand>,   // None on all-excluded + unavail
    pub incomplete_grants: Vec<Uuid>,
    pub staleness_fx: Staleness,
    pub fx_date: Option<NaiveDate>,
}

pub struct PerGrantGains {
    pub grant_id: Uuid,
    pub complete: bool,
    pub native_currency: Currency,
    pub gain_native: Option<Decimal>,
    pub gain_eur_band: Option<EurBand>,
    pub missing_reason: Option<MissingReason>,
}

pub enum MissingReason {
    FmvMissing,                    // AC-5.5.1 — ≥1 past vest has fmv_at_vest=NULL
    NoCurrentPrice,                // AC-5.1.3 — no ticker price + no override
    NsoDeferred,                   // AC-5.4.3 — Slice-5 concern
    DoubleTriggerPreLiquidity,     // AC-5.4.4 — zero-realized shares
}

pub struct EurBand {
    pub low: Decimal,   // spread = 3% (worst-case retail)
    pub mid: Decimal,   // spread = 1.5% (central)
    pub high: Decimal,  // spread = 0% (best-case wholesale)
}
```

**Algorithm (AC-5.2.3 + AC-5.4).**

```text
for each grant in input.grants:
    price = pick_current_price(grant, input)
    if grant.instrument in {nso, iso_mapped_to_nso}:
        per_grant[grant.id] = { complete: false, missing: NsoDeferred }
        continue
    if price is None:
        per_grant[grant.id] = { complete: false, missing: NoCurrentPrice }
        continue

    gain_native = 0
    complete = true
    if grant.instrument == 'rsu':
        for each past vesting_event in grant.events where state in {Vested, TimeVestedAwaitingLiquidity}:
            if event.fmv_at_vest is None:
                complete = false
                continue
            gain_native += (price - event.fmv_at_vest) * event.shares_vested_this_event
        # AC-5.4.4: TimeVestedAwaitingLiquidity shares contribute zero to realized gains
        # so we skip them above by filtering on 'Vested' only for the contribution side;
        # but we include them in the complete-flag check because the FMV still matters
        # for Slice-4 tax math. IMPLEMENTATION NOTE: the filter is `Vested` only for
        # the gain sum; `{Vested, TimeVestedAwaitingLiquidity}` for the complete flag.

    elif grant.instrument == 'espp':
        for each purchase in grant.espp_purchases:
            gain_native += (price - purchase.fmv_at_purchase) * purchase.shares_purchased

    if not complete:
        incomplete_grants.push(grant.id)

    gain_eur_band = apply_fx_bands(gain_native, input.fx_rate)
    per_grant[grant.id] = { complete, gain_native, gain_eur_band, ... }

combined_eur_band = sum(p.gain_eur_band for p in per_grant where p.complete)
```

**`pick_current_price(grant, input)`.** Resolution precedence per AC-5.3.1/AC-5.3.5: (1) per-grant override in `input.grant_overrides` for `grant.id` → use it; (2) else `grant.ticker` is non-null → look up in `input.ticker_prices` by normalized ticker → use it; (3) else None.

**`apply_fx_bands(gain_native, fx)`.** Per AC-4.6.2 + §5.2 the three bands are applied at render time. `gain_eur_mid = gain_native * ecb_mid * (1 - 0.015)`; `gain_eur_low = ... * (1 - 0.03)`; `gain_eur_high = ... * (1 - 0.00)`. When `fx.staleness == Unavailable`, the function returns `None` for `combined_eur_band` and sets `staleness_fx = Unavailable` (AC-5.5.4).

**AC-5.4.3 NSO deferral.** NSO/ISO grants return `complete: false, missing_reason: NsoDeferred`. They are **excluded** from `incomplete_grants` per AC-5.4.3 (the tester-facing legend line — "coming soon" — is a frontend concern, not a banner-driven concern); the Vec is reserved for RSU + ESPP grants with actionable gaps.

**AC-5.4.4 double-trigger exclusion.** A double-trigger RSU with `liquidity_event_date IS NULL` produces `complete: false, missing_reason: DoubleTriggerPreLiquidity, gain_native: Some(0), gain_eur_band: Some(0-band)`. The zero contribution is correct (zero realized shares); the `complete: false` excludes the grant from the aggregate and from the banner (AC-5.4.4).

**Parity mirror.** `frontend/src/lib/paperGains.ts` implements the same algorithm in TypeScript + `decimal.js`. Shared fixture: `backend/crates/orbit-core/tests/fixtures/paper_gains_cases.json` — ~12 cases covering every branch (all-complete RSU, one-missing-fmv RSU, ESPP only, mixed RSU+ESPP, NSO excluded, double-trigger pre-liquidity, unlisted-company with override, ticker-with-override, FX-unavailable, FX-stale, no current price). Both sides consume the fixture; CI hard-fails on drift.

**Performance.** Per §8, the handler-side wrapper (`handlers::dashboard::paper_gains`) is budgeted at 500 ms P95 with 20 grants × ~240 events each. The pure function is the dominant cost; per-grant iteration is O(E) where E is the vest-event count. At 4 800 events total the arithmetic is trivial (`rust_decimal` mul/add + i128 overflow-safe).

### 6. Sequence diagrams

Mermaid; matches the ADR-014 §4 and ADR-016 §5 shape. Two are worth writing in full.

#### 6.1 Daily ECB fetch (scheduled, worker path)

```mermaid
sequenceDiagram
    autonumber
    participant S as Scheduler (orbit-worker loop)
    participant W as Fetch task
    participant ECB as www.ecb.europa.eu
    participant PG as Postgres

    S->>S: sleep_until(next_17_00_madrid(now))
    S->>W: tick() — spawn fetch task
    W->>W: orbit_log::event!("fx.fetch_start", { attempt: 1 })
    W->>ECB: GET /stats/eurofxref/eurofxref-daily.xml (5s timeout)
    alt 200 OK
        ECB-->>W: <gesmes:Envelope> with <Cube time="..."> <Cube currency="USD" rate="1.0823"/> ...
        W->>W: quick-xml parse → (rate_date, vec![(USD, 1.0823), ...])
        W->>PG: INSERT INTO fx_rates (base, quote, rate_date, rate, source) \
                 VALUES ('EUR', 'USD', $1, $2, 'ecb') ON CONFLICT DO NOTHING
        PG-->>W: rows_inserted: 1 (or 0 on idempotent re-run)
        W->>PG: INSERT audit_log (action='fx.fetch_success', payload={publication_date, quote_currencies:['USD'], rows_inserted:N})
        W->>W: orbit_log::event!("fx.fetch_success", { rows_inserted: N })
    else timeout OR 4xx/5xx OR parse error
        W->>W: backoff: sleep(1s, 5s, 25s) across attempts 1..3
        W->>ECB: GET (attempt 2)
        W->>ECB: GET (attempt 3)
        W->>W: all three failed → reason ∈ {http, parse, timeout, dns}
        W->>PG: INSERT audit_log (action='fx.fetch_failure', \
                  payload={reason, attempt_number:3, attempted_at_minute:"HH:MM"})
        W->>W: orbit_log::event!("fx.fetch_failure", { reason })
        Note over W,PG: Walkback logic handles the gap; no row inserted.
    end
    W-->>S: done
    S->>S: sleep_until(next_17_00_madrid(now))
```

#### 6.2 Override a past vest + bulk-fill rest (handler path)

```mermaid
sequenceDiagram
    autonumber
    participant U as User
    participant SPA as React SPA
    participant API as axum API
    participant PG as Postgres

    Note over U,PG: Part 1 — override a single past row.
    U->>SPA: edits FMV in row $eventId, hits Enter
    SPA->>API: PUT /api/v1/grants/:grantId/vesting-events/:eventId \
               body {fmvAtVest:"42.80", fmvCurrency:"USD", updatedAt:"..."}
    API->>API: validator: fmv > 0, currency in set, schedule rules
    API->>PG: Tx::for_user (app.user_id = user)
    API->>PG: BEGIN
    API->>PG: SELECT updated_at FROM vesting_events WHERE id=$1 AND grant_id=$2 FOR UPDATE
    alt updated_at != request.updatedAt
        API-->>SPA: 409 { code: "resource.stale_client_state" }
        SPA->>U: inline advisory: "refresh to see current values"
    else updated_at matches
        API->>PG: UPDATE vesting_events \
                    SET fmv_at_vest=$1, fmv_currency=$2, is_user_override=true, \
                        overridden_at=now() \
                    WHERE id=$3
        Note over PG: touch_updated_at trigger fires → updated_at=now()
        Note over PG: CHECK override_flag_coherent passes (is_user_override <=> overridden_at NOT NULL)
        API->>PG: INSERT audit_log (action='vesting_event.override', \
                   target_kind='vesting_event', target_id=$eventId, \
                   payload={grant_id:$grantId, fields_changed:["fmv"]})
        API->>PG: COMMIT
        API-->>SPA: 200 { event with updatedAt_new }
        SPA->>SPA: update TanStack-Query cache; re-render row with "Ajustado manualmente" chip
    end

    Note over U,PG: Part 2 — bulk-fill the rest with "Aplicar FMV a todos".
    U->>SPA: opens bulk-fill modal, enters FMV=40.00 USD
    SPA->>SPA: client-side computes X (null-FMV rows), Y (existing-FMV rows); shows confirmation
    U->>SPA: confirms
    SPA->>API: POST /api/v1/grants/:grantId/vesting-events/bulk-fmv \
               body {fmv:"40.00", currency:"USD"}
    API->>PG: Tx::for_user; BEGIN
    API->>PG: UPDATE vesting_events \
                SET fmv_at_vest=$1, fmv_currency=$2, is_user_override=true, overridden_at=now() \
                WHERE grant_id=$3 AND fmv_at_vest IS NULL \
                RETURNING id
    PG-->>API: affected ids = [id_a, id_b, ..., id_x] (X rows)
    API->>PG: INSERT audit_log (action='vesting_event.override', \
               target_id=id_a, payload={grant_id, fields_changed:["fmv"]}) \
               — one per affected id
    API->>PG: (bulk INSERT via unnest()-style batch; X rows total)
    API->>PG: COMMIT
    API-->>SPA: 200 { appliedCount: X, skippedCount: Y }
    SPA->>SPA: re-fetch /grants/:grantId/vesting-events; dashboard paper-gains banner drops grant on next render (AC-8.6.6)
```

### 7. What Slice 3 explicitly defers (make TBD impossible)

The following are **designed but not implemented** in Slice 3. Each is listed here so the implementation engineer never sees a TBD.

| Deferred item | Slice | Note |
|---|---|---|
| Tax math (IRPF projection, ahorro-base, rendimiento-del-trabajo) | 4 | Per spec §12 + Slice-3 AC §14. Slice 3 surfaces zero tax numbers; paper-gains is a display-only EUR aggregator. |
| Rule-set versioning stamping on calculations | 4 | Per AC-7.1.6 — the footer chip ships now but carries no tax rule-set version in Slice 3. Slice 4 adds `calculations.rule_set_id` and the chip grows an `es-2026.1.0` segment. |
| `fx_rates.source = 'user_override'` write path | 4 | The DDL accepts it today (CHECK-in-list); no Slice-3 handler writes it. The calculation-scoped FX-override surface lands in Slice 4 per ADR-007 §User overrides. |
| Modelo 720 worksheet PDF (US-007 AC 3) | 6 | Slice 3 ships the threshold banner only (AC 1, 2, 4). Slice 6 consumes the full `modelo_720_user_inputs` time-series + Slice-3 FX to produce the worksheet. |
| Sell-now calculator (US-013) | 5 | The paper-gains tile is a **display-only** EUR aggregator; it is not the sell-now calculator. Sell-now, Finnhub market quotes, net-EUR-landing bands, and the Modelo 720/721 passive banner on a sell screen all ship in Slice 5. |
| Market-data vendor integration (Finnhub / Twelve Data) | 5 (dev tier) / 9 (commercial) | Current prices are user-entered in Slice 3. The per-ticker-current-price table is the input surface; Slice 5 replaces "user-entered" with "vendor-fetched" for listed tickers. |
| `nso_exercises` table + NSO exercise-FMV capture | 5 | Per AC-5.4.3 — NSO/ISO grants are excluded from Slice-3 paper-gains. Slice 5 adds the table, the exercise flow, and the NSO contribution to paper-gains. |
| Art. 7.p eligibility evaluation (pro-rata, €60 100 cap, exento / no-exento chip) | 4 | Per Slice-2 AC §12 (inherited) + Slice-3 AC §14. The Slice-2 trips screen still says "Capturado — revisión pendiente (N/5)" through Slice 3. |
| GeoIP country population on `sessions.country_iso2` | 9 | Per v1.4 plan Q2 — supersedes ADR-016 §9.2. The column stays NULL through Slice 8; the Sessions UI's `ubicación desconocida` branch is the default state. |
| Override badge on grant-detail timeline (Gantt + cumulative-curve toggle) | 4 polish | Per AC-8.1 + Slice-3 AC §14 — overridden rows surface in the "Precios de vesting" table via the chip, but the timeline Gantt / cumulative-curve toggle (Slice-1 AC-6.1.2) does not yet carry an override badge. |
| Full DSR self-service (export + erasure + rectify) | 7 | Slice 3 extends data-minimization posture only (G-26 extended — FMV values, current prices, EUR amounts, M720 totals never in analytics). |
| "Recompute under current rules" action | 4 (dormant) / 5 (active) | No calculations in Slice 3 → no "recompute" surface. |
| Bulk clear of FMVs | Never (product lock) | Per AC-8.6.5 — asymmetric by design; per-row clear is the only path. |
| User-overridable FX mid or spread as an editable field | 4 on tax surfaces / 5 on sell-now | Slice 3 renders 0 % / 1.5 % / 3 % spread bands but exposes no user-editable FX field. |
| nftables egress allowlist entry for ECB | 9 | Per G-28 — local-dev egress unrestricted in Slice 3. Slice 9's deploy gate adds the allowlist entry with `www.ecb.europa.eu` as the precise target. |

### 8. Performance and rate-limit targets applied to Slice 3

- `GET /api/v1/dashboard/paper-gains` ≤ **500 ms P95** with 20 grants × 240 vest events each (AC-5.6.1). The dominant cost is the per-grant algorithm + one FX lookup; all arithmetic is decimal (no allocation per event beyond the `Vec<PerGrantGains>`).
- `PUT /api/v1/grants/:grantId/vesting-events/:eventId` ≤ **200 ms P95**. One `SELECT ... FOR UPDATE`, one `UPDATE`, one audit INSERT, one COMMIT. The optimistic-concurrency check adds no round-trip on the happy path.
- `POST /api/v1/grants/:grantId/vesting-events/bulk-fmv` ≤ **500 ms P95** even at 240 rows (one monthly 20-year grant). A single `UPDATE ... RETURNING id` plus a bulk audit INSERT (one `INSERT INTO audit_log SELECT * FROM unnest(...)` row-multiplex) keeps the round-trip count at two.
- `GET /api/v1/dashboard/modelo-720-threshold` ≤ **300 ms P95**. One read of the three M720 categories + derivation of the securities line (reuses the paper-gains per-grant loop minus the FX-band expansion).
- `GET /api/v1/fx/rate`, `GET /api/v1/fx/latest`, `GET /api/v1/rule-set-chip`, `GET /api/v1/current-prices`, `GET /api/v1/grants/:id/current-price-override` ≤ **100 ms P95** (trivial reads).
- Daily ECB fetch (worker path) ≤ **5 s** end-to-end (the hard timeout; typical completion is well under 1 s against ECB's CDN).
- 90-day bootstrap (one-time per cold DB) ≤ **15 s** end-to-end (the 90-day file is ~40 KB of XML; the parse + bulk-insert is the cost).

**Per-user rate limits** (SEC-160; SQL-backed leaky bucket — same mechanism as Slices 1–2):

- Write endpoints on vesting events (`PUT /vesting-events/:id`, `POST /vesting-events/bulk-fmv`): **120 / user / hour**. The bulk endpoint counts as a single request against the bucket (the audit rows are internal).
- `POST /vesting-events/bulk-fmv`: tightened to **10 / user / minute** on top of the hourly budget. Bulk-fill is an attention-requiring action; a user-agent bug that retries the POST ten times in a second should be blocked with a 429.
- Write endpoints on current prices (`PUT|DELETE /current-prices/:ticker`, `PUT|DELETE /grants/:id/current-price-override`): **120 / user / hour**.
- FX reads (`GET /fx/rate`, `GET /fx/latest`): **600 / user / hour** — generous because the dashboard hydrates multiple times during normal use.
- `GET /rule-set-chip`: **600 / user / hour** — same rationale.

### 9. Test plan summary

Summary; detailed test work is `qa-engineer`'s (T31). The ADR pins what MUST be tested.

**Property / fixture tests (Rust + TS parity).**

- `orbit_core::vesting::derive_vesting_events` override preservation across grant-param changes: `vesting_total_months` shortened, `cliff_months` changed, `cadence` changed. The overrides survive every change; futures re-derive.
- Cumulative-invariant relaxation is correct: when `existing_overrides.is_empty()`, the Slice-1 `SUM == share_count` invariant holds; when any override exists, the sum MAY differ; the property test asserts both branches of this biconditional.
- Paper-gains pure function: Rust `proptest` + frontend `fast-check` against `paper_gains_cases.json`; bit-identical output between the two implementations for every case.
- Determinism: same `(grant, today, existing_overrides)` → bit-identical output across two calls.

**Integration tests (backend, against a real Postgres).**

- **ECB walkback.** Seed `fx_rates` with a 7-day gap ending `today - 8`; call `lookup_walkback(..., today, 7)` → returns `(rate, today-8, 8)` → caller maps to `Unavailable`. Insert a row at `today - 6`; call → `(rate, today-6, 6)` → `Stale`. Insert a row at `today`; call → `(rate, today, 0)` → `Fresh`. Cross every staleness tier.
- **Bulk-fmv skip.** Seed a grant with 48 vest events: 3 with existing `fmv_at_vest = 42.00` (via earlier overrides), 45 with NULL. POST `/bulk-fmv` with `{fmv: 40.00, currency: "USD"}`. Assert `appliedCount: 45, skippedCount: 3`; the 3 earlier rows retain `42.00`; 45 rows now carry `40.00, is_user_override: true`; audit log shows 45 rows of `vesting_event.override` (not 48).
- **Bulk-fmv SKIPS even when `is_user_override = false` on the existing-FMV row.** Seed a row with `fmv_at_vest = 42.00, is_user_override = false` (hypothetical migration-era state); POST `/bulk-fmv`. Assert the row is in `skippedCount` and is not overwritten (Q4 — the gate is `fmv_at_vest IS NULL`, not the override flag).
- **clearOverride reverts date + shares; preserves FMV.** Seed a row with `vest_date = 2025-10-15, shares_vested = 1000, fmv_at_vest = 42.00, fmv_currency = USD, is_user_override = true, overridden_at = 2026-04-10T12:00Z`. PUT with `{clearOverride: true}`. Assert: `vest_date` and `shares_vested_this_event` revert to the derivation's output for that slot; `fmv_at_vest` = 42.00, `fmv_currency = USD` preserved; `is_user_override = true` (FMV is still a user edit), `overridden_at` unchanged. Compare with a row that had no FMV: clearOverride drops `is_user_override` to `false`, `overridden_at` to NULL.
- **Grant-edit that shrinks `share_count` below sum-of-override-shares returns 422.** Seed 5 overrides summing to 5 000 shares on a grant with `share_count = 10 000`. PUT `/grants/:id` with `share_count = 4 000`. Assert 422 with `code = "grant.share_count_below_overrides"`; the grant row is unchanged; no audit row written.
- **Optimistic concurrency 409.** Open a tab A; edit a vesting event → server responds with `updatedAt_new`. Tab B holds the pre-edit `updatedAt_old`. Tab B PUTs → 409 with `code = "resource.stale_client_state"`.
- **ECB fetch idempotency.** Run `orbit worker --once fx` twice in the same day against a test ECB-fixture server; assert the second run writes zero `fx_rates` rows and emits an `fx.fetch_success` audit row with `rows_inserted: 0`.
- **Cold-DB bootstrap.** Truncate `fx_rates`; start the worker; assert the 90-day fixture is fetched and bulk-inserted; one `fx.bootstrap_success` audit row with `rows_inserted: ~60, span_days: ~90`.
- **Warm-restart bootstrap no-op.** Seed `fx_rates` with ≥30 rows in the last 90 days; start the worker; assert no `fx.bootstrap_success` audit row is written.
- **Cross-tenant RLS probe.** User A creates a ticker-price / per-grant-override; user B `GET`/`PUT`/`DELETE` on A's id → 404 (not 403). Every user-scoped endpoint (ticker-current-prices × 3 methods, grant-override × 3 methods, vesting-event PUT, bulk-fmv POST) is covered. `fx_rates` is NOT probed (shared reference data).
- **Audit payload allowlist.** For every new audit action (`fx.fetch_success`, `fx.fetch_failure`, `fx.bootstrap_success`, `vesting_event.override`, `vesting_event.clear_override`), assert the serialized `payload_summary` matches the allowlisted shape exactly (no FMVs, no share counts, no vest dates, no employer names, no ticker symbols, no raw XML bodies). Enforced in CI via `backend/crates/orbit-core/tests/fixtures/audit_payload_shapes.json` — the Slice-2 fixture extended with Slice-3 keys.

**Frontend unit + E2E tests.**

- Vitest on `paperGains.ts` against the shared fixture.
- Vitest on `vesting.ts` override-preservation against the shared `vesting_override_cases.json`.
- Playwright on the Slice-3 demo script (`docs/requirements/slice-3-acceptance-criteria.md` §13, 22 steps).
- `axe-core` on each new surface in G-21 extended: dashboard with paper-gains tile (full data + partial-data branch), Profile with M720 threshold banner, grant-detail "Precios de vesting" section (past-row edit open, future-row edit open, override marker visible), bulk-fill modal, grant-edit form with override-exists banner, rule-set chip explainer stub.
- Keyboard-only walkthrough (G-33) across steps 5–14 of the demo script.

### 10. Assumptions and escalations

Two items warrant explicit recording.

#### 10.1 ECB XML schema stability

The `eurofxref-daily.xml` format has been stable for 20+ years (the XML namespace `gesmes` and the `<Cube>` element shape predate the Eurosystem's public data portal). However, the format is **not explicitly versioned** — there is no schema URL, no deprecation policy, no SLA. The ingestion parser is therefore defensive: any deviation from the expected shape (missing `<Cube time=…>`, missing `<Cube currency= rate=>` children, unparseable decimals) produces an `fx.fetch_failure` audit row with `reason = "parse"` and does **not** insert partial data. The walkback logic then handles the resulting data gap from a user's perspective (chip goes stale, banner fires).

**Cost of the opposite decision** (trust the schema and partial-parse): a silent data corruption on an ECB format drift. Not acceptable for a surface that feeds tax math in Slice 4.

**Follow-up if schema drift ever occurs.** The parser is centralized in one `orbit_db::fx_rates::parse_ecb_xml` function; a format change requires one function rewrite + a regression test case. Slice 9's nftables allowlist doesn't shield against schema drift (it's a format concern, not an egress concern).

#### 10.2 Ticker ambiguity across employers

A ticker symbol is a per-market identifier, not a globally unique one. Two employers with the same ticker (rare but possible in pre-IPO + post-spin-off scenarios, or across venues) would share one `ticker_current_prices` row per AC-5.2.2 (the unique key is `(user_id, ticker_normalized)`). This is a Slice-8 concern once bulk import normalizes employer-ticker pairs.

**Chosen: one row per (user, ticker) in Slice 3.** The ambiguity is acknowledged but not solved. Rationale:

1. The primary persona (María) has at most two tickers in the Slice-3 demo script (ACME + ESPP employer), and a name collision between them is the user's own error — the grant-entry wizard rejects duplicate employer names (Slice-1 `employer_name` has a CHECK but not a uniqueness — by design per Slice-1 AC).
2. The per-grant override exists precisely to paper over ambiguity cases (AC-5.3.1). If a user's two grants share a ticker but represent distinct companies, the user sets per-grant overrides and the ticker-price row becomes moot.
3. Solving ticker-ambiguity properly requires an issuer identifier (CIK, ISIN, LEI) that Slice 3 does not collect and that the user likely does not know. Slice 8's bulk-import flow is the natural place to infer this.

**Cost of the opposite decision** (key by `(user_id, employer_id, ticker)`): a row schema change in Slice 8 + data migration for any Slice-3-era user who had a colliding ticker. Straightforward but out-of-scope for Slice 3.

### 11. Alternatives considered

- **`fmv_at_vest` on `grants` instead of `vesting_events`.** Rejected per Ivan's Option A decision. Post-IPO RSU price moves per vest; a single FMV per grant would be wrong on day one for any publicly-traded employer. The per-vest column is the only shape that supports both the pre-IPO 409A case (one FMV for many months) and the post-IPO price-movement case (a different FMV every month). Cost of opposite decision: a data migration in Slice 4/5 when the calculator lands.
- **Sidecar `vesting_event_overrides` table.** Rejected. The flag-column approach is simpler; the table stays within limits (max ~240 events per grant × realistic grant count → bounded well under 1M rows even at 10 000 users). A sidecar table would force an outer join on every read and a two-table transaction on every override write for zero correctness benefit.
- **Recompute cumulative when overrides exist (rebalance futures to meet `share_count`).** Rejected per Ivan's D1 decision. User edits are authoritative; a rebalance would silently modify futures the user did not edit, which is the opposite of the preservation invariant. The `AC-8.5.3` UI banner is the right shape: tell the user the curve is manually adjusted and let them decide. Cost of opposite decision: a surprising-edit foot-gun that would surface as user complaints in Slice 4 review.
- **A separate `current_prices` global reference-data table (e.g., indexed by ticker, shared across users).** Rejected. Current price in Slice 3 is **user-entered** — it is workspace data, not reference data. A global table would conflate two users' guesses at the same company's price. Slice 5 will introduce a `market_quotes_cache` (already outlined in ADR-005) for vendor-fetched prices — that's the right home for the shared shape. In Slice 3, the per-user table is correct.
- **One `current_prices` user-scoped table with nullable `grant_id`.** Rejected. The two surfaces (per-ticker, per-grant) have distinct UNIQUE key semantics that would force either a partial unique index or a trigger to enforce. Two tables keep the schema boring.
- **Store the `fmv_currency` as an `enum` type (`CREATE TYPE currency AS ENUM (...)`).** Rejected for consistency with Slice-2's `espp_purchases.currency` which uses `TEXT CHECK IN (...)`. Slice 4 may add more currencies (JPY, CHF) as Orbit goes multi-jurisdiction; growing a CHECK list is a one-line migration; growing an ENUM requires more ceremony. Boring, consistent.
- **`tokio-cron-scheduler` for the worker scheduler.** Rejected for Slice 3. `tokio::time::sleep_until` with a computed `next_17_00_madrid` is simpler and has no extra dep. If cron-like multi-schedule support ever lands (Slice 5 might want hourly market-quote polling), revisit then.
- **External cron + HTTP trigger for the fetch.** Rejected per ADR-007 — same reasoning, reaffirmed.
- **GeoIP population on `sessions.country_iso2` in Slice 3.** Rejected per v1.4 plan Q2 — deferred to Slice 9 alongside Finnhub + other vendor procurement. Supersedes ADR-016 §9.2.
- **Store the whole ECB XML body in a `raw_response` column on `fx_rates`.** Rejected. The parsed rate is the atomic fact; the XML body is regenerable from ECB's historical file at any time. Storing it bloats the table for no gain and risks a G-29 violation (raw XML in a row that might end up in a log).
- **Key `fx_rates` on `(quote, rate_date)` only (implying `source` is always 'ecb').** Rejected. Slice 4's user-override path writes rows with `source = 'user_override'`. Keying on `source` today is the boring, forward-compatible shape.
- **Model the `fmv_at_vest` on a nullable FK to an `fmv_events` table.** Rejected. A sidecar FMV table buys nothing over the two columns; it introduces a JOIN on every vesting-events read; and the FMV value is intrinsically per-vest (not a reusable reference across grants).
- **Populate `updated_at` on `vesting_events` via the handler, not the trigger.** Rejected. Trigger-based maintenance is the shape the rest of the schema uses (`grants`, `espp_purchases`, `art_7p_trips`). Handler-based maintenance would require every writer to remember; miss a writer and you silently break AC-10.5.

## Consequences

**Positive:**

- Every Slice-3 AC traces to a concrete schema column, trigger, handler path, audit payload shape, pure-function rule, or deferral note. No TBD.
- RLS enforced from the first commit on the two new user-scoped tables via the inherited `tenant_isolation` policy; the SEC-020 CI introspection test extends with two table names. `fx_rates` is explicitly flagged as reference-data (no RLS, no cross-tenant probe) — the posture is documented and testable.
- The override-preservation rule on `derive_vesting_events` is pinned in pseudocode, fixture-tested, and property-tested, removing the "two reimplementations drift apart" risk.
- The paper-gains algorithm is a pure function with a shared parity fixture — same discipline as the Slice-1 vesting algorithm and the Slice-2 stacked-cumulative algorithm.
- ECB pipeline is ADR-007-compliant and lives in a dedicated worker with a minimal in-process scheduler. The walkback helper centralizes staleness logic; the UI reads tier, not raw walkback_days.
- `fx_rates`'s append-only posture (GRANT SELECT + INSERT, no UPDATE, no DELETE on orbit_app) mirrors `audit_log` and preserves Slice-4 calculation reproducibility for free.
- The optimistic-concurrency story on vesting-event edits is concrete (via `updated_at` + the trigger) and does not require a sidecar `versions` table.

**Negative / risks:**

- The override-preservation rule is subtle: a future grant-param change can leave the cumulative sum diverged from `share_count`, and the UI must correctly render the AC-8.5.3 banner every time. Mitigation: the property test exercises the biconditional invariant; the integration test on `grant_edit_share_count_below_overrides` locks in the related 422 path.
- The ECB worker introduces the first external network call Orbit ever makes. Mitigation: 5-second timeout, exponential backoff, defensive parser, audit-logged failures, walkback-tolerant handler path. If ECB goes down, users see a stale-chip banner, not a 500.
- `tokio::time::sleep_until`-based scheduler is subject to clock drift if the host's wall clock jumps (NTP adjust, container restart). Mitigation: on every wake, recompute `next_17_00_madrid(now)` — the drift is self-correcting over one cycle. A Slice-9 enhancement could swap to `tokio-cron-scheduler`.
- Bulk-fill writes one audit row per modified vest event — up to 240 rows in a single transaction. Mitigation: the `INSERT INTO audit_log SELECT * FROM unnest(...)` multiplex pattern keeps the round-trip count constant; CI benchmark asserts the 240-row bulk-fill stays under the 500 ms P95 budget.
- Client and server each implement the paper-gains algorithm; drift risk is real. Mitigation is the shared fixture + CI hard-fail on drift — the same discipline that has held for Slice 1/2 vesting algorithms.
- The `fx_rates.source` CHECK list widens in Slice 4 when `user_override` actually gets written. This is a breaking change to any handler that reads `fx_rates` without filtering on `source = 'ecb'`. Mitigation: the Slice-3 `lookup_walkback` helper explicitly filters `source = 'ecb'`; the implementation engineer extends this contract in Slice 4.
- Two tables for current prices (`ticker_current_prices` + `grant_current_price_overrides`) is one more table than the minimum; a future Slice-5/8 refactor may merge them if bulk import needs a more normalized shape. The UNIQUE-key semantics would dominate any such decision.

**Tension with prior ADRs:**

- **ADR-016 §9.2 superseded on the Slice-3 population note.** Per v1.4 plan Q2, GeoIP lands in Slice 9. ADR-017 does not touch `sessions.country_iso2`; the Slice-2 DDL is complete. No contradiction in the column's shape or privacy posture — only its population schedule changes.
- **ADR-007 reaffirmed verbatim.** Slice 3 translates ADR-007 into concrete DDL + worker code; it re-decides nothing.
- **ADR-005's `fx_rates` outline is expanded.** ADR-005 listed 7 columns; ADR-017 pins the same column set with concrete types and constraints (NOT RLS-scoped, append-only from orbit_app, unique key on `(base, quote, rate_date, source)`). No contradiction.

**Follow-ups (not blocking Slice 3):**

- **Slice 4.** Promote the "Ajustado manualmente" row visual signal into the grant-detail timeline (Gantt + cumulative-curve toggle) per AC-8.1 tester-do-not-flag.
- **Slice 4.** Stamp every calculation with `fx_rate_id` + `rule_set_id` + `content_hash` per ADR-005. The `fx_rates` table's append-only posture is the enabling invariant.
- **Slice 4.** Activate the user-overridable FX mid + spread surface on tax-calc screens; write `fx_rates` rows with `source = 'user_override'`; the DDL is ready today.
- **Slice 5.** `nso_exercises` table with `fmv_at_exercise` — symmetric to `espp_purchases.fmv_at_purchase`. Paper-gains algorithm's `NsoDeferred` branch flips on.
- **Slice 5.** Finnhub / Twelve Data integration for listed tickers' current prices; `ticker_current_prices` becomes a user-overridable cache layer rather than the sole input surface.
- **Slice 6.** Modelo 720 worksheet PDF consumes the Slice-3 securities derivation + Slice-2 time-series. No new DDL; the read path is the same.
- **Slice 9.** GeoIP population on `sessions.country_iso2` (alongside Finnhub + other vendor procurement per v1.4 plan Q2). nftables allowlist entry for `www.ecb.europa.eu` as the single egress target. Deploy gate.
- **Implementation engineer (Slice 3).** Author `vesting_override_cases.json` and `paper_gains_cases.json` fixture files; wire both backend and frontend. Co-locate with Slice-1/-2 fixtures.
- **Implementation engineer (Slice 3).** Extend the audit-payload-shape CI fixture with the five new Slice-3 actions (`fx.fetch_success`, `fx.fetch_failure`, `fx.bootstrap_success`, `vesting_event.override`, `vesting_event.clear_override`).
- **Implementation engineer (Slice 3).** Extend the `Tx::for_user` cross-tenant probe suite (SEC-023) to cover every new `[A]` user-scoped endpoint listed in §3. Explicitly skip `fx_rates` endpoints (reference data).
- **Security-engineer (Slice 3).** Confirm that the ECB worker's log lines are scrubbed of raw-XML bodies (G-29 extended) and that the `fx.fetch_failure` payload's `attempted_at_minute` key is sufficient granularity for forensics without becoming a timing side-channel.
- **QA-engineer (T31).** Land the override-preservation property test suite, the ECB-walkback integration suite, the bulk-fmv skip-semantics suite, the optimistic-concurrency suite, and the demo-script Playwright flow (22 steps).
