//! Property-based invariants for [`orbit_core::sell_to_cover::compute`]
//! (Slice 3b T38, ADR-018 §4).
//!
//! Eight invariants from ADR-018; each gets its own `proptest!` block so
//! a regression points at the offending property name:
//!
//!   1. **gross_exact** — `gross = fmv × shares` exactly (no rounding in
//!      the product alone).
//!   2. **balance_shares** — `net_shares + shares_sold = shares_vested`.
//!   3. **ceiling_never_undershoots** — `shares_sold ≥ tax × gross /
//!      sell_price` (ideal).
//!   4. **ceiling_residual_bounded** — ceiling(ideal) − ideal lies
//!      strictly in `[0, 1)` at 4-dp scale (< 10_000 scaled units of
//!      shares_sold less the ideal).
//!   5. **cash_meets_or_exceeds_nominal** — `cash_withheld ≥ tax ×
//!      gross` (ceiling monotonicity on shares_sold ⇒ cash never
//!      under-withholds).
//!   6. **zero_tax_short_circuits** — `tax = 0 ⇒ shares_sold = 0,
//!      net = vested, cash = 0`.
//!   7. **cash_equals_shares_times_sell_price** — the output
//!      `cash_withheld` is `shares_sold × sell_price` at full i128
//!      precision (verified by recomputation).
//!   8. **negative_net_triggers_only_on_excess** — if `compute` returns
//!      `NegativeNetShares`, then `tax × gross > shares_vested ×
//!      sell_price` (the condition that produces a negative delta).

use orbit_core::{
    compute_sell_to_cover, SellToCoverComputeError, SellToCoverInput, SellToCoverResult,
    SHARES_SCALE,
};
use proptest::prelude::*;

const SCALE: i128 = SHARES_SCALE as i128;

/// Generator for a legal (fmv, shares, tax, sell) tuple.
///
/// Bounds are chosen to exercise realistic ranges without risking i128
/// overflow inside the algorithm:
///   * fmv: 0..=10_000 whole units (scaled up to 4 dp).
///   * shares: 0..=1_000_000 whole units (scaled up to 4 dp).
///   * tax: 0..=SHARES_SCALE (full `[0, 1]` range).
///   * sell: 0..=10_000 whole units.
///
/// Max `fmv × shares` ≈ `1e8 × 1e10 = 1e18`, well inside `i128::MAX` ≈
/// `1.7e38`.
fn input_strategy() -> impl Strategy<Value = SellToCoverInput> {
    (
        0i64..=100_000_000i64,    // fmv_scaled (0..=10_000 whole)
        0i64..=10_000_000_000i64, // shares_scaled (0..=1_000_000 whole)
        0i64..=SHARES_SCALE,      // tax_scaled
        0i64..=100_000_000i64,    // sell_scaled
    )
        .prop_map(|(fmv, shares, tax, sell)| SellToCoverInput {
            fmv_at_vest_scaled: fmv,
            shares_vested_scaled: shares,
            tax_withholding_percent_scaled: tax,
            share_sell_price_scaled: sell,
        })
}

/// Same as [`input_strategy`] but forces `sell > 0` so the compute call
/// never produces `ZeroSellPriceWithPositiveTax` — used by the
/// invariants that need an `Ok` result.
fn nonzero_sell_input_strategy() -> impl Strategy<Value = SellToCoverInput> {
    (
        0i64..=100_000_000i64,
        0i64..=10_000_000_000i64,
        0i64..=SHARES_SCALE,
        1i64..=100_000_000i64,
    )
        .prop_map(|(fmv, shares, tax, sell)| SellToCoverInput {
            fmv_at_vest_scaled: fmv,
            shares_vested_scaled: shares,
            tax_withholding_percent_scaled: tax,
            share_sell_price_scaled: sell,
        })
}

fn gross_i128(i: &SellToCoverInput) -> i128 {
    (i.fmv_at_vest_scaled as i128).saturating_mul(i.shares_vested_scaled as i128) / SCALE
}

proptest! {
    /// Invariant 1 — `gross_amount = (fmv × shares) / SCALE` exactly.
    #[test]
    fn gross_is_fmv_times_shares(input in nonzero_sell_input_strategy()) {
        if let Ok(r) = compute_sell_to_cover(input) {
            let want = gross_i128(&input) as i64;
            prop_assert_eq!(r.gross_amount_scaled, want);
        }
    }

    /// Invariant 2 — `net + shares_sold = shares_vested`.
    #[test]
    fn balance_shares(input in nonzero_sell_input_strategy()) {
        if let Ok(SellToCoverResult {
            shares_sold_for_taxes_scaled,
            net_shares_delivered_scaled,
            ..
        }) = compute_sell_to_cover(input)
        {
            prop_assert_eq!(
                shares_sold_for_taxes_scaled + net_shares_delivered_scaled,
                input.shares_vested_scaled,
            );
        }
    }

    /// Invariant 3 — ceiling never undershoots:
    /// `shares_sold × sell_price >= tax × gross` (integer-scaled form).
    #[test]
    fn ceiling_never_undershoots(input in nonzero_sell_input_strategy()) {
        if let Ok(r) = compute_sell_to_cover(input) {
            let lhs = (r.shares_sold_for_taxes_scaled as i128)
                .saturating_mul(input.share_sell_price_scaled as i128);
            let rhs = (input.tax_withholding_percent_scaled as i128)
                .saturating_mul(gross_i128(&input));
            prop_assert!(lhs >= rhs, "lhs={lhs} rhs={rhs}");
        }
    }

    /// Invariant 4 — ceiling residual bounded: the ceiling output is at
    /// most one scaled-unit above the ideal-scaled integer quotient.
    /// `ceil(a/b) × b − a ∈ [0, b)`.
    #[test]
    fn ceiling_residual_bounded(input in nonzero_sell_input_strategy()) {
        if let Ok(r) = compute_sell_to_cover(input) {
            let a = (input.tax_withholding_percent_scaled as i128)
                .saturating_mul(gross_i128(&input));
            let b = input.share_sell_price_scaled as i128;
            let c = r.shares_sold_for_taxes_scaled as i128;
            let residual = c.saturating_mul(b) - a;
            prop_assert!(residual >= 0);
            prop_assert!(residual < b, "residual={residual} sell_price={b}");
        }
    }

    /// Invariant 5 — cash_withheld ≥ tax × gross (ceiling direction
    /// guarantees no under-withholding).
    #[test]
    fn cash_meets_or_exceeds_nominal(input in nonzero_sell_input_strategy()) {
        if let Ok(r) = compute_sell_to_cover(input) {
            // nominal = (tax × gross) / SCALE at full precision.
            let nominal = (input.tax_withholding_percent_scaled as i128)
                .saturating_mul(gross_i128(&input))
                / SCALE;
            prop_assert!(
                r.cash_withheld_scaled as i128 >= nominal,
                "cash={} nominal={nominal}",
                r.cash_withheld_scaled,
            );
        }
    }

    /// Invariant 6 — zero tax yields zero sold, all-delivered, no cash.
    #[test]
    fn zero_tax_short_circuits(
        fmv in 0i64..=100_000_000i64,
        shares in 0i64..=10_000_000_000i64,
        sell in 0i64..=100_000_000i64,
    ) {
        let input = SellToCoverInput {
            fmv_at_vest_scaled: fmv,
            shares_vested_scaled: shares,
            tax_withholding_percent_scaled: 0,
            share_sell_price_scaled: sell,
        };
        let r = compute_sell_to_cover(input).expect("tax=0 never errors");
        prop_assert_eq!(r.shares_sold_for_taxes_scaled, 0);
        prop_assert_eq!(r.net_shares_delivered_scaled, shares);
        prop_assert_eq!(r.cash_withheld_scaled, 0);
    }

    /// Invariant 7 — cash_withheld = (shares_sold × sell_price) / SCALE
    /// at full precision.
    #[test]
    fn cash_equals_shares_times_sell_price(input in nonzero_sell_input_strategy()) {
        if let Ok(r) = compute_sell_to_cover(input) {
            let want = ((r.shares_sold_for_taxes_scaled as i128)
                .saturating_mul(input.share_sell_price_scaled as i128))
                / SCALE;
            prop_assert_eq!(r.cash_withheld_scaled as i128, want);
        }
    }

    /// Invariant 8 — negative-net rejection condition: if compute
    /// returns `NegativeNetShares`, then `tax × gross > shares_vested ×
    /// sell_price`. Recompute both sides in i128 to prove the
    /// equivalence.
    #[test]
    fn negative_net_triggers_only_on_excess(input in input_strategy()) {
        if let Err(SellToCoverComputeError::NegativeNetShares) = compute_sell_to_cover(input) {
            let lhs = (input.tax_withholding_percent_scaled as i128)
                .saturating_mul(gross_i128(&input));
            let rhs = (input.shares_vested_scaled as i128)
                .saturating_mul(input.share_sell_price_scaled as i128);
            prop_assert!(
                lhs > rhs,
                "NegativeNetShares fired but tax×gross ({lhs}) <= shares×sell ({rhs})",
            );
        }
    }
}
