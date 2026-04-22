//! Shared sell-to-cover fixture parity test (Slice 3b T38, ADR-018 §4).
//!
//! Loads `tests/fixtures/sell_to_cover_cases.json` and runs
//! [`orbit_core::compute_sell_to_cover`] over every case, asserting
//! bitwise equality on every scaled-i64 output field OR the exact
//! error variant the case expects. The TS parity mirror
//! (`frontend/src/lib/__tests__/sellToCover.spec.ts`, T39 scope)
//! consumes the same fixture file to lock parity at the bit level.

use std::path::PathBuf;

use orbit_core::{
    compute_sell_to_cover, SellToCoverComputeError, SellToCoverInput, SellToCoverResult,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixtures {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    input: Input,
    #[serde(default)]
    expected: Option<Expected>,
    #[serde(rename = "expectedError", default)]
    expected_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    fmv_at_vest_scaled: i64,
    shares_vested_scaled: i64,
    tax_withholding_percent_scaled: i64,
    share_sell_price_scaled: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    gross_amount_scaled: i64,
    shares_sold_for_taxes_scaled: i64,
    net_shares_delivered_scaled: i64,
    cash_withheld_scaled: i64,
}

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sell_to_cover_cases.json")
}

fn load_fixtures() -> Fixtures {
    let raw = std::fs::read_to_string(fixtures_path()).expect("read fixtures");
    serde_json::from_str(&raw).expect("parse fixtures")
}

fn to_input(i: &Input) -> SellToCoverInput {
    SellToCoverInput {
        fmv_at_vest_scaled: i.fmv_at_vest_scaled,
        shares_vested_scaled: i.shares_vested_scaled,
        tax_withholding_percent_scaled: i.tax_withholding_percent_scaled,
        share_sell_price_scaled: i.share_sell_price_scaled,
    }
}

fn to_expected(e: &Expected) -> SellToCoverResult {
    SellToCoverResult {
        gross_amount_scaled: e.gross_amount_scaled,
        shares_sold_for_taxes_scaled: e.shares_sold_for_taxes_scaled,
        net_shares_delivered_scaled: e.net_shares_delivered_scaled,
        cash_withheld_scaled: e.cash_withheld_scaled,
    }
}

fn parse_error(s: &str) -> SellToCoverComputeError {
    match s {
        "NegativeNetShares" => SellToCoverComputeError::NegativeNetShares,
        "ZeroSellPriceWithPositiveTax" => SellToCoverComputeError::ZeroSellPriceWithPositiveTax,
        other => panic!("unknown expectedError: {other}"),
    }
}

#[test]
fn fixture_file_is_present_and_parseable() {
    let f = load_fixtures();
    assert_eq!(
        f.cases.len(),
        12,
        "Slice 3b AC pins 12 canonical sell-to-cover cases; got {}",
        f.cases.len(),
    );
}

#[test]
fn every_sell_to_cover_case_matches_compute_output() {
    let fixtures = load_fixtures();
    for case in &fixtures.cases {
        let input = to_input(&case.input);
        let got = compute_sell_to_cover(input);
        match (&case.expected, &case.expected_error, got) {
            (Some(expected), None, Ok(result)) => {
                let want = to_expected(expected);
                assert_eq!(result, want, "case {}: compute result mismatch", case.name,);
            }
            (None, Some(err), Err(got_err)) => {
                let want_err = parse_error(err);
                assert_eq!(
                    got_err, want_err,
                    "case {}: compute error mismatch",
                    case.name,
                );
            }
            (Some(_), None, Err(e)) => {
                panic!("case {}: expected Ok, got Err({e:?})", case.name);
            }
            (None, Some(_), Ok(r)) => {
                panic!("case {}: expected Err, got Ok({r:?})", case.name);
            }
            _ => panic!(
                "case {}: fixture must specify exactly one of `expected` or `expectedError`",
                case.name,
            ),
        }
    }
}

#[test]
fn every_case_name_is_unique() {
    let fixtures = load_fixtures();
    let mut seen = std::collections::BTreeSet::new();
    for case in &fixtures.cases {
        assert!(
            seen.insert(case.name.clone()),
            "duplicate case name: {}",
            case.name,
        );
    }
}
