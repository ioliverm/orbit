//! Property-based test suite for `orbit_core::paper_gains::compute`.
//!
//! Pins the AC-5.x invariants from ADR-017 §5. Each block owns a single
//! invariant so a regression fails the named property, not a generic
//! "compute" test:
//!
//!   1. **complete_grants_produce_band** — when every past vest event
//!      carries FMV, `incomplete_grants` is empty and `combined_eur_band`
//!      is `Some(_)` (unless FX is `None`, handled in its own case).
//!   2. **missing_fmv_surfaces_incomplete** — if any past vest carries
//!      `fmv_at_vest = None`, the grant appears in `incomplete_grants`.
//!   3. **zero_gain_when_price_equals_fmv** — setting `current_price` to
//!      match every past vest's FMV yields `gain_native = 0.0000`.
//!   4. **envelope_ordering** — `low <= mid <= high` for every
//!      per-grant band and for the combined band.
//!   5. **nso_exclusion** — NSO / ISO grants never appear in `per_grant`
//!      as complete, never in `incomplete_grants`.
//!   6. **espp_inclusion** — ESPP grants with at least one purchase and
//!      a resolved current price surface as complete and contribute to
//!      the combined band.

use chrono::NaiveDate;
use orbit_core::{
    compute_paper_gains, EsppPurchaseForPaperGains, EurBand, GrantForPaperGains, MissingReason,
    PaperGainsInput, Shares, TickerPriceForPaperGains, VestingEventForPaperGains, VestingState,
    SHARES_SCALE,
};
use proptest::prelude::*;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn dec_strategy() -> impl Strategy<Value = String> {
    // Plain price strings the paper-gains f64 parser handles cleanly.
    (1u32..=10_000u32, 0u32..=9_999u32).prop_map(|(w, f)| format!("{w}.{f:04}"))
}

fn price_strategy() -> impl Strategy<Value = String> {
    dec_strategy()
}

fn date_strategy() -> impl Strategy<Value = NaiveDate> {
    (2020i32..=2025i32, 1u32..=12u32, 1u32..=28u32)
        .prop_map(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).expect("valid ymd"))
}

fn shares_strategy() -> impl Strategy<Value = Shares> {
    (1i64..=10_000_000i64).prop_map(|whole| whole.saturating_mul(SHARES_SCALE))
}

fn vesting_event_strategy(
    today: NaiveDate,
    fmv_always_some: bool,
) -> impl Strategy<Value = VestingEventForPaperGains> {
    (
        date_strategy(),
        shares_strategy(),
        dec_strategy(),
        any::<bool>(),
    )
        .prop_map(move |(d, s, fmv, has_fmv)| {
            // Cap dates at `today` so we land on a *past* vest event.
            let vest_date = if d > today { today } else { d };
            VestingEventForPaperGains {
                vest_date,
                state: VestingState::Vested,
                shares_vested_this_event: s,
                fmv_at_vest: if fmv_always_some || has_fmv {
                    Some(fmv)
                } else {
                    None
                },
                fmv_currency: Some("USD".into()),
            }
        })
}

fn complete_rsu_strategy(today: NaiveDate) -> impl Strategy<Value = GrantForPaperGains> {
    (proptest::collection::vec(
        vesting_event_strategy(today, true),
        1..=5,
    ),)
        .prop_map(|(events,)| GrantForPaperGains {
            id: Uuid::new_v4(),
            instrument: "rsu".into(),
            native_currency: "USD".into(),
            ticker: Some("ACME".into()),
            double_trigger: false,
            liquidity_event_date: None,
            vesting_events: events,
            espp_purchases: Vec::new(),
        })
}

fn incomplete_rsu_strategy(today: NaiveDate) -> impl Strategy<Value = GrantForPaperGains> {
    (
        // Guarantee at least one event with FMV = None by pinning the
        // first element, and pad with any-FMV rows.
        vesting_event_strategy(today, false),
        proptest::collection::vec(vesting_event_strategy(today, false), 0..=4),
    )
        .prop_map(move |(seed, rest)| {
            let mut events = Vec::with_capacity(1 + rest.len());
            let mut seed = seed;
            seed.fmv_at_vest = None;
            seed.fmv_currency = None;
            events.push(seed);
            events.extend(rest);
            GrantForPaperGains {
                id: Uuid::new_v4(),
                instrument: "rsu".into(),
                native_currency: "USD".into(),
                ticker: Some("ACME".into()),
                double_trigger: false,
                liquidity_event_date: None,
                vesting_events: events,
                espp_purchases: Vec::new(),
            }
        })
}

fn nso_strategy(today: NaiveDate) -> impl Strategy<Value = GrantForPaperGains> {
    // NSO never surfaces; events are allowed but irrelevant.
    proptest::collection::vec(vesting_event_strategy(today, true), 0..=3).prop_map(|events| {
        GrantForPaperGains {
            id: Uuid::new_v4(),
            instrument: "nso".into(),
            native_currency: "USD".into(),
            ticker: Some("ACME".into()),
            double_trigger: false,
            liquidity_event_date: None,
            vesting_events: events,
            espp_purchases: Vec::new(),
        }
    })
}

fn espp_strategy(today: NaiveDate) -> impl Strategy<Value = GrantForPaperGains> {
    (proptest::collection::vec(
        (date_strategy(), shares_strategy(), dec_strategy()).prop_map(move |(d, s, fmv)| {
            let purchase_date = if d > today { today } else { d };
            EsppPurchaseForPaperGains {
                purchase_date,
                shares_purchased: s,
                fmv_at_purchase: fmv,
                currency: "USD".into(),
            }
        }),
        1..=3,
    ),)
        .prop_map(|(purchases,)| GrantForPaperGains {
            id: Uuid::new_v4(),
            instrument: "espp".into(),
            native_currency: "USD".into(),
            ticker: Some("ACME".into()),
            double_trigger: false,
            liquidity_event_date: None,
            vesting_events: Vec::new(),
            espp_purchases: purchases,
        })
}

const TODAY: NaiveDate = match NaiveDate::from_ymd_opt(2026, 4, 19) {
    Some(d) => d,
    None => panic!("literal TODAY invalid"),
};

fn acme_price(price: &str) -> Vec<TickerPriceForPaperGains> {
    vec![TickerPriceForPaperGains {
        ticker: "ACME".into(),
        price: price.into(),
        currency: "USD".into(),
    }]
}

fn parse_eur(s: &str) -> f64 {
    s.parse().expect("band strings are always numeric")
}

fn band_ordered(b: &EurBand) {
    let l = parse_eur(&b.low);
    let m = parse_eur(&b.mid);
    let h = parse_eur(&b.high);
    assert!(l <= m + 1e-9, "EurBand low={l} > mid={m}");
    assert!(m <= h + 1e-9, "EurBand mid={m} > high={h}");
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// 1. All-FMV-present grants produce a band and do NOT appear in
    ///    incomplete_grants.
    #[test]
    fn prop_complete_grants_produce_band(
        g in complete_rsu_strategy(TODAY),
        price in price_strategy(),
    ) {
        let prices = acme_price(&price);
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".into()),
            today: TODAY,
        };
        let result = compute_paper_gains(&input);
        prop_assert_eq!(result.incomplete_grants.len(), 0);
        prop_assert!(result.combined_eur_band.is_some());
        prop_assert!(result.per_grant[0].complete);
    }

    /// 2. At least one null-FMV past vest → grant lands in
    ///    `incomplete_grants` with `FmvMissing`.
    #[test]
    fn prop_missing_fmv_surfaces_incomplete(
        g in incomplete_rsu_strategy(TODAY),
        price in price_strategy(),
    ) {
        let prices = acme_price(&price);
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".into()),
            today: TODAY,
        };
        let result = compute_paper_gains(&input);
        prop_assert!(
            result.incomplete_grants.contains(&g.id),
            "missing-FMV grant must surface in incomplete_grants",
        );
        prop_assert_eq!(
            result.per_grant[0].missing_reason,
            Some(MissingReason::FmvMissing),
        );
    }

    /// 3. When price matches each event's FMV verbatim, native gain is 0.
    #[test]
    fn prop_zero_gain_when_price_equals_fmv(
        shares in shares_strategy(),
        fmv in dec_strategy(),
    ) {
        let id = Uuid::new_v4();
        let g = GrantForPaperGains {
            id,
            instrument: "rsu".into(),
            native_currency: "USD".into(),
            ticker: Some("ACME".into()),
            double_trigger: false,
            liquidity_event_date: None,
            vesting_events: vec![VestingEventForPaperGains {
                vest_date: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
                state: VestingState::Vested,
                shares_vested_this_event: shares,
                fmv_at_vest: Some(fmv.clone()),
                fmv_currency: Some("USD".into()),
            }],
            espp_purchases: Vec::new(),
        };
        let prices = acme_price(&fmv);
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("1.0".into()),
            today: TODAY,
        };
        let result = compute_paper_gains(&input);
        prop_assert_eq!(result.per_grant[0].complete, true);
        // Allow at most half a cent of float drift (per-row accumulation).
        let gain: f64 = result.per_grant[0]
            .gain_native
            .as_deref()
            .expect("complete → gain_native")
            .parse()
            .expect("numeric");
        prop_assert!(gain.abs() < 0.005, "gain={gain} should be ~0");
    }

    /// 4. Envelope ordering `low <= mid <= high` for every complete
    ///    grant's band and for the combined band, under the
    ///    non-negative-gain regime (price >= fmv). The envelope is a
    ///    retail-vs-wholesale spread — the ordering semantically
    ///    flips on negative gains (a 3 % spread makes a loss worse),
    ///    which is the documented intent; asserting non-negative-gain
    ///    ordering is both the spec invariant and exercises the
    ///    happy-path the UI relies on. A negative-gain ordering
    ///    regression would show up in the unit tests that pin exact
    ///    EUR amounts.
    #[test]
    fn prop_envelope_ordering(
        shares in shares_strategy(),
        fmv_dec in 1u32..=500u32,
        price_bump in 0u32..=5_000u32,
        fx in (5u32..=200u32).prop_map(|n| format!("0.{n:04}")),
    ) {
        // price >= fmv guarantees non-negative native gain.
        let fmv = format!("{fmv_dec}.0000");
        let price = format!("{}.0000", fmv_dec + price_bump);
        let g = GrantForPaperGains {
            id: Uuid::new_v4(),
            instrument: "rsu".into(),
            native_currency: "USD".into(),
            ticker: Some("ACME".into()),
            double_trigger: false,
            liquidity_event_date: None,
            vesting_events: vec![VestingEventForPaperGains {
                vest_date: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
                state: VestingState::Vested,
                shares_vested_this_event: shares,
                fmv_at_vest: Some(fmv),
                fmv_currency: Some("USD".into()),
            }],
            espp_purchases: Vec::new(),
        };
        let prices = acme_price(&price);
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some(fx),
            today: TODAY,
        };
        let result = compute_paper_gains(&input);
        for row in &result.per_grant {
            if let Some(band) = &row.gain_eur_band {
                band_ordered(band);
            }
        }
        if let Some(band) = &result.combined_eur_band {
            band_ordered(band);
        }
    }

    /// 5. NSO grants are excluded — per_grant has `NsoDeferred` + not in
    ///    incomplete_grants.
    #[test]
    fn prop_nso_exclusion(g in nso_strategy(TODAY), price in price_strategy()) {
        let prices = acme_price(&price);
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".into()),
            today: TODAY,
        };
        let result = compute_paper_gains(&input);
        prop_assert_eq!(result.per_grant.len(), 1);
        prop_assert_eq!(result.per_grant[0].complete, false);
        prop_assert_eq!(
            result.per_grant[0].missing_reason,
            Some(MissingReason::NsoDeferred),
        );
        prop_assert!(
            !result.incomplete_grants.contains(&g.id),
            "NSO grants must NOT surface in incomplete_grants (AC-5.4.3)",
        );
    }

    /// 6. ESPP with purchases + a resolvable price yields a complete row
    ///    and the basis comes from `espp_purchases.fmv_at_purchase` per
    ///    AC-5.4.2.
    #[test]
    fn prop_espp_inclusion(g in espp_strategy(TODAY), price in price_strategy()) {
        let prices = acme_price(&price);
        let input = PaperGainsInput {
            grants: std::slice::from_ref(&g),
            ticker_prices: &prices,
            grant_overrides: &[],
            fx_rate_eur_native: Some("0.90".into()),
            today: TODAY,
        };
        let result = compute_paper_gains(&input);
        prop_assert_eq!(result.per_grant[0].complete, true);
        prop_assert!(
            !result.incomplete_grants.contains(&g.id),
            "ESPP grants with purchases must not surface in incomplete_grants",
        );
        // Basis-of-record: a zero-purchases ESPP would be skipped from
        // the combined band (handled in the dedicated integration
        // probe). Since this strategy guarantees >=1 purchase, we have
        // a combined band.
        prop_assert!(result.combined_eur_band.is_some());
    }
}
