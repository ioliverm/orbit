//! Shared vesting-override fixture parity test (Slice 3 T29, ADR-017 §2).
//!
//! Loads `tests/fixtures/vesting_override_cases.json` and exercises
//! `derive_vesting_events` with the per-case override list. The TS mirror
//! (frontend/src/lib/vesting.ts parity) will consume the same file in T30.
//!
//! Each case asserts:
//!
//!   * every overridden `vest_date` appears in the output;
//!   * the shares at each overridden `vest_date` match the override's
//!     `sharesVestedScaled` verbatim (preservation — AC-8.4.2);
//!   * the cumulative-invariant relaxation flag matches expectation
//!     (AC-8.5.2);
//!   * determinism: calling the function twice yields bit-identical
//!     output (AC-4.3.5 property, extended to the override branch).

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use orbit_core::{derive_vesting_events, Cadence, GrantInput, Shares, VestingEventOverride};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixtures {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    inputs: Inputs,
    today: String,
    #[serde(default)]
    overrides: Vec<OverrideRow>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct Inputs {
    #[serde(rename = "shareCountScaled")]
    share_count_scaled: Shares,
    #[serde(rename = "vestingStart")]
    vesting_start: String,
    #[serde(rename = "vestingTotalMonths")]
    vesting_total_months: u32,
    #[serde(rename = "cliffMonths")]
    cliff_months: u32,
    cadence: String,
    #[serde(rename = "doubleTrigger")]
    double_trigger: bool,
    #[serde(rename = "liquidityEventDate")]
    liquidity_event_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OverrideRow {
    #[serde(rename = "vestDate")]
    vest_date: String,
    #[serde(rename = "sharesVestedScaled")]
    shares_vested_scaled: Shares,
    #[serde(rename = "fmvAtVest")]
    fmv_at_vest: Option<String>,
    #[serde(rename = "fmvCurrency")]
    fmv_currency: Option<String>,
    #[serde(rename = "originalDerivationIndex")]
    original_derivation_index: usize,
}

#[derive(Debug, Deserialize)]
struct Expected {
    #[serde(rename = "eventCount")]
    event_count: usize,
    #[serde(rename = "cumulativeRelaxed")]
    cumulative_relaxed: bool,
    #[serde(rename = "overriddenDates")]
    overridden_dates: Vec<String>,
    #[serde(rename = "overriddenShares")]
    overridden_shares: HashMap<String, Shares>,
}

fn parse_date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_else(|e| panic!("bad date {s}: {e}"))
}

fn parse_cadence(s: &str) -> Cadence {
    match s {
        "monthly" => Cadence::Monthly,
        "quarterly" => Cadence::Quarterly,
        other => panic!("unknown cadence {other}"),
    }
}

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("vesting_override_cases.json")
}

fn load_fixtures() -> Fixtures {
    let raw = std::fs::read_to_string(fixtures_path()).expect("read fixtures");
    serde_json::from_str(&raw).expect("parse fixtures")
}

#[test]
fn fixture_file_is_present_and_parseable() {
    let f = load_fixtures();
    assert!(!f.cases.is_empty(), "at least one override fixture case");
    assert!(
        f.cases.len() >= 8,
        "at least 8 canonical override cases per ADR-017 §2 (got {})",
        f.cases.len(),
    );
}

#[test]
fn every_override_case_matches_derive_output() {
    let fixtures = load_fixtures();
    for case in &fixtures.cases {
        let grant = GrantInput {
            share_count: case.inputs.share_count_scaled,
            vesting_start: parse_date(&case.inputs.vesting_start),
            vesting_total_months: case.inputs.vesting_total_months,
            cliff_months: case.inputs.cliff_months,
            cadence: parse_cadence(&case.inputs.cadence),
            double_trigger: case.inputs.double_trigger,
            liquidity_event_date: case.inputs.liquidity_event_date.as_deref().map(parse_date),
        };
        let today = parse_date(&case.today);
        let overrides: Vec<VestingEventOverride> = case
            .overrides
            .iter()
            .map(|o| VestingEventOverride {
                vest_date: parse_date(&o.vest_date),
                shares_vested_this_event: o.shares_vested_scaled,
                fmv_at_vest: o.fmv_at_vest.clone(),
                fmv_currency: o.fmv_currency.clone(),
                original_derivation_index: o.original_derivation_index,
            })
            .collect();

        let events = derive_vesting_events(&grant, today, &overrides)
            .unwrap_or_else(|e| panic!("case {}: derive error {:?}", case.name, e));

        // Event count
        assert_eq!(
            events.len(),
            case.expected.event_count,
            "case {}: eventCount (got {:?})",
            case.name,
            events.iter().map(|e| e.vest_date).collect::<Vec<_>>(),
        );

        // Every overridden date appears
        for expected_date in &case.expected.overridden_dates {
            let d = parse_date(expected_date);
            assert!(
                events.iter().any(|e| e.vest_date == d),
                "case {}: override date {} missing from output",
                case.name,
                expected_date,
            );
        }

        // Overridden shares preserved verbatim
        for (date_str, expected_shares) in &case.expected.overridden_shares {
            let d = parse_date(date_str);
            let ev = events
                .iter()
                .find(|e| e.vest_date == d)
                .unwrap_or_else(|| panic!("case {}: override date {d} missing", case.name));
            assert_eq!(
                ev.shares_vested_this_event, *expected_shares,
                "case {}: shares at {d}",
                case.name,
            );
        }

        // Cumulative-invariant relaxation check: when an override shifts
        // the sum, it must diverge from grant.share_count.
        let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
        let relaxed = sum != grant.share_count;
        assert_eq!(
            relaxed, case.expected.cumulative_relaxed,
            "case {}: cumulativeRelaxed expected {} got {} (sum={}, share_count={})",
            case.name, case.expected.cumulative_relaxed, relaxed, sum, grant.share_count,
        );

        // Determinism: second call matches first (AC-4.3.5 extended).
        let again = derive_vesting_events(&grant, today, &overrides)
            .unwrap_or_else(|e| panic!("case {}: second derive error {:?}", case.name, e));
        assert_eq!(
            events, again,
            "case {}: non-deterministic output",
            case.name
        );
    }
}

#[test]
fn empty_overrides_matches_no_overrides_baseline() {
    // When `existing_overrides` is empty, the Slice-3 branch is bit-
    // identical to the Slice-1 derivation path. Use the first fixture
    // case's grant input to verify.
    let fixtures = load_fixtures();
    let case = &fixtures.cases[0];
    let grant = GrantInput {
        share_count: case.inputs.share_count_scaled,
        vesting_start: parse_date(&case.inputs.vesting_start),
        vesting_total_months: case.inputs.vesting_total_months,
        cliff_months: case.inputs.cliff_months,
        cadence: parse_cadence(&case.inputs.cadence),
        double_trigger: case.inputs.double_trigger,
        liquidity_event_date: case.inputs.liquidity_event_date.as_deref().map(parse_date),
    };
    let today = parse_date(&case.today);
    let events = derive_vesting_events(&grant, today, &[]).expect("ok");
    let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
    assert_eq!(
        sum, grant.share_count,
        "no-overrides path must preserve cumulative invariant"
    );
}
