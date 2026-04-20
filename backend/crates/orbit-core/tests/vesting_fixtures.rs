//! Shared vesting-fixture parity test.
//!
//! Loads `tests/fixtures/vesting_cases.json` and runs
//! `derive_vesting_events` over every case, asserting the fixture's
//! `expected` block matches the computed events. The TS suite at
//! `frontend/src/lib/__tests__/vesting.fixtures.test.ts` consumes the same
//! file via `readFileSync`; any drift between the two implementations
//! (AC-4.3.5) fails both suites.
//!
//! This closes ADR-014 §Consequences "client/server drift-risk" mitigation.

use std::path::PathBuf;

use chrono::NaiveDate;
use orbit_core::{derive_vesting_events, Cadence, GrantInput, VestingState};
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
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct Inputs {
    #[serde(rename = "shareCountScaled")]
    share_count_scaled: i64,
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
struct Expected {
    #[serde(rename = "eventCount")]
    event_count: usize,
    #[serde(rename = "sumScaled")]
    sum_scaled: i64,
    #[serde(rename = "firstVestDate")]
    first_vest_date: String,
    #[serde(rename = "lastVestDate")]
    last_vest_date: String,
    #[serde(rename = "firstSharesScaled")]
    first_shares_scaled: Option<i64>,
    #[serde(rename = "lastCumulativeScaled")]
    last_cumulative_scaled: i64,
    #[serde(rename = "allStates")]
    all_states: Vec<String>,
    #[serde(rename = "eventDates", default)]
    event_dates: Option<Vec<String>>,
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

fn state_str(s: VestingState) -> &'static str {
    match s {
        VestingState::Upcoming => "upcoming",
        VestingState::TimeVestedAwaitingLiquidity => "time_vested_awaiting_liquidity",
        VestingState::Vested => "vested",
    }
}

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("vesting_cases.json")
}

fn load_fixtures() -> Fixtures {
    let raw = std::fs::read_to_string(fixtures_path()).expect("read fixtures");
    serde_json::from_str(&raw).expect("parse fixtures")
}

#[test]
fn fixture_file_is_present_and_parseable() {
    let f = load_fixtures();
    assert!(!f.cases.is_empty(), "at least one fixture case");
    assert!(
        f.cases.len() >= 10,
        "at least 10 canonical cases per T15 scope (got {})",
        f.cases.len(),
    );
}

#[test]
fn every_fixture_case_matches_derive_output() {
    let fixtures = load_fixtures();
    for case in &fixtures.cases {
        let input = GrantInput {
            share_count: case.inputs.share_count_scaled,
            vesting_start: parse_date(&case.inputs.vesting_start),
            vesting_total_months: case.inputs.vesting_total_months,
            cliff_months: case.inputs.cliff_months,
            cadence: parse_cadence(&case.inputs.cadence),
            double_trigger: case.inputs.double_trigger,
            liquidity_event_date: case.inputs.liquidity_event_date.as_deref().map(parse_date),
        };
        let today = parse_date(&case.today);
        let events = derive_vesting_events(&input, today)
            .unwrap_or_else(|e| panic!("case {}: derive error {:?}", case.name, e));

        // eventCount
        assert_eq!(
            events.len(),
            case.expected.event_count,
            "case {}: eventCount",
            case.name,
        );

        // sumScaled
        let sum: i64 = events.iter().map(|e| e.shares_vested_this_event).sum();
        assert_eq!(
            sum, case.expected.sum_scaled,
            "case {}: sumScaled",
            case.name,
        );

        // first / last vest dates
        let first = events.first().expect("at least one event");
        let last = events.last().expect("at least one event");
        assert_eq!(
            first.vest_date.format("%Y-%m-%d").to_string(),
            case.expected.first_vest_date,
            "case {}: firstVestDate",
            case.name,
        );
        assert_eq!(
            last.vest_date.format("%Y-%m-%d").to_string(),
            case.expected.last_vest_date,
            "case {}: lastVestDate",
            case.name,
        );

        // firstSharesScaled (optional)
        if let Some(expected_first) = case.expected.first_shares_scaled {
            assert_eq!(
                first.shares_vested_this_event, expected_first,
                "case {}: firstSharesScaled",
                case.name,
            );
        }

        // lastCumulativeScaled
        assert_eq!(
            last.cumulative_shares_vested, case.expected.last_cumulative_scaled,
            "case {}: lastCumulativeScaled",
            case.name,
        );

        // allStates: every event's state must be in the allowed set.
        for (i, e) in events.iter().enumerate() {
            let got = state_str(e.state);
            assert!(
                case.expected.all_states.iter().any(|s| s == got),
                "case {}: event {i} state {} not in {:?}",
                case.name,
                got,
                case.expected.all_states,
            );
        }

        // eventDates (optional): exact sequence match.
        if let Some(expected_dates) = &case.expected.event_dates {
            let actual: Vec<String> = events
                .iter()
                .map(|e| e.vest_date.format("%Y-%m-%d").to_string())
                .collect();
            assert_eq!(&actual, expected_dates, "case {}: eventDates", case.name,);
        }
    }
}
