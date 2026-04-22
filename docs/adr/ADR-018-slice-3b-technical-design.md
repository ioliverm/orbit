# ADR-018: Slice-3b technical design

- **Status:** Proposed
- **Date:** 2026-04-19
- **Deciders:** Ivan (owner)
- **Traces to:** `docs/requirements/slice-3b-acceptance-criteria.md` (authoritative for this slice — every Slice-3b AC cited below is resolved by a concrete schema, API, algorithm, or deferral decision here), `docs/requirements/v1-slice-plan.md` v1.5 (Q-A — Slice 3b as a net-new slice; Q-B — `share_sell_price` as a distinct column from `fmv_at_vest`; Q-C — cap-gains basis shifts to `fmv_at_vest × net_shares_delivered` in Slice 4; Q-D — tax-withholding default lives on the user via `user_tax_preferences`, not on the grant; Q-E — the dialog edits FMV + sell price + tax % + shares + vest date in one modal), ADR-005 (entity outline — Slice 3b introduces the first `user_tax_preferences` sidecar), ADR-010 (API envelope + route prefix — unchanged), ADR-014 (Slice-1 DDL — `grants`, `vesting_events`, `residency_periods`, `users`, `sessions`, `audit_log`; `tenant_isolation` policy convention; `touch_updated_at` trigger), ADR-016 (Slice-2 DDL — `modelo_720_user_inputs`, close-and-create transactional pattern; same-day idempotency; partial unique index on `(user_id, category) WHERE to_date IS NULL`), ADR-017 (Slice-3 technical design — override-preservation discipline, `overridden_at` vs `updated_at` split, OCC via `updated_at`, FMV/currency CHECK pair, `touch_updated_at` on `vesting_events`), SEC-020..SEC-026 (RLS), SEC-050 (log allowlist), SEC-101..SEC-103 (audit log hand-rolled `json!({...})` allowlist), SEC-160..SEC-163 (rate limit + validation), spec `docs/specs/orbit-v1-persona-b-spain.md` L319/L334 (RSU cap-gains basis — text amended in Slice 4 under §11 of this ADR to consume `net_shares_delivered` when a sell-to-cover override exists). UX refs `docs/design/screens/profile-tax-preferences.html` (Preferencias fiscales — parallel ux-designer artifact), `docs/design/screens/vesting-event-dialog.html` (dialog replacing the Slice-3 inline editor — parallel ux-designer artifact).

## Context

Slice 3b's boundary per the AC doc's header: **`user_tax_preferences` time-series sidecar · ALTER `vesting_events` with five sell-to-cover columns + CHECK coherence · `orbit_core::sell_to_cover::compute` pure function with TS parity mirror and shared JSON fixture · Profile "Preferencias fiscales" section (country + `rendimiento_del_trabajo_percent` + `sell_to_cover_enabled`) · Vesting-events editor refactor from inline rows to a per-row dialog · default-sourcing of `tax_withholding_percent` from the user's active tax preferences · extended `PUT /api/v1/grants/:gid/vesting-events/:eid` accepting the new fields + `clearSellToCoverOverride` · three new audit actions (SEC-101-strict).** No tax math. No worker changes. No NSO sell-to-cover. No dual-residency concurrent rows. No per-grant tax-percentage default. No GeoIP country auto-detect. No automatic FMV ↔ sell-price reconciliation banner. No retroactive back-fill on pre-Slice-3b vests.

Slice 3b is implementation-ready on the requirements side but leaves concrete DDL for the new `user_tax_preferences` sidecar plus five additive columns on `vesting_events`, the `orbit_core::sell_to_cover::compute` algorithm pseudocode and cross-currency / rounding / negative-net-shares policies, the TS parity mirror path, the extended `PUT` body shape, default-sourcing precedence and the null-vs-omitted distinction on `tax_withholding_percent`, the revert-button two-button model's server contracts (`clearOverride` full vs `clearSellToCoverOverride` narrow) and their dual-audit sequencing, and the sequence-diagram shape of the two load-bearing flows (Profile save + dialog open/save/revert). This ADR produces all of those, traces every AC to a component or decision, resolves the three ambiguities the AC doc deferred (AC-6.3 rounding direction, AC-6.4.2 negative-net behaviour, AC-7.6.3 null-vs-omitted default-sourcing), and enumerates exactly what Slice 3b defers so the implementation engineer never sees a TBD.

Five load-bearing inputs from Slices 1–3 carry forward unchanged:

- **ADR-017 override-preservation is authoritative.** The FMV-track preservation rule in `orbit_core::vesting::derive_vesting_events` (ADR-017 §2) is reused verbatim in Slice 3b; the sell-to-cover track layers on top of the same discipline via a second independent flag (`is_sell_to_cover_override`). ADR-018 does not re-decide derivation semantics; it pins how the second flag composes with the first.
- **OCC via `updated_at` continues.** Slice-3 `vesting_events_touch_updated_at` (ADR-017 §1) handles every write on the table, including Slice-3b writes to the five new columns. No new trigger. No new column. A single `updated_at` gates both tracks (AC-7.4.4).
- **Close-and-create from ADR-016 is the sidecar shape.** `user_tax_preferences` mirrors `modelo_720_user_inputs` structurally: partial UNIQUE on `(user_id) WHERE to_date IS NULL`, transactional close-prior-then-insert-new, same-day-in-place idempotency. The implementation re-uses the `UpsertOutcome { Inserted, ClosedAndCreated, UpdatedSameDay, NoOp }` enum shape verbatim (one new value — `NoOp` — is possible when country/percent/toggle are all equal to the existing open row).
- **Currency whitelist unchanged.** `share_sell_currency` joins `fmv_currency`, `espp_purchases.currency`, and `ticker_current_prices.currency` on the `{USD, EUR, GBP}` allowlist (Slice-2 AC-4.2.6). No new currency enters the system in Slice 3b.
- **Audit-payload allowlist (SEC-101-strict) is inherited verbatim.** Slice 3b's three new actions (`user_tax_preferences.upsert`, `vesting_event.sell_to_cover_override`, `vesting_event.clear_sell_to_cover_override`) hand-roll `json!({...})` with literal keys, no FMVs, no percents, no prices, no amounts, no country codes in the payload bodies — only symbolic outcomes and `fields_changed` arrays.

One architectural-compromise retirement carries forward: the Slice-3 inline row-editor for vesting events is **removed** in Slice 3b in favour of the dialog. This is the AC-7.8.1 tester-do-not-flag item; ADR-018 does not attempt to preserve both the inline editor and the dialog. The underlying `PUT /api/v1/grants/:gid/vesting-events/:eid` endpoint continues to accept FMV-only bodies (Slice-3 semantics preserved) per AC-7.8.3, so a CLI user who hits the API directly with an FMV-only body is still a valid Slice-3 client.

## Decision

### 1. Slice-3b DDL (concrete)

All migrations live under `migrations/`. Numbering is `YYYYMMDDHHMMSS_label.sql` and must sort strictly after `20260523120000_slice_3.sql`. Slice 3b appends one migration: `20260530120000_slice_3b.sql` (ISO timestamp chosen one week after Slice-3).

Slice 3b adds **one net-new user-scoped sidecar table** (`user_tax_preferences`), **five additive columns on `vesting_events`** (`tax_withholding_percent`, `share_sell_price`, `share_sell_currency`, `is_sell_to_cover_override`, `sell_to_cover_overridden_at`), and **two cross-field CHECK constraints** on `vesting_events`. It reuses the Slice-3 `vesting_events_touch_updated_at` trigger verbatim — no new trigger. All other Slice-1/-2/-3 tables are left untouched.

```sql
-- migrations/20260530120000_slice_3b.sql (Slice 3b additions)
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
-- constraints. No new extensions required. No new trigger.

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
```

**RLS policy naming convention (inherited).** The one new user-scoped table carries one policy named `tenant_isolation`. The SEC-020 CI `pg_policies` introspection test extends with the new table name in the expected-set fixture.

**Why `country_iso2` is `TEXT` + length CHECK and not an enum.** The curated v1 list (Spain + the EU-5 + UK for paper-design parity per AC-4.2.1) is handler-gated; growing the list as Orbit expands jurisdictions is a one-line handler change. An enum would require a DB migration for every country addition. Consistent with `grants.instrument`, `modelo_720_user_inputs.category`, and `espp_purchases.currency` (ADR-014, ADR-016 patterns). Boring, reversible.

**Why `rendimiento_del_trabajo_percent` is nullable but stored for all countries.** Per AC-4.2.2 the field is hidden for non-Spain-like countries; the persisted value for those rows is NULL. Per AC-4.2.3 a blank save on a Spain row is also allowed (stored as NULL). Making the column nullable is the only shape that accepts both paths; the in-range CHECK (`>= 0 AND <= 1`) gates only non-null values. Stored as a fraction (`0.4500`) because the sell-to-cover computation consumes it as a fraction; the handler converts from the user-visible percent (`45`) to the stored fraction (`0.4500`) on write and back on read. See §3 for the handler's validator shape.

**Why `sell_to_cover_enabled` has no DEFAULT.** Per the task brief: the server rejects omission. A missing key means the client forgot; a DEFAULT of `false` would silently persist "off" when the user expected "on" (or vice versa). The validator fires a 422 with `code = "user_tax_preferences.sell_to_cover_enabled.required"` when the key is absent from the body. This is the only place Slice 3b deviates from the "absent = null / default" convention of the Slice-2/-3 handlers; the deviation is documented at the validator site.

**Why a partial UNIQUE on `(user_id) WHERE to_date IS NULL`.** Matches the `modelo_720_user_inputs` pattern (ADR-016 §1). The invariant is "at most one open row per user"; a time-series of closed rows accumulates below. The UNIQUE index is the last line of defense if two racing save handlers somehow both pass the application-level check; the second trips a `unique_violation` and its transaction rolls back. The handler layer catches this specific violation and returns a 500 with generic "no se pudo guardar" copy (AC-4.4.4) — it is a server bug if the client path ever sees it.

**Why no OCC on `user_tax_preferences` in Slice 3b.** Per AC-9.4: Profile saves are last-write-wins by design. The history table preserves every closed row regardless of which device wrote it, so a concurrent edit from two devices produces two entries in the history and one open row (the second writer's); neither user loses data. The `updated_at` column is present for a future OCC retrofit but no handler consults it in Slice 3b. Document this explicitly in the AC-9.4 tester-do-not-flag note.

**Why `NUMERIC(5,4)` on `rendimiento_del_trabajo_percent` and `tax_withholding_percent`.** Both columns store a fraction in `[0, 1]` with up to 4 decimal places of precision. `NUMERIC(5,4)` admits values in `(-9.9999, 9.9999)` but the CHECK constraint clamps to `[0, 1]` — the narrower range is what the column promises. 4 decimal places matches AC-4.2.3 (percent renders as `45,0000 %`) and gives enough headroom for future regime-specific percentages (Beckham-law flat 24 % lives well within the precision envelope).

**Why `NUMERIC(20,6)` on `share_sell_price`.** Matches `fmv_at_vest` (ADR-017 §1) precision. The two columns participate in the same arithmetic (sell-to-cover computation divides cash-withheld-in-FMV-currency by sell-price-per-share; same-precision numerator and denominator avoid precision loss in the round-trip). Six decimals is the headroom; the frontend renders 4 decimals and the handler accepts up to 4 on input, matching `fmv_at_vest` exactly.

**Why `is_sell_to_cover_override` parallels `is_user_override` rather than reusing it.** The two flags carry different semantics. `is_user_override = true` means the user edited the vest date, the shares, or the FMV. `is_sell_to_cover_override = true` means the user captured sell-to-cover data. A row may carry `is_user_override = false AND is_sell_to_cover_override = true` (the user took the algorithm's FMV as-is but captured a sell price and tax percent), and the inverse. The two tracks revert independently (narrow `clearSellToCoverOverride` vs full `clearOverride`). A shared flag would force either the full-clear to also clear FMV (wrong — the user edited the FMV; they did not consent to clear it by triggering a narrow revert) or the narrow-clear to leave the flag set after clearing sell-to-cover (also wrong — the row is no longer "manually adjusted" in the sell-to-cover sense). The two-flag shape is the only one that composes both revert paths correctly. The UI collapses the two flags into one visual "Ajustado manualmente" chip (G-23 extended) because the user-visible distinction is meaningless outside the dialog; the distinction matters only for the revert affordances inside the dialog.

**Why no trigger maintains `sell_to_cover_overridden_at`.** Same rationale as Slice-3's `overridden_at` (ADR-017 §1). The `touch_updated_at` function writes only `NEW.updated_at := now()`; it does not touch other columns. Handler code sets `sell_to_cover_overridden_at = now()` in the `UPDATE ... SET` list explicitly on every sell-to-cover override write; the `clearSellToCoverOverride: true` path sets it to `NULL` explicitly. The `sell_to_cover_override_flag_coherent` CHECK catches any handler bug that forgets to pair the two.

### 2. `orbit_core::sell_to_cover::compute` — pure function

The load-bearing algorithm change in Slice 3b. Backend authoritative, TS parity mirror at `frontend/src/lib/sellToCover.ts`, shared JSON fixture at `backend/crates/orbit-core/tests/fixtures/sell_to_cover_cases.json` (Rust parity test at `backend/crates/orbit-core/tests/sell_to_cover_fixtures.rs`).

**Backend location.** `orbit_core::sell_to_cover::compute(...)` — a pure function in the shared core crate, testable without a DB. Module file: `backend/crates/orbit-core/src/sell_to_cover.rs`.

**Signature.**

```rust
/// Input to [`compute`]. All monetary values are scaled i128 in units
/// of 1/10_000 of the native currency (matching the `Shares` / FMV
/// scaling convention in `orbit_core::vesting` — 4 decimal places of
/// precision carried as integers to avoid float drift).
pub struct SellToCoverInput {
    pub fmv_at_vest: Shares,              // scaled — FMV per share
    pub shares_vested: Shares,            // scaled — share count (AC-5.4 parity)
    pub tax_withholding_percent: Shares,  // scaled fraction in [0, 10_000]
                                          //   meaning [0.0000, 1.0000]
    pub share_sell_price: Shares,         // scaled — sell price per share
    pub currency: Currency,               // shared by FMV and sell_price
                                          //   per currency policy below
}

pub struct SellToCoverResult {
    pub gross_amount_scaled: Shares,            // fmv × shares (full precision)
    pub shares_sold_for_taxes_scaled: Shares,   // ceiling at 4 dp (AC-6.3.1)
    pub net_shares_delivered_scaled: Shares,    // shares − sold
    pub cash_withheld_scaled: Shares,           // sold × sell_price
}

pub enum ComputeError {
    /// The `shares_vested − shares_sold_for_taxes` delta is negative.
    /// Fires when `tax_percent = 1` AND `sell_price < fmv`; the
    /// broker did not sell enough shares to cover the nominal tax
    /// obligation. v1 rejects this — see §3 resolution below.
    NegativeNetShares,
    /// Division-by-zero guard on `share_sell_price = 0`. Pre-empted
    /// by the DB CHECK (`share_sell_price > 0`) and the handler
    /// validator, but the function is standalone-testable and must
    /// not panic on a direct caller.
    NonPositiveSellPrice,
}

pub fn compute(input: SellToCoverInput) -> Result<SellToCoverResult, ComputeError>;
```

`Shares` is the scaled-i64 type from `orbit_core::vesting` (`SHARES_SCALE = 10_000`). Monetary arithmetic inside `compute` widens to `i128` for the multiply-divide chain and narrows back to `i64` on return; `rust_decimal` is not introduced. The frontend parity type mirrors this shape using `decimal.js` on the TS side; the shared fixture pins exact string outputs so the two implementations cannot drift.

**Algorithm.**

```text
// Step 1 — defensive guards.
if input.share_sell_price == 0:
    return Err(NonPositiveSellPrice)        // defense in depth
if input.shares_vested == 0:
    return Ok(all-zero result)              // AC-6.4.4

// Step 2 — gross amount (in FMV currency).
// gross_scaled = fmv_scaled × shares_scaled / SHARES_SCALE
//   (one extra SHARES_SCALE in the numerator is cancelled by the
//   divide; we carry the intermediate as i128 to prevent overflow at
//   million-share grants × four-digit FMVs).
gross_i128 = (input.fmv_at_vest as i128) * (input.shares_vested as i128)
           / (SHARES_SCALE as i128)

// Step 3 — ideal shares-sold (in share units, real number).
//   shares_sold_ideal = (tax_pct × gross) / sell_price
// All three terms are scaled by SHARES_SCALE; the output must be
// scaled by SHARES_SCALE too, so we multiply through and divide
// once at the end.
//   numerator_i128   = tax_pct × gross
//   denom_i128       = sell_price
//   shares_sold_ideal_scaled = (numerator_i128 × SHARES_SCALE)
//                              / (denom_i128 × SHARES_SCALE)
//                            = numerator_i128 / denom_i128
// But we want ceiling at 4 dp per §3 resolution AC-6.3:
shares_sold_ceil_i128 =
    ceil_div(tax_pct_i128 * gross_i128, sell_price_i128 * SHARES_SCALE)

// Step 4 — net shares delivered.
net_shares_i128 = (input.shares_vested as i128) - shares_sold_ceil_i128
if net_shares_i128 < 0:
    return Err(NegativeNetShares)           // §3 resolution AC-6.4.2

// Step 5 — cash withheld = actual shares_sold × sell_price.
// Actual shares_sold is the ceilinged value, not the ideal — the
// residual overwithhold is documented (§3).
cash_withheld_i128 = (shares_sold_ceil_i128 * sell_price_i128)
                   / (SHARES_SCALE as i128)

return Ok(SellToCoverResult {
    gross_amount_scaled: gross_i128 as Shares,
    shares_sold_for_taxes_scaled: shares_sold_ceil_i128 as Shares,
    net_shares_delivered_scaled: net_shares_i128 as Shares,
    cash_withheld_scaled: cash_withheld_i128 as Shares,
})
```

**Currency policy (AC-6.1.2 resolution).** The function itself is currency-agnostic — it takes one `Currency` parameter and assumes FMV and sell_price share it. The handler layer enforces same-currency at write time: if the body contains `fmv_currency != share_sell_currency`, the handler returns 422 with `code = "vesting_event.sell_to_cover.currency_mismatch"`. This is the v1 posture per the task brief; a follow-up in Slice 5 (symmetric NSO sell-to-cover) revisits cross-currency if exercise venues genuinely differ from FMV venues. The DB CHECK constraints do not gate same-currency (the constraint would require a cross-column CHECK with a nuance for "both NULL" that complicates the all-or-none triplet CHECK); the handler is the authoritative gate.

**Rounding policy (AC-6.3 resolution).** `shares_sold_for_taxes` is **ceiling** at 4 decimal places. Rationale: Spanish withholding practice rounds UP on shares_sold so the employer has enough cash to remit to AEAT — a broker selling exactly `shares_sold_ideal` risks a 1-cent cash deficit after settlement fees, and the standard practice is to sell one extra hundredth-of-a-share to cover. The residual `cash_withheld` value is therefore `ceil(shares_sold_ideal, 4 dp) × sell_price`, which may be slightly over the nominal `tax_percent × gross`. This residual over-withhold is documented in the fixture (case `typical_spain_45pct` shows the non-zero overwithhold on a realistic ratio). `cash_withheld` is returned at full internal precision (4 dp on shares × 6 dp on sell_price via i128 arithmetic); the dialog display rounds to 2 decimal places (AC-6.3.2 — banker's rounding at display time, not in the pure function).

**Negative-net policy (AC-6.4.2 resolution).** When `tax_percent = 1.0000` and `sell_price < fmv_at_vest`, the nominal shares-sold-to-cover exceeds shares_vested; `net_shares_delivered` goes negative. v1 rejects this with `ComputeError::NegativeNetShares`. Rationale: a row in this state is almost certainly a user typo (accidentally swapped FMV and sell price, or entered a decimal point wrong on one of the two). The alternative (allow negative; interpret as "employer undercharged; user owes shares at settlement") has no real-world analogue in the Persona-B RSU flow — broker sell-to-cover systems don't produce negative deliveries. If a future slice encounters a legitimate negative-delivery scenario (cross-jurisdiction trueup, for example), we revisit the policy then. For Slice 3b, reject and let the handler surface a 422 with `code = "vesting_event.sell_to_cover.negative_net_shares"` and copy: `El precio de venta es menor que el FMV y la retención es 100 %: revisa los valores.` / `Sell price is below FMV with 100 % withholding: re-check the values.`

**Shared JSON fixture — `backend/crates/orbit-core/tests/fixtures/sell_to_cover_cases.json`.** Each case carries input grant params, input overrides, and expected output. Roughly 12 cases covering:

1. `zero_tax` — `tax_percent = 0`; `shares_sold = 0`, `net = shares_vested`, `cash_withheld = 0`.
2. `full_tax_sell_equals_fmv` — `tax_percent = 1`, `sell_price = fmv`; `shares_sold = shares_vested`, `net = 0`.
3. `typical_spain_45pct` — `tax_percent = 0.45`, `fmv = sell_price = $42`, 100 shares; the headline demo-acceptance case.
4. `sell_above_fmv` — `sell_price > fmv` (broker got lucky on the open; `shares_sold < ideal-at-fmv` but still ceilings at 4 dp).
5. `sell_below_fmv_partial_tax` — `sell_price < fmv`, `tax_percent = 0.45`; realistic IRPF scenario; `shares_sold > shares_vested × tax_percent` but `net > 0` is still preserved.
6. `full_tax_sell_below_fmv_rejects` — `tax_percent = 1`, `sell_price < fmv` → `ComputeError::NegativeNetShares` (AC-6.4.2 resolution).
7. `fractional_shares` — `shares_vested = 1.2345`; ceiling interaction with fractional inputs.
8. `zero_vest` — `shares_vested = 0` → all-zero result, no reject (AC-6.4.4).
9. `zero_fmv` — `fmv_at_vest = 0` → `gross = 0`, `shares_sold = 0`, `net = shares_vested`, `cash = 0` (AC-6.4.5).
10. `tiny_tax_percent` — `tax_percent = 0.0001` (1 bp); exercises ceiling at the `0.000x` boundary.
11. `tiny_sell_price` — `sell_price = $0.0001`; exercises precision in the divide.
12. `million_share_vest` — `shares_vested = 1_000_000`; overflow headroom check (i128 chain must not wrap).

**Parity test — `backend/crates/orbit-core/tests/sell_to_cover_fixtures.rs`.** Loads the JSON, iterates cases, asserts each computed output byte-for-byte against the expected. The frontend mirror test at `frontend/src/lib/__tests__/sellToCover.spec.ts` (Vitest) consumes the same JSON path (CI artifact-copied into the frontend bundle at test time) and asserts identical outputs.

**Property tests** (in addition to the fixture tests). Three invariants tested via `proptest` against randomly-generated inputs:

1. `gross_amount = fmv × shares_vested` exactly (no rounding in this product).
2. `net_shares_delivered + shares_sold_for_taxes = shares_vested` (the ceiling residual flows entirely into `cash_withheld` — the shares equation is exact).
3. `cash_withheld >= tax_percent × gross_amount` (ceiling-on-shares_sold always withholds at least the nominal amount — monotonicity of the rounding direction).

### 3. API contract additions

Path-relative to `/api/v1`. Notation inherited from ADR-010 §9 and ADR-016 §3: `[A]` = authenticated; `[V]` = CSRF-validated state change. All mutation endpoints go through `Tx::for_user(user_id)` per SEC-022.

**User tax preferences (new)**

| Method | Path | Notes |
|---|---|---|
| `GET` | `/user-tax-preferences/current` `[A]` | Returns the currently-open row or null. Response: `{ preferences: { id, countryIso2, rendimientoDelTrabajoPercent: "0.4500" \| null, sellToCoverEnabled: bool, fromDate, createdAt, updatedAt } \| null }`. Always 200 (null body is the first-render state per AC-4.1.3). Rate-limited as a read. |
| `GET` | `/user-tax-preferences` `[A]` | Returns the full history (open + closed rows), ordered `from_date DESC`. Response: `{ preferences: [{ ...same shape, toDate: "YYYY-MM-DD" \| null }] }`. The UI's history table filters the open row client-side (AC-4.5.1). Rate-limited as a read. |
| `POST` | `/user-tax-preferences` `[A]` `[V]` | Close-and-create + same-day idempotency. Body: `{ countryIso2: "ES" \| "PT" \| ..., rendimientoDelTrabajoPercent: "0.4500" \| null, sellToCoverEnabled: true \| false }`. Validators: country in curated list (AC-4.2.1), percent in `[0, 1]` or null (AC-4.2.3; the handler accepts either a stringified fraction `"0.4500"` or a stringified percent `"45"` + converts — see below), `sell_to_cover_enabled` required (no default — AC-4.3.*). Response: `200 { preferences: { ...open row }, outcome: "inserted" \| "closed_and_created" \| "updated_same_day" \| "no_op" }`. `no_op` fires when every submitted field equals the existing open row (parity with `modelo_720_user_inputs`). Audit written on every non-`no_op` outcome per §5. |

**Input conversion on `rendimientoDelTrabajoPercent`.** The wire format is a stringified fraction in `[0, 1]` with up to 4 decimal places (e.g., `"0.4500"`). The client converts from the user-visible percent input (`45`) to the fraction (`0.4500`) before sending. The server rejects wire values outside `[0, 1]` (422 with `code = "user_tax_preferences.percent.out_of_range"`). Rationale: the DB column is a fraction; the wire format matches the storage format; the client-side conversion is a one-line `value / 100` at submit. This mirrors how `fmv_at_vest` is a wire-level decimal string matching the DB column exactly (no unit gymnastics in the handler).

**Extended vesting-event PUT**

| Method | Path | Notes |
|---|---|---|
| `PUT` | `/grants/:grantId/vesting-events/:eventId` `[A]` `[V]` | Body (camelCase DTO), extended from Slice 3: `{ vestDate?, sharesVested?, fmvAtVest? \| null, fmvCurrency? \| null, clearOverride?, taxWithholdingPercent? \| null, shareSellPrice? \| null, shareSellCurrency? \| null, clearSellToCoverOverride?, expectedUpdatedAt }`. Validators + handler behaviour below. Response: `200 { event: { ...row fields, plus derived sell-to-cover values when the triplet is non-null } }`. |
| `GET` | `/grants/:id/vesting` `[A]` | Extended from Slice 3: each event row now carries `taxWithholdingPercent`, `shareSellPrice`, `shareSellCurrency`, `isSellToCoverOverride`, `sellToCoverOverriddenAt`, plus derived `grossAmount`, `sharesSoldForTaxes`, `netSharesDelivered`, `cashWithheld` (all `null` when the triplet is null). Derived values are computed server-side on the GET via the pure function — not persisted. |

**Extended PUT body semantics.**

- **Slice-3 fields unchanged.** `vestDate`, `sharesVested`, `fmvAtVest`, `fmvCurrency`, `clearOverride`, `expectedUpdatedAt` behave exactly as Slice-3 AC-8 pins. FMV-only edits (body omits every sell-to-cover key, sets only `fmvAtVest` / `fmvCurrency`) remain a valid Slice-3 client shape per AC-7.8.3. Shared `updated_at` OCC token gates both tracks (AC-7.4.4).
- **Sell-to-cover triplet (all-or-none).** `taxWithholdingPercent`, `shareSellPrice`, `shareSellCurrency` must be written together or cleared together. Validator: if any one of the three is present non-null in the body and another is explicitly null (or vice versa), 422 with `code = "vesting_event.sell_to_cover.triplet_incomplete"`. `shareSellCurrency` may be omitted to default to `fmvCurrency` per AC-7.3.4 (see §4 default-sourcing).
- **`clearSellToCoverOverride` (narrow clear — new).** Body key `clearSellToCoverOverride: true` clears only `tax_withholding_percent`, `share_sell_price`, `share_sell_currency`; preserves `fmv_at_vest`, `fmv_currency`, `vest_date`, `shares_vested_this_event`, `is_user_override`, `overridden_at`. Sets `is_sell_to_cover_override = false`, `sell_to_cover_overridden_at = NULL`. Mutually exclusive with any sell-to-cover field in the same body (422 with `code = "vesting_event.clear_conflict.narrow"`). Writes one `vesting_event.clear_sell_to_cover_override` audit row. No `vesting_event.clear_override` is written (narrow is narrow — see §5).
- **`clearOverride` (full clear — extended semantics from Slice 3).** Now clears both tracks in one transaction:
  - Reverts `vest_date` + `shares_vested_this_event` to the derivation algorithm's current output for that slot (Slice-3 semantics).
  - **Clears** `fmv_at_vest`, `fmv_currency`, `tax_withholding_percent`, `share_sell_price`, `share_sell_currency` (a departure from Slice-3's "preserve FMV on clearOverride"). Rationale: in Slice-3 the narrow-clear didn't exist — the user had no "clear only sell-to-cover" path — so preserving FMV on `clearOverride` was the non-destructive default. In Slice 3b the narrow-clear exists; the full-clear is the "nuclear" revert and the user has an explicit narrow path if they want to preserve FMV. AC-7.5.1 locks this semantic in. This is the one Slice-3 behaviour Slice 3b modifies; it is documented at the handler site and called out in the Slice-3b CHANGELOG entry.
  - Sets `is_user_override = false`, `overridden_at = NULL`, `is_sell_to_cover_override = false`, `sell_to_cover_overridden_at = NULL`.
  - Writes **two** audit rows in order: `vesting_event.clear_override` (Slice-3 shape) followed by `vesting_event.clear_sell_to_cover_override` (Slice-3b shape), both in the same transaction (AC-7.7.3). The Slice-3 `vesting_event.clear_override` payload's `cleared_fields` array now always contains `["vest_date", "shares", "fmv"]` (FMV is now always cleared on full-clear); `preserved` is always `[]`.
- **Mixed-body rejection.** A body mixing `clearOverride: true` with any non-null value on `vestDate` / `sharesVested` / `fmvAtVest` / `taxWithholdingPercent` / `shareSellPrice` / `shareSellCurrency` is rejected 422 with `code = "vesting_event.clear_conflict.full"`. Same for `clearSellToCoverOverride: true` mixed with any sell-to-cover key (`code = "vesting_event.clear_conflict.narrow"`).
- **OCC gates all paths.** Every mutating path — sell-to-cover write, FMV-only edit, narrow clear, full clear — predicates on `expectedUpdatedAt == row.updated_at` (Slice-3 AC-10.5, extended per AC-8.3.1). A mismatch returns 409 with `code = "resource.stale_client_state"`; no audit row written (AC-7.4.3).

**New error envelope codes (additive over Slice 3).**

- `user_tax_preferences.country.invalid` — country not in the curated list (AC-4.2.1).
- `user_tax_preferences.percent.out_of_range` — percent outside `[0, 1]` (AC-4.2.3).
- `user_tax_preferences.sell_to_cover_enabled.required` — body omits `sellToCoverEnabled`.
- `vesting_event.sell_to_cover.triplet_incomplete` — all-or-none violated (AC-5.2.1).
- `vesting_event.sell_to_cover.currency_mismatch` — FMV currency != sell currency when both non-null (§2 currency policy).
- `vesting_event.sell_to_cover.negative_net_shares` — passthrough of `ComputeError::NegativeNetShares` (AC-6.4.2, §2 resolution).
- `vesting_event.sell_to_cover.requires_fmv` — `shareSellPrice` present but no `fmvAtVest` on row or in body (AC-7.3.4 final sentence).
- `vesting_event.clear_conflict.narrow` — `clearSellToCoverOverride: true` combined with sell-to-cover fields.
- `vesting_event.clear_conflict.full` — `clearOverride: true` combined with any field.

### 4. Default-sourcing of `tax_withholding_percent` — the one-shot rule

The central behavioral subtlety Slice 3b introduces. Source of AC-7.6.1..7 plus the AC-7.6.3 resolution the AC doc explicitly deferred to this ADR.

**Resolution of AC-7.6.3 — null vs omitted body semantics.** Per the task brief, picking option (a): **null in body ⇒ explicit clear (no default-sourcing); omitted from body ⇒ seed from profile if conditions hold; present non-null ⇒ use verbatim**. Three wire cases, three semantics:

1. `body: { taxWithholdingPercent: "0.4700", ... }` — explicit value; use verbatim. Default-sourcing is skipped. Validator gates the `[0, 1]` range.
2. `body: { taxWithholdingPercent: null, ... }` — explicit null; user said "no withholding percent". Default-sourcing is skipped. The row is written with `tax_withholding_percent = NULL`, which may then trip the sell-to-cover-triplet CHECK (§1) if `shareSellPrice` is non-null in the same body — a 422 fires at validation time with `code = "vesting_event.sell_to_cover.triplet_incomplete"`. If `shareSellPrice` is also null, the row simply has no sell-to-cover data (triplet all-null is legal).
3. `body: { ... }` without the `taxWithholdingPercent` key at all — default-sourcing applies if conditions hold.

The dialog UI submits `null` only when the user explicitly clears the field; omission is produced only by raw API callers that omit the key (e.g., a CLI user scripting FMV-only edits). The JSON wire format distinguishes `null` from "absent" natively; the server's deserializer uses `Option<Option<T>>` on the Rust side (outer `Option` = key presence; inner `Option` = null-vs-present) — this is the Slice-3 `deserialize_optional_nullable_string` pattern from `handlers/vesting_events.rs`, extended to the three new keys.

**Default-sourcing conditions (AC-7.6.1).** All of:
- (a) the row's current `is_sell_to_cover_override = false` (i.e., this is the first sell-to-cover override on this row — AC-7.6.6 one-shot);
- (b) the body contains `shareSellPrice` (non-null — the user is creating a sell-to-cover override, not just editing FMV);
- (c) the body **omits** `taxWithholdingPercent` (the key is absent — not explicitly `null`; per the AC-7.6.3 resolution above);
- (d) the user has an active `user_tax_preferences` row with `sell_to_cover_enabled = true` and `rendimiento_del_trabajo_percent IS NOT NULL`.

When all four hold, the handler seeds `tax_withholding_percent` from `user_tax_preferences.rendimiento_del_trabajo_percent` and writes the row accordingly. When any condition fails, the handler does not seed; if the triplet then ends up partial (AC-7.6.5 case), the CHECK fires and 422 surfaces.

**Default-sourcing is read at save time, not at dialog-open time (AC-9.8).** The handler reads `user_tax_preferences` in the same transaction as the vesting-event write. A user who saves a new Profile row on Tab B between Tab A's dialog-open and Tab A's save sees Tab A's save seed against the newly-saved preferences. No warning banner fires; the semantic is "most recent intent wins" and the AC doc pins this as a tester-do-not-flag.

**One-shot semantics (AC-7.6.6).** The default-sourcing check is gated on `is_sell_to_cover_override = false`. Once a row has any sell-to-cover override, subsequent edits with an omitted `taxWithholdingPercent` **do not** re-seed — the user's prior value wins. This prevents a Profile-side percent change from silently mutating historical vest rows. A user who wants to apply the new percent to a prior row must edit that row's dialog explicitly (editing `taxWithholdingPercent` to the new value is one path; issuing a narrow clear + re-save is another — both surface in the audit log as an explicit user action).

**No retroactive back-fill on Profile save (AC-7.6.7).** The Profile save path (`POST /user-tax-preferences`) writes only to `user_tax_preferences`. It never touches `vesting_events`. Default-sourcing fires only at the per-row dialog save path. This is the "minimum surprise" shape: changing the Profile percent does not silently mutate historical tax data on individual vests.

### 5. Audit-log payload allowlist (SEC-101-strict)

Three new actions in Slice 3b. Every payload is hand-rolled `json!({...})` with literal keys per SEC-101. No country codes. No percents. No prices. No amounts. No employer names. No ticker symbols.

**`user_tax_preferences.upsert`.** Written on every non-`no_op` outcome of `POST /user-tax-preferences` (AC-4.6.1). `target_kind = "user_tax_preferences"`, `target_id = <id of the row that ended the transaction as the open row>`. Payload:

```rust
json!({
    "outcome": outcome_str,   // "inserted" | "closed_and_created" | "updated_same_day"
})
```

No country, no percent, no enabled flag. An auditor reconstructing the time-series sees only the cadence of saves, which is sufficient for the AC-4.6.1 threat model.

**`vesting_event.sell_to_cover_override`.** Written on every successful dialog save that mutates at least one sell-to-cover field (i.e., any change to `tax_withholding_percent`, `share_sell_price`, `share_sell_currency`, or an AC-8.2.1 "first sell-to-cover write" that mutates `shares`/`fmv`/`vest_date` as part of establishing the override). `target_kind = "vesting_event"`, `target_id = vesting_event.id`. Payload:

```rust
json!({
    "grant_id": grant_id.to_string(),
    "fields_changed": fields_changed_vec,  // 1..6 symbolic strings
                                            //   drawn from: "tax_percent",
                                            //   "sell_price", "sell_currency",
                                            //   "shares", "fmv", "vest_date"
})
```

The `fields_changed` vector contains only the keys the save actually mutated. A save with no mutations (idempotent re-save of identical values) writes no audit row (AC-7.5.6 parity for the forward path).

**`vesting_event.clear_sell_to_cover_override`.** Written on `clearSellToCoverOverride: true` AND as the second audit row on `clearOverride: true`. `target_kind = "vesting_event"`, `target_id = vesting_event.id`. Payload:

```rust
json!({
    "grant_id": grant_id.to_string(),
})
```

No `preserved` array, no `cleared_fields` array — the action's name carries the semantics (AC-7.7.2). This keeps the payload shape trivially narrow and testable.

**Audit write sequencing on full-clear.** Per AC-7.7.3, a `clearOverride: true` that reverts both tracks writes **two** audit rows in this order, in the same transaction as the row update:

1. `vesting_event.clear_override` (Slice-3 shape — `{ grant_id, cleared_fields: ["vest_date", "shares", "fmv"], preserved: [] }`; note `preserved` is always `[]` in Slice 3b because the full-clear now also clears FMV per §3).
2. `vesting_event.clear_sell_to_cover_override` (Slice-3b shape — `{ grant_id }`).

The order matters for the audit allowlist CI sweep: the T31 test asserts the two rows appear in this order (by `created_at` ASC, then by insert position). The implementation engineer should use a deterministic two-INSERT statement rather than relying on `INSERT ... RETURNING` timing.

**T25 convention.** All three audit writes ride inside the mutation's `Tx::for_user` transaction. A rollback on any step rolls back both the row mutation and the audit rows. No audit row is written on 409 (OCC mismatch) or 422 (validator reject).

### 6. Sequence diagrams

Mermaid; matches the ADR-014 §4, ADR-016 §5, and ADR-017 §6 shape. Two flows are load-bearing.

#### 6.1 Save Profile "Preferencias fiscales"

```mermaid
sequenceDiagram
    autonumber
    participant U as User
    participant SPA as React SPA
    participant API as axum API
    participant PG as Postgres

    U->>SPA: opens Profile → Preferencias fiscales section
    SPA->>API: GET /api/v1/user-tax-preferences/current
    API->>PG: Tx::for_user (app.user_id = user)
    API->>PG: SELECT ... FROM user_tax_preferences \
                WHERE user_id = $1 AND to_date IS NULL LIMIT 1
    PG-->>API: open row (or none)
    API-->>SPA: 200 { preferences: {...} | null }
    SPA->>U: renders form (country picker, percent field per AC-4.2.2, toggle)
    U->>SPA: selects ES, enters 45, checks sell-to-cover, clicks Guardar
    SPA->>SPA: client-side valid; converts 45 → "0.4500"
    SPA->>API: POST /api/v1/user-tax-preferences \
               body { countryIso2: "ES", rendimientoDelTrabajoPercent: "0.4500", \
                      sellToCoverEnabled: true }
    API->>API: validator (country in list, percent in [0,1], toggle present)
    API->>PG: Tx::for_user; BEGIN
    API->>PG: SELECT ... FROM user_tax_preferences \
                WHERE user_id = $1 AND to_date IS NULL LIMIT 1 FOR UPDATE
    alt no prior open row (first save)
        API->>PG: INSERT INTO user_tax_preferences \
                    (user_id, country_iso2, rendimiento_del_trabajo_percent, \
                     sell_to_cover_enabled, from_date) \
                    VALUES ($1, $2, $3, $4, $5) RETURNING id
        API->>PG: INSERT audit_log (action='user_tax_preferences.upsert', \
                   target_kind='user_tax_preferences', target_id=<new id>, \
                   payload={outcome:"inserted"})
    else prior open row, from_date == today (same-day idempotent)
        alt incoming == existing (country, percent, toggle all equal)
            Note over API,PG: NoOp — no DB write, no audit row (AC idempotency).
        else any field differs
            API->>PG: UPDATE user_tax_preferences \
                        SET country_iso2=$1, rendimiento_del_trabajo_percent=$2, \
                            sell_to_cover_enabled=$3 \
                        WHERE id=<open id> RETURNING id
            Note over PG: user_tax_preferences_touch_updated_at trigger \
                          bumps updated_at
            API->>PG: INSERT audit_log (action='user_tax_preferences.upsert', \
                       payload={outcome:"updated_same_day"})
        end
    else prior open row, from_date < today (close-and-create)
        API->>PG: UPDATE user_tax_preferences SET to_date=$today \
                    WHERE id=<prior open id>
        API->>PG: INSERT INTO user_tax_preferences (...) RETURNING id
        API->>PG: INSERT audit_log (action='user_tax_preferences.upsert', \
                   target_id=<new id>, payload={outcome:"closed_and_created"})
    end
    API->>PG: COMMIT
    API-->>SPA: 200 { preferences: {...open row}, outcome }
    SPA->>SPA: invalidates TanStack-Query cache for user-tax-preferences/*
    SPA->>U: re-renders form + history table
```

#### 6.2 Open dialog, save, and revert a vesting-event row

```mermaid
sequenceDiagram
    autonumber
    participant U as User
    participant SPA as React SPA
    participant API as axum API
    participant PG as Postgres

    Note over U,PG: Part 1 — open the dialog.
    U->>SPA: clicks edit on a past vest row
    SPA->>API: GET /api/v1/grants/:gid/vesting
    API->>PG: Tx::for_user; SELECT events
    PG-->>API: rows (including sell-to-cover columns)
    API->>API: for each row with full triplet, compute derived values via \
               orbit_core::sell_to_cover::compute
    API-->>SPA: 200 { events: [{ ..., grossAmount, sharesSoldForTaxes, \
                 netSharesDelivered, cashWithheld }] }
    SPA->>U: opens dialog; derived-values panel renders (dashes if triplet null)
    Note over SPA: client captures row's updatedAt (OCC token)

    Note over U,PG: Part 2 — save a first sell-to-cover override with \
                     omitted taxWithholdingPercent → default-sourcing.
    U->>SPA: enters FMV $42.00 USD, sell price $42.25, leaves tax % blank
    SPA->>API: PUT /api/v1/grants/:gid/vesting-events/:eid \
               body { fmvAtVest: "42.00", fmvCurrency: "USD", \
                      shareSellPrice: "42.25", \
                      expectedUpdatedAt: "<capture>" }
               (taxWithholdingPercent and shareSellCurrency KEYS ABSENT)
    API->>API: validator (currencies, price positive, no mixed-clear conflict)
    API->>PG: Tx::for_user; BEGIN
    API->>PG: SELECT ... FROM vesting_events WHERE id=$1 AND grant_id=$2 \
                FOR UPDATE
    alt updated_at != expectedUpdatedAt
        API-->>SPA: 409 { code: "resource.stale_client_state" }
        SPA->>U: banner "refresh to see current values"
        Note over API,PG: No audit row (AC-7.4.3).
    else OCC match
        Note over API: Default-sourcing gate (§4): \
                       is_sell_to_cover_override=false AND \
                       shareSellPrice present non-null AND \
                       taxWithholdingPercent key ABSENT → seed
        API->>PG: SELECT rendimiento_del_trabajo_percent, sell_to_cover_enabled \
                    FROM user_tax_preferences \
                    WHERE user_id=$1 AND to_date IS NULL LIMIT 1
        PG-->>API: { "0.4500", true }
        Note over API: Seed taxWithholdingPercent="0.4500"; \
                       default shareSellCurrency to fmvCurrency="USD"
        API->>PG: UPDATE vesting_events \
                    SET fmv_at_vest=$1, fmv_currency=$2, \
                        tax_withholding_percent=$3, share_sell_price=$4, \
                        share_sell_currency=$5, \
                        is_user_override=true, overridden_at=now(), \
                        is_sell_to_cover_override=true, \
                        sell_to_cover_overridden_at=now() \
                    WHERE id=$6
        Note over PG: touch_updated_at fires → updated_at=now(); \
                      all CHECKs pass (triplet all-non-null; both \
                      override-flag coherence invariants)
        API->>PG: INSERT audit_log (action='vesting_event.override', \
                   payload={grant_id, fields_changed:["fmv"]})
        API->>PG: INSERT audit_log (action='vesting_event.sell_to_cover_override', \
                   payload={grant_id, fields_changed:["tax_percent","sell_price","sell_currency"]})
        API->>PG: COMMIT
        API-->>SPA: 200 { event with updatedAt_new + derived values }
        SPA->>U: dialog closes; focus returns to row edit button
    end

    Note over U,PG: Part 3 — narrow revert (sell-to-cover only).
    U->>SPA: re-opens dialog; clicks "Revertir solo sell-to-cover"
    SPA->>U: confirm dialog (AC-7.5.5)
    U->>SPA: confirms
    SPA->>API: PUT /api/v1/grants/:gid/vesting-events/:eid \
               body { clearSellToCoverOverride: true, \
                      expectedUpdatedAt: "<capture>" }
    API->>PG: Tx::for_user; BEGIN
    API->>PG: SELECT ... FOR UPDATE (OCC check)
    API->>PG: UPDATE vesting_events \
                SET tax_withholding_percent=NULL, share_sell_price=NULL, \
                    share_sell_currency=NULL, \
                    is_sell_to_cover_override=false, \
                    sell_to_cover_overridden_at=NULL \
                WHERE id=$1
    Note over PG: fmv_at_vest, fmv_currency, is_user_override, overridden_at \
                  all PRESERVED (AC-7.5.2)
    API->>PG: INSERT audit_log (action='vesting_event.clear_sell_to_cover_override', \
               payload={grant_id})
    API->>PG: COMMIT
    API-->>SPA: 200 { event with derived values now null }

    Note over U,PG: Part 4 — full revert (both tracks).
    U->>SPA: clicks "Revertir todos los ajustes"
    SPA->>U: confirm dialog (AC-7.5.4)
    U->>SPA: confirms
    SPA->>API: PUT /api/v1/grants/:gid/vesting-events/:eid \
               body { clearOverride: true, expectedUpdatedAt: "<capture>" }
    API->>PG: Tx::for_user; BEGIN; OCC check; SELECT existing row
    API->>API: compute algorithmic vest_date + shares for this slot \
               via orbit_core::vesting::derive_vesting_events
    API->>PG: UPDATE vesting_events \
                SET vest_date=$1, shares_vested_this_event=$2, \
                    fmv_at_vest=NULL, fmv_currency=NULL, \
                    tax_withholding_percent=NULL, share_sell_price=NULL, \
                    share_sell_currency=NULL, \
                    is_user_override=false, overridden_at=NULL, \
                    is_sell_to_cover_override=false, \
                    sell_to_cover_overridden_at=NULL \
                WHERE id=$3
    API->>PG: INSERT audit_log (action='vesting_event.clear_override', \
               payload={grant_id, cleared_fields:["vest_date","shares","fmv"], \
                        preserved:[]})
    API->>PG: INSERT audit_log (action='vesting_event.clear_sell_to_cover_override', \
               payload={grant_id})
    API->>PG: COMMIT
    API-->>SPA: 200 { event fully reverted }
```

### 7. What Slice 3b explicitly defers (make TBD impossible)

The following are **designed but not implemented** in Slice 3b. Each is listed here so the implementation engineer never sees a TBD.

| Deferred item | Slice | Note |
|---|---|---|
| Tax math (IRPF projection, ahorro-base, rendimiento-del-trabajo) | 4 | Slice 3b captures the sell-to-cover data; the dialog's derived values are display-only. IRPF projection, Art. 7.p math, RSU cap-gains basis recomputation all ship in Slice 4. |
| RSU cap-gains basis formula amendment (spec L319/L334) | 4 | Per v1.5 Q-C: `basis = fmv_at_vest × net_shares_delivered` when sell-to-cover applied. Slice 3b captures `net_shares_delivered` as a derived field; Slice 4 is where the cap-gains formula consumes it. Spec text update lands in the Slice 4 patch. The paper-gains tile in Slice 3b continues to compute on `fmv × shares_vested_this_event` (Slice-3 AC-5.4.1 unchanged). |
| Modelo 720 securities derivation amendment | 4 | Same rationale — M720 securities total in Slice 3b still uses `fmv × shares_vested_this_event` per Slice-3 AC-6.1.1. The sell-to-cover-adjusted variant ships alongside the cap-gains basis amendment in Slice 4. |
| NSO sell-to-cover | 5 | NSO exercise mechanics (`nso_exercises` table) and sell-to-cover on exercise both ship in Slice 5. The Slice-3b dialog is gated to `grants.instrument ∈ {rsu, espp}` for sell-to-cover editability; FMV-only editing on `nso` / `iso_mapped_to_nso` rows remains available through the dialog without the sell-to-cover fields rendered. The handler rejects a sell-to-cover body on non-RSU/ESPP grants with `code = "vesting_event.sell_to_cover.instrument_unsupported"`. |
| Dual-residency concurrent `user_tax_preferences` rows | post-v1 | One open row per user at a time per the partial UNIQUE. Overlapping periods (e.g., a user with genuine dual ES/UK residency) are not modelable in Slice 3b. Requires either a second jurisdiction-scoped sidecar or a composite partial UNIQUE on `(user_id, jurisdiction)` — both are follow-up design decisions. |
| GeoIP country auto-detect on Profile first render | 9 | Per v1.5 Q-D carry-over: Slice 3b's Profile starts with an empty country picker on first render (AC-4.1.3). GeoIP population lands in Slice 9 alongside `sessions.country_iso2`. |
| Per-grant tax-percentage default | post-v1 | Per v1.5 Q-D: the percent lives on the user, not on the grant. A per-grant default would require a new `grants.tax_withholding_percent_override` column + resolution precedence in the handler; out of scope for v1. |
| Automatic FMV ↔ sell-price reconciliation banner | post-v1 | A user who enters `fmv = $42` and `sell_price = $42_000` gets no warning banner in Slice 3b. Heuristic banners (e.g., "sell price is > 100× FMV; did you mean $42.00?") are a Slice 4+ polish concern. |
| Retroactive back-fill of sell-to-cover on pre-Slice-3b vests | Never (scope lock) | Per AC-5.1.2 + AC-7.6.7: existing Slice-1/-2/-3 vest rows carry `tax_withholding_percent = NULL` until the user explicitly edits them via the dialog. Saving the Profile form does not walk existing rows. |
| Bulk-edit of sell-to-cover fields | Never (scope lock) | Per Slice-3b tester-do-not-flag: the Slice-3 bulk-fill modal continues to bulk-fill only FMV. No "apply sell-to-cover to all" analogue. Users open the dialog per row. |
| History-table editability (retroactive correction of a prior period) | post-v1 | Per AC-4.5.3: prior closed rows are read-only. If a user needs to correct a historical period, admin intervention is the only path — not exposed in Slice 3b. |
| OCC on `user_tax_preferences` saves | post-v1 (if ever) | Per AC-9.4: Profile saves are last-write-wins by design. The `updated_at` column is in the schema for a future retrofit but no handler consults it. |
| Override badge on the cumulative vesting timeline (Gantt + curve toggle) | 4 polish | Per Slice-3 carry-over: overridden rows surface in the "Precios de vesting" table via the chip, but the timeline Gantt / cumulative-curve toggle does not yet carry an override badge. Slice-3b does not change this. |
| Market-data auto-fill of `share_sell_price` | 5 | `share_sell_price` is user-entered in Slice 3b — no broker-quote lookup. Finnhub integration remains Slice 5. |
| FX conversion inside the dialog | Never (scope lock) | The dialog's derived-values panel renders in the grant's native currency. EUR conversion lives on the Slice-3 paper-gains tile on the dashboard and nowhere else in Slice 3b. |

### 8. Performance and rate-limit targets applied to Slice 3b

- `POST /api/v1/user-tax-preferences` ≤ **200 ms P95** — one `SELECT ... FOR UPDATE`, up to two `INSERT`/`UPDATE`s, one audit `INSERT`, one `COMMIT`. Same round-trip profile as `POST /modelo-720-inputs`.
- `GET /api/v1/user-tax-preferences/current` ≤ **100 ms P95** — trivial read.
- `GET /api/v1/user-tax-preferences` (history) ≤ **150 ms P95** — reads all rows for a user (bounded well under 100 in realistic use).
- `GET /api/v1/grants/:id/vesting` with derived values ≤ **300 ms P95** at 240 events per grant. The pure function is O(1) per row; the per-grant iteration dominates at negligible cost.
- `PUT /api/v1/grants/:gid/vesting-events/:eid` with sell-to-cover body ≤ **250 ms P95** — one `SELECT ... FOR UPDATE`, one possible `SELECT` on `user_tax_preferences` (default-sourcing path), one `UPDATE`, one or two audit `INSERT`s, one `COMMIT`.
- Dialog open-to-interactive latency ≤ **200 ms P95** (client-side + network); the derived-values panel paints on first render with the row's current values (AC-7.8 §"Performance targets — extended").

**Per-user rate limits** (SEC-160; SQL-backed leaky bucket — same mechanism as Slices 1–3):

- Write endpoint on tax preferences (`POST /user-tax-preferences`): **60 / user / hour**. The Profile save is a low-frequency action; 60 is generous headroom for a user who iterates on their percent value.
- Read endpoints on tax preferences (`GET /user-tax-preferences/*`): **600 / user / hour** — matches the ADR-017 chip-read rationale.
- Vesting-event PUT (already bounded at **120 / user / hour** per Slice 3) — unchanged; the Slice-3b body extension doesn't grow the budget.

### 9. Test plan summary

Summary; detailed test work is `qa-engineer`'s. The ADR pins what MUST be tested.

**Property / fixture tests (Rust + TS parity).**

- `orbit_core::sell_to_cover::compute` cases 1–12 from `sell_to_cover_cases.json`; bit-identical across Rust and TS.
- Three invariants via `proptest`: (i) `gross = fmv × shares_vested` exact; (ii) `net + sold = shares_vested` exact; (iii) `cash_withheld >= tax_pct × gross` (ceiling monotonicity).
- Rounding edge cases: a fuzz generator covers the 4-dp ceiling boundary (inputs chosen so `shares_sold_ideal` lands exactly at a 4-dp tick vs one nano-unit above).

**Integration tests (backend, against a real Postgres).**

- **`user_tax_preferences` close-and-create.** First save → `Inserted`. Save on the next day with different values → `ClosedAndCreated`; prior row now has `to_date = today`. Save again same day with same values → `NoOp`; no audit. Save same day with different values → `UpdatedSameDay`; no new row materializes.
- **`user_tax_preferences` country change carries sell_to_cover toggle.** Save `ES 45 % on` → switch to `PT` → toggle pre-unchecks to `false` (client-side) → save; the closed row retains the 45 % percent; the new open row has percent NULL (client hid the field) and toggle `false`.
- **Default-sourcing — first override with omitted tax percent.** User has active prefs with percent `0.4500` and toggle `true`. PUT body sets only `fmvAtVest`, `fmvCurrency`, `shareSellPrice` — no `taxWithholdingPercent` key at all. Assert: row is written with `tax_withholding_percent = 0.4500`, `share_sell_currency` defaults to `fmvCurrency`, `is_user_override = true`, `is_sell_to_cover_override = true`; audit `vesting_event.override` (fields `["fmv"]`) + `vesting_event.sell_to_cover_override` (fields `["tax_percent","sell_price","sell_currency"]`).
- **Default-sourcing — explicit null suppresses seeding (AC-7.6.3).** Same user. PUT body sets `taxWithholdingPercent: null` + `shareSellPrice: "42.25"`. Assert: 422 with `code = "vesting_event.sell_to_cover.triplet_incomplete"` (row not written; null stays null; triplet would be partial).
- **Default-sourcing — `sell_to_cover_enabled = false` suppresses seeding (AC-7.6.4).** User has active prefs with toggle `false`. PUT body omits `taxWithholdingPercent`. Assert: 422 with `code = "vesting_event.sell_to_cover.triplet_incomplete"`.
- **One-shot semantics (AC-7.6.6).** Row already has `is_sell_to_cover_override = true` with `tax_withholding_percent = 0.4500`. Profile percent changes to `0.4600`. PUT body omits `taxWithholdingPercent` + edits `shareSellPrice` to a new value. Assert: row retains `tax_withholding_percent = 0.4500` (no re-seed); audit fires with `fields_changed = ["sell_price"]` only.
- **Narrow clear preserves FMV (AC-7.5.2).** Row has full triplet + `fmv_at_vest = $42`. PUT `clearSellToCoverOverride: true`. Assert: triplet cleared; FMV preserved; `is_user_override = true` preserved; `is_sell_to_cover_override = false`; one `vesting_event.clear_sell_to_cover_override` audit row; no `vesting_event.clear_override`.
- **Full clear reverts both tracks and dual-writes audit (AC-7.5.1 + AC-7.7.3).** Row has both overrides. PUT `clearOverride: true`. Assert: FMV cleared, triplet cleared, vest_date + shares reverted to derivation output, both override flags false; two audit rows in order: `vesting_event.clear_override` (`cleared_fields: ["vest_date","shares","fmv"]`, `preserved: []`) followed by `vesting_event.clear_sell_to_cover_override` (`{grant_id}`).
- **Mixed-body rejections.** `clearOverride: true` + `fmvAtVest: "42"` → 422 `clear_conflict.full`. `clearSellToCoverOverride: true` + `taxWithholdingPercent: "0.45"` → 422 `clear_conflict.narrow`.
- **Currency-mismatch rejection.** Body with `fmvCurrency: "USD"` + `shareSellCurrency: "EUR"` both non-null → 422 `currency_mismatch`. Body with `shareSellPrice` + no `fmvCurrency` on row or in body → 422 `requires_fmv`.
- **Negative-net rejection (AC-6.4.2 resolution).** Row seeded with `fmv = $100, sell_price = $40` via an earlier save. PUT with `taxWithholdingPercent: "1.0000"` → 422 `negative_net_shares`.
- **OCC on every path.** Stale `expectedUpdatedAt` → 409 on each of: sell-to-cover write, FMV-only edit, narrow clear, full clear. No audit row written on any 409.
- **OCC crosses tracks (AC-7.4.4).** Tab A opens dialog (captures `updated_at` = T0). Tab B edits FMV (via direct API call); `updated_at` = T1. Tab A submits sell-to-cover save with `expectedUpdatedAt = T0` → 409 `stale_client_state`.
- **Cross-tenant 404 (AC-9.3).** User A creates a `user_tax_preferences` row; user B `GET`/`POST`/`PUT` on A's row id or endpoint with A's user_id → 404 (not 403). Same probe for extended vesting-event PUT when the grant's user_id ≠ caller.
- **Instrument gate on sell-to-cover body (deferral item).** PUT with sell-to-cover fields on a grant with `instrument = 'nso'` → 422 `instrument_unsupported`.
- **Audit payload allowlist sweep.** For every new Slice-3b audit action, assert the serialized `payload_summary` matches the allowlisted shape exactly (no percents, no prices, no currency strings, no amounts, no country codes). Enforced in CI via `backend/crates/orbit-api/tests/audit_allowlist_sweep_slice_3b.rs` — the Slice-3 sweep extended with Slice-3b keys.

**Frontend unit + E2E tests.**

- Vitest on `sellToCover.ts` against the shared fixture.
- Vitest on the dialog's client-side validators (currency matching, required-field matrix, mutual-exclusion of clear buttons with field edits).
- Playwright on the Slice-3b demo script (AC doc §12, 25 steps).
- `axe-core` on the Preferencias fiscales section (first-render, post-first-save, post-country-switch states) and the dialog (empty state with dashes, populated state with derived values, 409 banner state, unsaved-changes confirm state).
- Keyboard-only walkthrough (G-33 + G-35) across steps 6–20 of the demo script.

### 10. Assumptions and escalations

Three items warrant explicit recording.

#### 10.1 Currency enforcement — v1 rejects cross-currency FMV/sell_price

Per §2 currency policy. The v1 assumption is that a vest's FMV currency and the broker's sell-price currency match (broker-native; typically USD for US employers). The handler gates same-currency and the `compute` function assumes same-currency. Rationale: (a) the primary persona's case (María + US-headquartered employer) always has both values in USD; (b) genuine cross-currency FMV/sell_price is a Slice-5 NSO-exercise concern where the grant is denominated in one currency (e.g., USD 409A FMV) and the exercise venue quotes another (e.g., GBP for London-listed employer); (c) allowing cross-currency in v1 would require a conversion inside `compute`, which brings in FX lookup, band semantics, and staleness — a non-trivial widening.

**Cost of the opposite decision** (allow cross-currency, convert via ECB inside `compute`): the pure function loses its purity (needs an FX rate parameter or an async FX lookup), the fixture cases multiply (each case needs an FX scenario), and the demo-acceptance script adds FX-in-dialog UX that Slice 3b explicitly excluded per AC-7.2.5.

**Follow-up if cross-currency is ever needed.** Slice 5's NSO track is the natural place to relax the constraint. The path: `compute` gains an optional `fx_rate` parameter; the handler looks up the FX rate via Slice-3's `lookup_walkback` helper when the two currencies differ; the fixture cases grow. Estimated effort: one day.

#### 10.2 Rounding — ceiling on shares_sold, residual overwithhold documented

Per §2 rounding policy. The chosen direction is **ceiling** at 4 decimal places. The residual (cash withheld slightly exceeds `tax_percent × gross`) is documented in the `typical_spain_45pct` fixture case and in a `// DOCUMENTED RESIDUAL` comment at the `compute` call site. The UI does not surface the residual as a warning; the dialog's "Retenido en efectivo" value simply shows the ceilinged amount.

**Cost of the opposite decision** (floor on shares_sold): a broker selling one hundredth of a share fewer than the ideal risks a 1-cent cash deficit at AEAT remittance. Spanish practice rounds UP for this reason. Flooring would produce a user-visible 1-cent underwithhold that the user then reads as "Orbit is wrong about my tax"; ceiling matches the broker's statement and avoids the user-visible discrepancy.

**Follow-up if rounding policy changes.** Fixture case `typical_spain_45pct` is the regression gate. A one-line change to the `ceil_div` call becomes a `floor_div` call; the fixture's expected values flip; the documented residual flips sign. Trivial to reverse.

#### 10.3 Nullability posture — five new columns, four nullable

Per §1 DDL. `is_sell_to_cover_override` is `NOT NULL DEFAULT false` (every row carries a definite flag); the other four Slice-3b columns are nullable. A row with the flag `true` has all four null-able columns non-null (enforced by the all-or-none triplet CHECK + the flag-coherence CHECK); a row with the flag `false` has them all null. The two state-spaces are the only legal states.

**Open door for a future `sell_to_cover_source` enum.** A post-v1 slice may want to distinguish user-entered sell-to-cover from broker-CSV-imported sell-to-cover from profile-defaulted sell-to-cover. The nullable-columns posture leaves the door open: a `sell_to_cover_source TEXT CHECK (... IN ('manual','imported_from_broker_csv','defaulted_from_profile'))` column can be added without a data migration on existing rows (nullable default `NULL` on additive migration; handler writes the source value on every new write; reads default to `'manual'` when null). No Slice-3b work needed; the schema is already shaped to accommodate.

### 11. Alternatives considered

- **`user_tax_preferences` as a JSONB column on `users`.** Rejected. The time-series semantics (history table of prior periods per AC-4.5.1) require separate rows. A JSONB column would either lose history (one value per user) or reinvent the close-and-create pattern inside a JSONB array, which loses SQL-level integrity guarantees (no partial UNIQUE, no easy "active row" lookup, no RLS scope on rows). The sidecar table is the boring correct shape.
- **Sell-to-cover as its own sidecar table.** Rejected. The relationship is one-to-one with `vesting_events` (every vest with sell-to-cover data has exactly one sell-to-cover record). Columns on `vesting_events` are simpler to query (no JOIN on every dialog read) and simpler to invariant (one row, one CHECK on the triplet). A sidecar would force a two-table write on every override + an outer join on every read for zero correctness benefit. The Slice-3 `is_user_override` column precedent also argues for columns-on-row.
- **Storing derived values (`gross`, `shares_sold`, `net_delivered`, `cash_withheld`) on `vesting_events`.** Rejected. Can always recompute from the four captured inputs (`fmv`, `shares_vested`, `tax_percent`, `sell_price`); storage cost (four more columns × 240 events × 10 000 users) outweighs the redundancy. The pure function is O(1); recomputing on every GET is cheap (< 1 ms per row on a realistic workload). Storage would also introduce a "stale derived value" failure mode (e.g., fmv edited but derived `gross` not refreshed) that the column-coherence CHECK couldn't easily guard without a trigger. Compute-on-read is simpler.
- **Single `tax_percent` column on `users`.** Rejected per Ivan's explicit sidecar preference (v1.5 Q-D rationale). A column on `users` loses history, forces a retroactive-update decision on every Profile save, and couples the percent to the user's identity rather than to a time-bounded tax scenario. The sidecar + partial UNIQUE shape is forward-compatible with Slice-4 residency-period cross-joins and the post-v1 dual-residency concern.
- **One `overrides` column on `vesting_events` carrying a JSONB payload of all override fields.** Rejected. CHECK constraints on JSONB are verbose and hard to maintain; RLS does not interact cleanly with JSONB queries; the existing two-flag + typed-column shape composes cleanly with the Slice-3 FMV track. Boring, consistent with Slices 1–3.
- **Reuse the Slice-3 `is_user_override` flag for both tracks.** Rejected. The two tracks revert independently (narrow vs full clear). A single flag would force either the full-clear to also clear FMV (Slice-3 preserved FMV on full-clear — a behaviour change would be destructive) or the narrow-clear to leave the flag set after clearing sell-to-cover (which mis-reports the row's state). Two flags are the only composition that makes both revert paths correct. The visual chip at the row level collapses to a single "Ajustado manualmente" signal, so the two-flag shape is invisible to the user outside the dialog.
- **`orbit_core::sell_to_cover::compute` as an `async` function taking a DB handle for default-sourcing.** Rejected. The pure function is the testable surface; default-sourcing is handler concern. Mixing the two would force every fixture case to stub a DB; the TS parity mirror would need a fake DB; the property tests would need a mock. Keeping `compute` pure and putting default-sourcing in the handler is the same discipline Slice 3's `paper_gains::compute` used (FX resolution in handler, pure math in core).
- **Omit `share_sell_currency` and force `= fmv_currency` at the DB level.** Rejected. Per v1.5 Q-B, the broker's per-share sell price may differ from the FMV used for income recognition; the currency might also differ in a future Slice-5 NSO case. Keeping a distinct column gives the DDL forward headroom; the handler's current same-currency gate is a one-line removal when Slice 5 relaxes it. The AC-7.3.4 default-to-fmv-currency is a handler default, not a DDL invariant.
- **OCC on `user_tax_preferences`.** Rejected per AC-9.4. The last-write-wins posture is the deliberate v1 semantic — the history table preserves prior writes regardless, so there is no risk of losing data. Adding OCC would slow down the Profile save by one round-trip and introduce a 409 surface for a flow that doesn't need it.
- **Tighten the Slice-3 `clearOverride: true` to also preserve FMV in Slice 3b.** Rejected. The AC-7.5.1 lock says the full-clear is the "nuclear" revert; preserving FMV on full-clear would leave a row with `is_user_override = true AND fmv_at_vest = <value>` and force a second click through the narrow-clear (which doesn't exist for the FMV track) to reach algorithm-defaults. Clearing FMV on full-clear gives the user a single-click path to "revert everything on this row" and the narrow-clear is the non-destructive alternative for users who want to keep FMV. The Slice-3 → Slice-3b behaviour change is documented at the handler site and in the Slice-3b CHANGELOG.
- **Store `rendimiento_del_trabajo_percent` as a percent (0–100) rather than a fraction (0–1).** Rejected. The sell-to-cover computation consumes it as a fraction; storing as a percent would force the handler to divide by 100 on every read. The wire format matches the storage format; the one conversion lives in the client's submit handler (`value / 100`), which is the natural boundary.

## Consequences

**Positive:**

- Every Slice-3b AC traces to a concrete schema column, trigger, handler path, audit payload shape, pure-function rule, or deferral note. No TBD.
- RLS enforced from the first commit on the new user-scoped table via the inherited `tenant_isolation` policy; the SEC-020 CI introspection test extends with one table name.
- The sell-to-cover computation is a pure function with a shared parity fixture — same discipline as the Slice-1 vesting algorithm, the Slice-2 stacked-cumulative algorithm, and the Slice-3 paper-gains algorithm.
- The two-flag override posture (`is_user_override` + `is_sell_to_cover_override`) composes cleanly with Slice-3's `derive_vesting_events` preservation rule; the implementation engineer does not need to re-design ADR-017's algorithm, only extend the preservation predicate to `OR is_sell_to_cover_override = true`.
- Default-sourcing's one-shot rule (AC-7.6.6) is fully specified: null vs omitted distinction pinned (§3), active-row read at save time pinned (§4), no retroactive back-fill pinned (§4). The implementation engineer has no ambiguity about when to seed.
- The sidecar + partial UNIQUE shape is forward-compatible with Slice-4 residency-period cross-joins and a post-v1 `sell_to_cover_source` enum; the nullability posture leaves the door open for either without a schema rewrite.
- Full-clear's dual audit write (§5 + §6) is deterministic and CI-testable; the audit-allowlist sweep can assert the pair and the order without a flaky timing test.

**Negative / risks:**

- The Slice-3 full-clear behaviour changes in Slice 3b (FMV is now cleared on `clearOverride: true`). This is a user-visible regression from Slice 3 in the specific case of a user who relied on the Slice-3 preservation; the narrow-clear is the new path. Mitigation: the Slice-3b CHANGELOG entry calls this out; the demo script's step 14 demonstrates the new behaviour; the audit shape's `preserved: []` field records the change for any auditor reconstructing a Slice-3-era vs Slice-3b-era history.
- Default-sourcing's null-vs-omitted distinction is subtle. A direct API caller who naively submits `taxWithholdingPercent: null` (thinking "I don't have a value") loses default-sourcing — they must omit the key entirely. Mitigation: the handler's 422 copy explicitly names the profile path (`Introduce un % de retención o configura tus Preferencias fiscales en tu perfil.`), and the dialog never submits `null` (only omits or submits non-null), so the distinction only bites raw-API users.
- The one-shot semantics (AC-7.6.6) means a Profile percent change does NOT propagate to historical vests. A user who updates their Profile from 45 % to 47 % and expects their prior vests to reflect 47 % will be surprised. Mitigation: the Profile section's helper copy makes this explicit (`Orbit la aplica por defecto al sell-to-cover de nuevos vestings`); the demo script's step 18 demonstrates the new percent applying to a fresh row.
- Rounding residual (cash withheld slightly exceeds nominal `tax × gross`) is invisible to the user. A tester comparing dialog values to a hand-computed `45 % × $4200 = $1890` will see the ceilinged cash_withheld may be a penny or two above. Mitigation: the `typical_spain_45pct` fixture pins the expected residual; the Slice-3b CHANGELOG notes the ceiling direction.
- Currency-mismatch rejection in v1 is a UX gap for users with legitimate cross-currency FMV/sell-price data. Mitigation: the 422 copy names the follow-up (`Esta versión requiere la misma moneda para FMV y precio de venta`); Slice 5 relaxes.
- Schema complexity on `vesting_events`: three CHECK constraints (Slice-3 FMV pair + Slice-3 override flag + Slice-3b triplet + Slice-3b sell-to-cover flag — four CHECKs in total now) raise the cognitive load on anyone reading the DDL. Mitigation: the CHECK names are self-documenting (`fmv_pair_coherent`, `override_flag_coherent`, `sell_to_cover_triplet_coherent`, `sell_to_cover_override_flag_coherent`); the migration comments pin the invariants.

**Tension with prior ADRs:**

- **ADR-017 §3 full-clear behaviour superseded.** The Slice-3 `clearOverride: true` preserved FMV; Slice 3b clears it. This ADR explicitly supersedes ADR-017's AC-8.7.1 behaviour for the `clearOverride` path. ADR-017 is not otherwise contradicted; the override-preservation rule on `derive_vesting_events`, the OCC via `updated_at`, and the `overridden_at` vs `updated_at` split are all reused verbatim.
- **ADR-016 close-and-create pattern reaffirmed.** `user_tax_preferences` is the second sidecar table using the pattern (after `modelo_720_user_inputs`); the `residency_periods` close-and-create is the pattern's original home. No contradiction — the pattern is codified at this point and Slice 3b adopts it.
- **ADR-005's entity outline extended.** ADR-005 did not anticipate `user_tax_preferences` (Slice 3b is a net-new slice per v1.5 Q-A); this ADR authors the table's shape. No contradiction with ADR-005's RLS posture — the new table is user-scoped per ADR-005's "shared-schema multi-tenancy" model.
- **Spec L319/L334 basis text to be amended in Slice 4.** Slice 3b captures the data; Slice 4 changes the formula. ADR-018 pins the data shape today so Slice 4 has no schema work ahead of its text amendment.

**Follow-ups (not blocking Slice 3b):**

- **Slice 4.** Update the RSU cap-gains basis formula to consume `net_shares_delivered` when `is_sell_to_cover_override = true` (spec L319/L334 amendment). Update the M720 securities derivation to use the same nuance.
- **Slice 4.** Update the paper-gains tile's computation to consume `net_shares_delivered` on RSU rows with sell-to-cover; the algorithm in `orbit_core::paper_gains::compute` gains a `net_shares_delivered` branch on the RSU basis.
- **Slice 5.** Symmetric `nso_exercises` sell-to-cover capture; `fmv_at_exercise` + `share_sell_price_at_exercise` + `tax_withholding_percent_at_exercise` columns; the `compute` pure function consumed on the NSO sell-to-cover track.
- **Slice 5 (optional).** Relax the same-currency gate in the `PUT` handler if the NSO exercise-venue case needs it.
- **Post-v1.** `sell_to_cover_source` enum on `vesting_events` to distinguish manual, broker-CSV-imported, and profile-defaulted sell-to-cover writes. Feeds into an auditor-facing timeline of how each row's sell-to-cover data was sourced.
- **Post-v1.** Dual-residency concurrent `user_tax_preferences` rows (composite partial UNIQUE on `(user_id, jurisdiction)` or a second jurisdiction-scoped sidecar).
- **Post-v1.** Per-grant tax-percent default (`grants.tax_withholding_percent_override`) with resolution precedence (grant override > user preference > no default).
- **Post-v1.** Automatic FMV ↔ sell-price reconciliation banner with heuristic thresholds (e.g., "sell price is > 100× FMV; did you mean $X.XX?").
- **Slice 9.** GeoIP country auto-detect on Profile first render (alongside `sessions.country_iso2` population).
- **Implementation engineer (Slice 3b).** Author `sell_to_cover_cases.json` fixture (~12 cases covering §2 case list); wire both backend and frontend. Co-locate with Slice-1/-2/-3 fixtures at `backend/crates/orbit-core/tests/fixtures/`.
- **Implementation engineer (Slice 3b).** Extend the audit-payload-shape CI fixture with the three new Slice-3b actions (`user_tax_preferences.upsert`, `vesting_event.sell_to_cover_override`, `vesting_event.clear_sell_to_cover_override`).
- **Implementation engineer (Slice 3b).** Extend the `Tx::for_user` cross-tenant probe suite (SEC-023) to cover every new `[A]` user-scoped endpoint listed in §3.
- **Security-engineer (Slice 3b).** Confirm that the `tax_withholding_percent`, `share_sell_price`, and `rendimiento_del_trabajo_percent` values are scrubbed from every `orbit_log::event!` path per G-29 extended. Confirm that the audit payloads never leak the three PII-adjacent values.
- **QA-engineer (Slice 3b).** Land the sell-to-cover fixture-parity suite, the default-sourcing integration suite (four triggering conditions × four suppression conditions), the dual-audit sequencing suite, and the demo-script Playwright flow (25 steps).
