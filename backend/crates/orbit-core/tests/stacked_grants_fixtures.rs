//! Shared stacked-dashboard fixture parity test (Slice 2 T21, ADR-016 §4).
//!
//! Loads `tests/fixtures/stacked_grants_cases.json` and exercises
//! `stack_dashboard` against every case, asserting the fixture's
//! `expected` block matches the computed output. The TS parity mirror
//! that T22 will land at `frontend/src/lib/__tests__/stackedGrants.fixtures.test.ts`
//! will consume the same file via `readFileSync`; any drift between the
//! two implementations (AC-8.2.8) fails both suites.
//!
//! This extends ADR-014's vesting-fixture discipline to the Slice-2
//! multi-grant surface — same pattern, new file.

use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use orbit_core::stacked_grants::{stack_dashboard, GrantMeta};
use orbit_core::vesting::{derive_vesting_events, Cadence, GrantInput};
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
    grants: Vec<GrantFixture>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct GrantFixture {
    id: Uuid,
    #[serde(rename = "employerName")]
    employer_name: String,
    instrument: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    vesting: VestingFixture,
}

#[derive(Debug, Deserialize)]
struct VestingFixture {
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
    #[serde(rename = "employerCount")]
    employer_count: usize,
    #[serde(rename = "employerNames")]
    employer_names: Vec<String>,
    #[serde(rename = "employerGrantIdCounts")]
    employer_grant_id_counts: Vec<usize>,
    #[serde(rename = "combinedPointCount")]
    combined_point_count: usize,
    #[serde(rename = "finalCumulativeVestedScaled")]
    final_cumulative_vested_scaled: i64,
    #[serde(rename = "finalCumulativeAwaitingScaled")]
    final_cumulative_awaiting_scaled: i64,
    #[serde(rename = "instrumentsPresentInBreakdown", default)]
    instruments_present: Option<Vec<String>>,
}

fn parse_date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_else(|e| panic!("bad date {s}: {e}"))
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .unwrap_or_else(|e| panic!("bad timestamp {s}: {e}"))
        .with_timezone(&Utc)
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
        .join("stacked_grants_cases.json")
}

fn load_fixtures() -> Fixtures {
    let raw = std::fs::read_to_string(fixtures_path()).expect("read fixtures");
    serde_json::from_str(&raw).expect("parse fixtures")
}

#[test]
fn fixture_file_is_present_and_parseable() {
    let f = load_fixtures();
    assert!(
        f.cases.len() >= 6,
        "expected ≥6 canonical cases per ADR-016"
    );
}

#[test]
fn every_case_matches_dashboard_output() {
    let fixtures = load_fixtures();
    for case in &fixtures.cases {
        let today = parse_date(&case.today);
        let inputs: Vec<(GrantMeta, Vec<orbit_core::vesting::VestingEvent>)> = case
            .grants
            .iter()
            .map(|g| {
                let vi = GrantInput {
                    share_count: g.vesting.share_count_scaled,
                    vesting_start: parse_date(&g.vesting.vesting_start),
                    vesting_total_months: g.vesting.vesting_total_months,
                    cliff_months: g.vesting.cliff_months,
                    cadence: parse_cadence(&g.vesting.cadence),
                    double_trigger: g.vesting.double_trigger,
                    liquidity_event_date: g.vesting.liquidity_event_date.as_deref().map(parse_date),
                };
                let events = derive_vesting_events(&vi, today, &[]).unwrap_or_else(|e| {
                    panic!(
                        "case {}: grant {} vesting derive failed: {:?}",
                        case.name, g.id, e
                    )
                });
                let meta = GrantMeta {
                    id: g.id,
                    employer_name: g.employer_name.clone(),
                    instrument: g.instrument.clone(),
                    created_at: parse_ts(&g.created_at),
                };
                (meta, events)
            })
            .collect();

        let out = stack_dashboard(inputs);

        assert_eq!(
            out.by_employer.len(),
            case.expected.employer_count,
            "case {}: employerCount",
            case.name
        );
        let names: Vec<_> = out
            .by_employer
            .iter()
            .map(|e| e.employer_name.clone())
            .collect();
        assert_eq!(
            names, case.expected.employer_names,
            "case {}: employerNames",
            case.name
        );
        let grant_id_counts: Vec<usize> =
            out.by_employer.iter().map(|e| e.grant_ids.len()).collect();
        assert_eq!(
            grant_id_counts, case.expected.employer_grant_id_counts,
            "case {}: employerGrantIdCounts",
            case.name
        );
        assert_eq!(
            out.combined.len(),
            case.expected.combined_point_count,
            "case {}: combinedPointCount",
            case.name
        );

        let last = out.combined.last().expect("at least one combined point");
        assert_eq!(
            last.cumulative_vested, case.expected.final_cumulative_vested_scaled,
            "case {}: finalCumulativeVestedScaled",
            case.name
        );
        assert_eq!(
            last.cumulative_awaiting_liquidity, case.expected.final_cumulative_awaiting_scaled,
            "case {}: finalCumulativeAwaitingScaled",
            case.name
        );

        if let Some(expected_instruments) = &case.expected.instruments_present {
            let seen: std::collections::BTreeSet<String> = out
                .by_employer
                .iter()
                .flat_map(|e| e.points.iter())
                .flat_map(|p| p.per_grant_breakdown.iter().map(|b| b.instrument.clone()))
                .collect();
            for want in expected_instruments {
                assert!(
                    seen.contains(want),
                    "case {}: instrument {} missing from breakdown",
                    case.name,
                    want
                );
            }
        }
    }
}
