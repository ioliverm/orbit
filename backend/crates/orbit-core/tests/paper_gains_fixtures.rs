//! Shared paper-gains fixture parity test (Slice 3 T29, ADR-017 §5).
//!
//! Loads `tests/fixtures/paper_gains_cases.json` and runs
//! `orbit_core::paper_gains::compute` over every case. The TS mirror
//! (frontend/src/lib/paperGains.ts — T30 scope) will consume the same
//! file to lock parity.
//!
//! Each case asserts:
//!
//!   * the set of `complete = true` grant ids matches `expected.completeIds`;
//!   * the `incompleteGrants` vec matches (order-insensitive);
//!   * `combinedEurBand.is_some()` matches `expected.hasCombinedBand`.
//!
//! Exact EUR amounts are unit-tested in `orbit_core::paper_gains::tests`;
//! the fixture's role is the cross-implementation-parity contract.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use chrono::NaiveDate;
use orbit_core::{
    compute_paper_gains, EsppPurchaseForPaperGains, GrantForPaperGains,
    GrantPriceOverrideForPaperGains, PaperGainsInput, Shares, TickerPriceForPaperGains,
    VestingEventForPaperGains, VestingState,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct Fixtures {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    today: String,
    #[serde(rename = "fxRateEurNative")]
    fx_rate_eur_native: Option<String>,
    /// T33 S4 — per-currency EUR/native map. Absent / empty in older
    /// single-USD cases so the legacy `fxRateEurNative` fallback drives
    /// conversion. New multi-currency cases populate this and leave the
    /// legacy rate as a stale decoy so a regression to "one rate fits
    /// all" fails the fixture.
    #[serde(rename = "fxRatesByCurrency", default)]
    fx_rates_by_currency: Option<std::collections::BTreeMap<String, Option<String>>>,
    grants: Vec<GrantJson>,
    #[serde(rename = "tickerPrices")]
    ticker_prices: Vec<TickerPriceJson>,
    #[serde(rename = "grantOverrides")]
    grant_overrides: Vec<GrantOverrideJson>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct GrantJson {
    id: Uuid,
    instrument: String,
    #[serde(rename = "nativeCurrency")]
    native_currency: String,
    ticker: Option<String>,
    #[serde(rename = "doubleTrigger")]
    double_trigger: bool,
    #[serde(rename = "liquidityEventDate")]
    liquidity_event_date: Option<String>,
    #[serde(rename = "vestingEvents")]
    vesting_events: Vec<VestingEventJson>,
    #[serde(rename = "esppPurchases")]
    espp_purchases: Vec<EsppPurchaseJson>,
}

#[derive(Debug, Deserialize)]
struct VestingEventJson {
    #[serde(rename = "vestDate")]
    vest_date: String,
    state: String,
    #[serde(rename = "sharesVestedScaled")]
    shares_vested_scaled: Shares,
    #[serde(rename = "fmvAtVest")]
    fmv_at_vest: Option<String>,
    #[serde(rename = "fmvCurrency")]
    fmv_currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EsppPurchaseJson {
    #[serde(rename = "purchaseDate")]
    purchase_date: String,
    #[serde(rename = "sharesPurchasedScaled")]
    shares_purchased_scaled: Shares,
    #[serde(rename = "fmvAtPurchase")]
    fmv_at_purchase: String,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct TickerPriceJson {
    ticker: String,
    price: String,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct GrantOverrideJson {
    #[serde(rename = "grantId")]
    grant_id: Uuid,
    price: String,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct Expected {
    #[serde(rename = "completeIds")]
    complete_ids: Vec<Uuid>,
    #[serde(rename = "incompleteGrants")]
    incomplete_grants: Vec<Uuid>,
    #[serde(rename = "hasCombinedBand")]
    has_combined_band: bool,
    /// T33 S1 — optional bitwise-parity numeric expectations. Present
    /// on the two fully-pinned cases (single-RSU-complete + mixed
    /// ESPP+RSU); absent cases still assert structural shape only.
    #[serde(rename = "expectedPerGrant", default)]
    expected_per_grant: std::collections::BTreeMap<Uuid, ExpectedPerGrant>,
    #[serde(rename = "expectedCombinedEurBand", default)]
    expected_combined_eur_band: Option<ExpectedBand>,
}

#[derive(Debug, Deserialize)]
struct ExpectedPerGrant {
    #[serde(rename = "gainNative")]
    gain_native: String,
    #[serde(rename = "gainEurBand")]
    gain_eur_band: ExpectedBand,
}

#[derive(Debug, Deserialize)]
struct ExpectedBand {
    low: String,
    mid: String,
    high: String,
}

fn parse_date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_else(|e| panic!("bad date {s}: {e}"))
}

fn parse_state(s: &str) -> VestingState {
    match s {
        "upcoming" => VestingState::Upcoming,
        "time_vested_awaiting_liquidity" => VestingState::TimeVestedAwaitingLiquidity,
        "vested" => VestingState::Vested,
        other => panic!("unknown state {other}"),
    }
}

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("paper_gains_cases.json")
}

fn load_fixtures() -> Fixtures {
    let raw = std::fs::read_to_string(fixtures_path()).expect("read fixtures");
    serde_json::from_str(&raw).expect("parse fixtures")
}

fn to_grant(g: &GrantJson) -> GrantForPaperGains {
    GrantForPaperGains {
        id: g.id,
        instrument: g.instrument.clone(),
        native_currency: g.native_currency.clone(),
        ticker: g.ticker.clone(),
        double_trigger: g.double_trigger,
        liquidity_event_date: g.liquidity_event_date.as_deref().map(parse_date),
        vesting_events: g
            .vesting_events
            .iter()
            .map(|e| VestingEventForPaperGains {
                vest_date: parse_date(&e.vest_date),
                state: parse_state(&e.state),
                shares_vested_this_event: e.shares_vested_scaled,
                fmv_at_vest: e.fmv_at_vest.clone(),
                fmv_currency: e.fmv_currency.clone(),
            })
            .collect(),
        espp_purchases: g
            .espp_purchases
            .iter()
            .map(|p| EsppPurchaseForPaperGains {
                purchase_date: parse_date(&p.purchase_date),
                shares_purchased: p.shares_purchased_scaled,
                fmv_at_purchase: p.fmv_at_purchase.clone(),
                currency: p.currency.clone(),
            })
            .collect(),
    }
}

#[test]
fn fixture_file_is_present_and_parseable() {
    let f = load_fixtures();
    assert!(!f.cases.is_empty(), "at least one paper-gains case");
    assert!(
        f.cases.len() >= 7,
        "at least 7 canonical paper-gains cases (got {})",
        f.cases.len(),
    );
}

#[test]
fn every_paper_gains_case_matches_compute_output() {
    let fixtures = load_fixtures();
    for case in &fixtures.cases {
        let grants: Vec<GrantForPaperGains> = case.grants.iter().map(to_grant).collect();
        let ticker_prices: Vec<TickerPriceForPaperGains> = case
            .ticker_prices
            .iter()
            .map(|t| TickerPriceForPaperGains {
                ticker: t.ticker.clone(),
                price: t.price.clone(),
                currency: t.currency.clone(),
            })
            .collect();
        let grant_overrides: Vec<GrantPriceOverrideForPaperGains> = case
            .grant_overrides
            .iter()
            .map(|o| GrantPriceOverrideForPaperGains {
                grant_id: o.grant_id,
                price: o.price.clone(),
                currency: o.currency.clone(),
            })
            .collect();

        let fx_rates_by_currency: BTreeMap<String, Option<String>> = case
            .fx_rates_by_currency
            .as_ref()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let input = PaperGainsInput {
            grants: &grants,
            ticker_prices: &ticker_prices,
            grant_overrides: &grant_overrides,
            fx_rate_eur_native: case.fx_rate_eur_native.clone(),
            fx_rates_by_currency,
            today: parse_date(&case.today),
        };

        let result = compute_paper_gains(&input);

        // Complete set
        let complete: HashSet<Uuid> = result
            .per_grant
            .iter()
            .filter(|p| p.complete)
            .map(|p| p.grant_id)
            .collect();
        let expected_complete: HashSet<Uuid> = case.expected.complete_ids.iter().copied().collect();
        assert_eq!(
            complete, expected_complete,
            "case {}: completeIds mismatch",
            case.name,
        );

        // Incomplete set (surface for the banner)
        let got_incomplete: HashSet<Uuid> = result.incomplete_grants.iter().copied().collect();
        let want_incomplete: HashSet<Uuid> =
            case.expected.incomplete_grants.iter().copied().collect();
        assert_eq!(
            got_incomplete, want_incomplete,
            "case {}: incompleteGrants mismatch",
            case.name,
        );

        // Combined band presence
        assert_eq!(
            result.combined_eur_band.is_some(),
            case.expected.has_combined_band,
            "case {}: hasCombinedBand",
            case.name,
        );

        // T33 S1 — bitwise-parity numeric assertions for cases that pin
        // them. Rust is the source of truth; the same file drives the
        // TS mirror in `frontend/src/lib/__tests__/paperGains.test.ts`.
        for (grant_id, expected) in &case.expected.expected_per_grant {
            let row = result
                .per_grant
                .iter()
                .find(|p| p.grant_id == *grant_id)
                .unwrap_or_else(|| {
                    panic!(
                        "case {}: expectedPerGrant entry {grant_id} has no matching row",
                        case.name
                    )
                });
            assert_eq!(
                row.gain_native.as_deref(),
                Some(expected.gain_native.as_str()),
                "case {}: gainNative mismatch for grant {grant_id}",
                case.name,
            );
            let band = row.gain_eur_band.as_ref().unwrap_or_else(|| {
                panic!(
                    "case {}: expectedPerGrant {grant_id} pins a gainEurBand but compute returned None",
                    case.name
                )
            });
            assert_eq!(
                band.low, expected.gain_eur_band.low,
                "case {}: gainEurBand.low mismatch for grant {grant_id}",
                case.name,
            );
            assert_eq!(
                band.mid, expected.gain_eur_band.mid,
                "case {}: gainEurBand.mid mismatch for grant {grant_id}",
                case.name,
            );
            assert_eq!(
                band.high, expected.gain_eur_band.high,
                "case {}: gainEurBand.high mismatch for grant {grant_id}",
                case.name,
            );
        }
        if let Some(expected_combined) = &case.expected.expected_combined_eur_band {
            let band = result.combined_eur_band.as_ref().unwrap_or_else(|| {
                panic!(
                    "case {}: expectedCombinedEurBand pinned but combined_eur_band is None",
                    case.name
                )
            });
            assert_eq!(
                band.low, expected_combined.low,
                "case {}: combinedEurBand.low mismatch",
                case.name,
            );
            assert_eq!(
                band.mid, expected_combined.mid,
                "case {}: combinedEurBand.mid mismatch",
                case.name,
            );
            assert_eq!(
                band.high, expected_combined.high,
                "case {}: combinedEurBand.high mismatch",
                case.name,
            );
        }
    }
}
