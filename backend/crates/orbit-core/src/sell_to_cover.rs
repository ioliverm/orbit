//! Sell-to-cover pure computation (Slice 3b T38, ADR-018 §2 + §4).
//!
//! The sell-to-cover dialog and the `GET /grants/:id/vesting` derived-
//! values projection both call [`compute`]. Pure — no DB, no HTTP, no
//! async — so the same function backs the shared fixture
//! (`sell_to_cover_cases.json`) that the frontend parity mirror
//! consumes.
//!
//! # Algorithm (ADR-018 §4)
//!
//! For each vesting event whose user has captured the sell-to-cover
//! triplet `(tax_withholding_percent, share_sell_price,
//! share_sell_currency)`:
//!
//! 1. **Gross amount in FMV currency**:
//!    `gross = fmv_at_vest × shares_vested_this_event` (exact i128).
//! 2. **Shares sold for taxes, ceiling at 4 dp**:
//!    `shares_sold = ceil_to_4dp( (tax × gross) / sell_price )`.
//! 3. **Net shares delivered**:
//!    `net = shares_vested − shares_sold`. Negative → error.
//! 4. **Cash withheld (actual)**: `cash = shares_sold × sell_price`.
//!
//! Ceiling-on-shares-sold rounding is the Spanish withholding-practice
//! convention (broker sells UP so remittance doesn't under-collect —
//! ADR-018 §10.2). The residual `cash_withheld >= tax × gross` is
//! expected and documented.
//!
//! # Numeric model
//!
//! Inputs and outputs are scaled `i64` in units of `1/SHARES_SCALE`
//! (`SHARES_SCALE = 10_000`) — same convention as
//! [`crate::vesting`]. Internal arithmetic widens to `i128` for the
//! multiply / divide chain so a million-share grant times a four-digit
//! FMV cannot overflow:
//!
//! ```text
//! max(shares_scaled)  ≈ 1e16
//! × max(fmv_scaled)   ≈ 1e10  (NUMERIC(20,6) → handler-capped)
//! = 1e26
//! ```
//!
//! `i128::MAX` ≈ `1.7 × 10^38`, a comfortable 12 orders of magnitude
//! of headroom. No `rust_decimal` dep is added; the same bridge idiom
//! is used throughout `orbit_core`.
//!
//! # Currency policy (ADR-018 §2)
//!
//! `compute` assumes the caller has already asserted
//! `fmv_currency == share_sell_currency`. The handler layer enforces
//! this before constructing the input; see `orbit-api/handlers/
//! vesting_events.rs` for the 422 mapping.
//!
//! Traces to:
//!   - ADR-018 §4 (authoritative algorithm).
//!   - ADR-018 §6 (ambiguity resolutions: ceiling, negative-net,
//!     null-vs-omitted).
//!   - docs/requirements/slice-3b-acceptance-criteria.md §6 (AC-6.*).

use serde::{Deserialize, Serialize};

use crate::vesting::{Shares, SHARES_SCALE};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Input to [`compute`]. Every numeric field is scaled `i64`; the caller
/// is responsible for enforcing same-currency across `fmv_at_vest` and
/// `share_sell_price` (see module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SellToCoverInput {
    /// Per-share FMV at vest, scaled (one unit = `1 / SHARES_SCALE` of
    /// the native currency — matches [`crate::vesting::Shares`]
    /// convention so shares × FMV is a plain i128 multiplication).
    pub fmv_at_vest_scaled: Shares,
    /// Scaled share count (one unit = `1 / SHARES_SCALE` of a share).
    pub shares_vested_scaled: Shares,
    /// Tax-withholding fraction in `[0, 1]`, scaled
    /// (0 = 0 %; `SHARES_SCALE` = 100 %).
    pub tax_withholding_percent_scaled: Shares,
    /// Per-share sell price at vest, scaled.
    pub share_sell_price_scaled: Shares,
}

/// Output of [`compute`]. Every numeric field is scaled `i64` matching
/// the input convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SellToCoverResult {
    /// `fmv × shares_vested`, scaled. The income recognition amount in
    /// the FMV currency.
    pub gross_amount_scaled: Shares,
    /// `ceil_4dp( tax × gross / sell_price )`, scaled. The number of
    /// shares the broker sells to cover the withholding obligation.
    pub shares_sold_for_taxes_scaled: Shares,
    /// `shares_vested − shares_sold`, scaled. The shares actually
    /// delivered to the user after the broker's sale.
    pub net_shares_delivered_scaled: Shares,
    /// `shares_sold × sell_price`, scaled. The cash the broker remits
    /// for withholding; `>= tax × gross` by the ceiling on
    /// `shares_sold`.
    pub cash_withheld_scaled: Shares,
}

/// Errors produced by [`compute`].
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone, Copy)]
pub enum ComputeError {
    /// `shares_vested − shares_sold_for_taxes` goes negative.
    /// Fires when `tax = 1` AND `sell_price < fmv`; the broker did
    /// not sell enough shares to cover nominal withholding. Per
    /// ADR-018 §6 decision-b + AC-6.4.2, v1 rejects this.
    #[error("shares_sold_for_taxes exceeds shares_vested (tax=100% with sell_price<fmv)")]
    NegativeNetShares,
    /// `share_sell_price == 0` with `tax > 0`. The DB CHECK (`> 0`)
    /// and the handler validator should pre-empt this, but the pure
    /// function guards against a direct caller passing zero.
    #[error("share_sell_price is zero with tax_withholding_percent > 0")]
    ZeroSellPriceWithPositiveTax,
}

// ---------------------------------------------------------------------------
// Pure function
// ---------------------------------------------------------------------------

/// Compute the sell-to-cover breakdown for one vesting event.
///
/// See the module-level documentation for the algorithm, rounding
/// direction, and overflow reasoning.
pub fn compute(input: SellToCoverInput) -> Result<SellToCoverResult, ComputeError> {
    let fmv = input.fmv_at_vest_scaled as i128;
    let shares = input.shares_vested_scaled as i128;
    let tax = input.tax_withholding_percent_scaled as i128;
    let sell_price = input.share_sell_price_scaled as i128;
    let scale = SHARES_SCALE as i128;

    // Edge: zero-vest short-circuit. All zeros regardless of tax /
    // FMV / sell_price (AC-6.4.4). This also side-steps the
    // zero-sell-price guard when there is nothing to tax.
    if shares == 0 {
        return Ok(SellToCoverResult {
            gross_amount_scaled: 0,
            shares_sold_for_taxes_scaled: 0,
            net_shares_delivered_scaled: 0,
            cash_withheld_scaled: 0,
        });
    }

    // Step 1 — gross = fmv × shares. Both operands are scaled by
    // `SHARES_SCALE`, so the product is scaled by `SHARES_SCALE^2`;
    // divide once to bring it back to a single-scale value.
    let gross = fmv.saturating_mul(shares) / scale;

    // Edge: zero tax short-circuit. No shares sold; user receives
    // every vested share verbatim; no cash withheld. Valid regardless
    // of sell_price (including zero — we don't need to divide).
    if tax == 0 {
        return Ok(SellToCoverResult {
            gross_amount_scaled: narrow_to_i64(gross),
            shares_sold_for_taxes_scaled: 0,
            net_shares_delivered_scaled: input.shares_vested_scaled,
            cash_withheld_scaled: 0,
        });
    }

    // Step 2 — defensive: zero sell price with positive tax would
    // mean dividing by zero. Handler + DB both prevent this but the
    // pure function must not panic.
    if sell_price == 0 {
        return Err(ComputeError::ZeroSellPriceWithPositiveTax);
    }

    // Step 3 — shares_sold_ideal = (tax × gross) / sell_price.
    // All three terms are `SHARES_SCALE`-scaled; the output must also
    // be `SHARES_SCALE`-scaled.
    //
    // Algebra (`S = SHARES_SCALE`):
    //   tax_scaled     = tax_frac × S
    //   gross_scaled   = gross    × S
    //   sell_scaled    = sell     × S
    //   ideal_scaled   = ideal    × S
    //   ideal          = (tax_frac × gross) / sell
    //                  = ((tax_scaled/S) × (gross_scaled/S)) / (sell_scaled/S)
    //                  = (tax_scaled × gross_scaled) / (sell_scaled × S)
    //   ideal_scaled   = (tax_scaled × gross_scaled × S) / (sell_scaled × S)
    //                  = (tax_scaled × gross_scaled)     / sell_scaled
    //
    // So `shares_sold_scaled_ideal = (tax × gross) / sell_price` with
    // all four operands in scaled-i128 space. Ceiling the quotient at
    // 4 dp is equivalent to ceiling-div on the scaled integer.
    let numerator = tax.saturating_mul(gross);
    let shares_sold_scaled = ceil_div_i128(numerator, sell_price);

    // Step 4 — net shares delivered.
    let net_scaled = shares - shares_sold_scaled;
    if net_scaled < 0 {
        return Err(ComputeError::NegativeNetShares);
    }

    // Step 5 — cash withheld = shares_sold × sell_price. Both scaled;
    // product is double-scaled; divide once.
    let cash_withheld = shares_sold_scaled.saturating_mul(sell_price) / scale;

    Ok(SellToCoverResult {
        gross_amount_scaled: narrow_to_i64(gross),
        shares_sold_for_taxes_scaled: narrow_to_i64(shares_sold_scaled),
        net_shares_delivered_scaled: narrow_to_i64(net_scaled),
        cash_withheld_scaled: narrow_to_i64(cash_withheld),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Integer ceiling division on non-negative `i128` values. Returns
/// `ceil(a / b)`; panics in debug if `b <= 0` (the caller has already
/// filtered zero sell_price — see `compute`; this is defense in
/// depth).
fn ceil_div_i128(a: i128, b: i128) -> i128 {
    debug_assert!(b > 0, "ceil_div_i128 requires a positive divisor");
    debug_assert!(a >= 0, "ceil_div_i128 expects non-negative dividend");
    // `(a + b - 1) / b` is the standard non-negative ceiling-div idiom.
    (a + b - 1) / b
}

/// Narrow an `i128` result back to `i64`, saturating at
/// `i64::MIN`/`i64::MAX` rather than panicking. Overflow is
/// unreachable under the DDL bounds (see module doc) but staying
/// explicit keeps the edge testable.
fn narrow_to_i64(v: i128) -> i64 {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        i64::MIN
    } else {
        v as i64
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vesting::whole_shares;

    /// Convenience: build an input from whole-number sugars. Percent is
    /// expressed as a fraction in `[0, 1]` in units of 1/SHARES_SCALE
    /// (so `0.4500` → `4500`).
    fn input(
        fmv_whole: i64,
        shares_whole: i64,
        tax_bp_of_unit: i64,
        sell_whole: i64,
    ) -> SellToCoverInput {
        SellToCoverInput {
            fmv_at_vest_scaled: whole_shares(fmv_whole),
            shares_vested_scaled: whole_shares(shares_whole),
            tax_withholding_percent_scaled: tax_bp_of_unit,
            share_sell_price_scaled: whole_shares(sell_whole),
        }
    }

    #[test]
    fn zero_tax_delivers_all_shares() {
        let r = compute(input(42, 100, 0, 42)).unwrap();
        assert_eq!(r.gross_amount_scaled, whole_shares(4200));
        assert_eq!(r.shares_sold_for_taxes_scaled, 0);
        assert_eq!(r.net_shares_delivered_scaled, whole_shares(100));
        assert_eq!(r.cash_withheld_scaled, 0);
    }

    #[test]
    fn full_tax_equal_prices_sells_everything() {
        // tax = 1.0000 (= SHARES_SCALE), sell = fmv → shares_sold =
        // shares_vested.
        let r = compute(input(42, 100, SHARES_SCALE, 42)).unwrap();
        assert_eq!(r.shares_sold_for_taxes_scaled, whole_shares(100));
        assert_eq!(r.net_shares_delivered_scaled, 0);
        assert_eq!(r.cash_withheld_scaled, whole_shares(4200));
    }

    #[test]
    fn full_tax_sell_below_fmv_rejects() {
        // tax = 1.0000, sell < fmv → shares_sold > shares_vested → error.
        let err = compute(input(100, 10, SHARES_SCALE, 40)).unwrap_err();
        assert_eq!(err, ComputeError::NegativeNetShares);
    }

    #[test]
    fn zero_shares_is_all_zeros() {
        let r = compute(input(42, 0, 4500, 42)).unwrap();
        assert_eq!(r.gross_amount_scaled, 0);
        assert_eq!(r.shares_sold_for_taxes_scaled, 0);
        assert_eq!(r.net_shares_delivered_scaled, 0);
        assert_eq!(r.cash_withheld_scaled, 0);
    }

    #[test]
    fn zero_sell_price_with_positive_tax_rejects() {
        let err = compute(input(42, 100, 4500, 0)).unwrap_err();
        assert_eq!(err, ComputeError::ZeroSellPriceWithPositiveTax);
    }

    #[test]
    fn zero_sell_price_with_zero_tax_is_ok() {
        // Defensive branch: zero tax short-circuits before the
        // zero-sell-price check fires.
        let r = compute(input(42, 100, 0, 0)).unwrap();
        assert_eq!(r.shares_sold_for_taxes_scaled, 0);
        assert_eq!(r.net_shares_delivered_scaled, whole_shares(100));
    }

    #[test]
    fn ceiling_rounds_up_on_residual() {
        // fmv = 100, shares = 1, tax = 0.3333, sell = 100.
        //   gross       = 100
        //   tax × gross = 33.33
        //   ideal sold  = 0.3333 (scaled: 3_333)
        // Ceiling should round 0.3333 to 0.3333 exactly (already at
        // 4dp). Verify no over-rounding.
        let r = compute(input(100, 1, 3_333, 100)).unwrap();
        assert_eq!(r.shares_sold_for_taxes_scaled, 3_333);
    }

    #[test]
    fn ceiling_rounds_up_below_tick() {
        // Construct inputs so the ideal falls exactly 1 scaled unit
        // above an integer-4dp tick:
        //   numerator = tax × gross = 1 × SHARES_SCALE = 10_000
        //   shares_sold_scaled = ceil(10_000 / 2) = 5_000.
        let r = compute(SellToCoverInput {
            fmv_at_vest_scaled: SHARES_SCALE,   // 1 scaled whole
            shares_vested_scaled: SHARES_SCALE, // 1 scaled whole
            tax_withholding_percent_scaled: 1,  // 1/10_000 fraction
            share_sell_price_scaled: 2,         // 2 units scaled
        })
        .unwrap();
        assert_eq!(r.shares_sold_for_taxes_scaled, 5_000);
    }

    #[test]
    fn balance_shares_always_sums() {
        // Across a representative sweep, shares_sold + net == vested.
        let cases = [
            (42, 100, 4500, 42),
            (42, 100, 3000, 45),
            (100, 50, 2500, 95),
        ];
        for (f, s, t, p) in cases {
            let r = compute(input(f, s, t, p)).unwrap();
            assert_eq!(
                r.shares_sold_for_taxes_scaled + r.net_shares_delivered_scaled,
                whole_shares(s),
                "balance failed for fmv={f} shares={s} tax={t} sell={p}",
            );
        }
    }

    #[test]
    fn cash_withheld_meets_or_exceeds_nominal() {
        // ceiling-on-shares implies cash_withheld >= tax × gross.
        let r = compute(input(42, 1000, 4500, 42)).unwrap();
        // Nominal = 0.45 × 42 × 1000 = 18_900 scaled = 189_000_000.
        let nominal = (4500_i128 * r.gross_amount_scaled as i128) / SHARES_SCALE as i128;
        assert!(r.cash_withheld_scaled as i128 >= nominal);
    }
}
