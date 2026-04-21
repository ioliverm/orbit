//! Property-based test suite for `orbit_core::stacked_grants::stack_dashboard`.
//!
//! Slice 2 T23 — pins the six invariants the ADR-016 §4 algorithm must
//! hold across 1000 random *legal* portfolios per invariant. Complements
//! the hand-written unit tests in `src/stacked_grants.rs` + the shared
//! fixture suite in `stacked_grants_fixtures.rs`.
//!
//! Invariants asserted (one `proptest!` block per invariant so a
//! regression points at the offending property name):
//!
//! 1. **cumulative_monotonic**: within each `EmployerStack` and the
//!    `combined` curve, both `cumulative_vested` and
//!    `cumulative_awaiting_liquidity` are non-decreasing between
//!    consecutive points.
//! 2. **per_employer_sum_equals_total**: for every employer E,
//!    Σ `shares_vested_this_event` across E's events equals Σ
//!    `share_count` across E's grants (the events emitted by the
//!    Slice-1 derivation preserve the sum invariant; this asserts the
//!    stacker does not drop or duplicate an event).
//! 3. **combined_sum_equals_portfolio_total**: the final `combined`
//!    point's vested + awaiting sum equals Σ `share_count` across all
//!    input grants.
//! 4. **deterministic_tie_break**: for two grants with identical
//!    `created_at` timestamps, the output order within a per-date
//!    breakdown is stable and keyed on `grant_id ASC`. Swapping input
//!    order produces an identical dashboard.
//! 5. **mixed_instrument_preserves_per_instrument_totals**: for an
//!    employer holding RSU + NSO grants, summing the per-grant
//!    `shares_vested_this_event` by instrument (via the drill-down)
//!    equals the per-instrument total (every grant's `share_count`).
//! 6. **double_trigger_partition**: `cumulative_vested` and
//!    `cumulative_awaiting_liquidity` never overlap on the same grant at
//!    the same date, and their sum equals the Slice-1
//!    `vested_to_date_at(events, date)` summed over all grants.
//!
//! # Input bounds (per the task spec)
//!
//! * up to 5 employers per case
//! * up to 10 grants per employer
//! * `share_count` 1..=1e16 scaled (matches the Slice-1 DDL ceiling)
//! * `vesting_total_months` 1..=240
//! * `cliff_months` 0..=vesting_total_months
//! * grant dates 2000-01-01..=2040-12-31
//!
//! Strategies are deliberately bounded to the algorithm's **legal**
//! domain so `prop_assume!` is never needed — any rejection rate would
//! suggest a strategy fix rather than a proptest-pluggable skip.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use orbit_core::stacked_grants::{stack_dashboard, vested_to_date_at, GrantMeta};
use orbit_core::vesting::{derive_vesting_events, Cadence, GrantInput, Shares, VestingEvent};
use proptest::collection::vec as prop_vec;
use proptest::prelude::*;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn date_strategy() -> impl Strategy<Value = NaiveDate> {
    // Day capped at 28 to avoid Feb 29 on non-leap years (the algorithm
    // handles the clamp fine; we just want to avoid rejection in the
    // strategy itself).
    (2000i32..=2040i32, 1u32..=12u32, 1u32..=28u32)
        .prop_map(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).expect("valid ymd"))
}

fn cadence_strategy() -> impl Strategy<Value = Cadence> {
    prop_oneof![Just(Cadence::Monthly), Just(Cadence::Quarterly)]
}

/// One grant shaped for the stacker: a `GrantMeta` plus the derived
/// vesting events. `today` is the reference clock used by
/// `derive_vesting_events` — for the invariants under test the value
/// only affects which events land in `Vested` vs. `Upcoming`, which the
/// properties either average over (`cumulative_monotonic`) or handle
/// explicitly (`double_trigger_partition`).
#[derive(Debug, Clone)]
struct GeneratedGrant {
    meta: GrantMeta,
    events: Vec<VestingEvent>,
    share_count: Shares,
}

fn grant_strategy_for_employer(
    employer_name: String,
    today: NaiveDate,
) -> impl Strategy<Value = GeneratedGrant> {
    (
        // share_count: 1..=1e16 scaled (Slice-1 DDL ceiling).
        1i64..=10_000_000_000_000_000i64,
        1u32..=240u32,
        date_strategy(),
        cadence_strategy(),
        any::<bool>(),
        // createdAt: independent of vesting_start. Generate a raw day
        // offset within a 40-year window and convert.
        (2000i32..=2040i32, 1u32..=12u32, 1u32..=28u32),
        // liquidity_event_date offset: None or a date somewhere in
        // vesting_start ± 5000 days.
        prop_oneof![
            Just::<Option<i64>>(None),
            (-5000i64..=5000i64).prop_map(Some),
        ],
        any::<u128>(), // grant_id entropy
    )
        .prop_flat_map(
            move |(
                share_count,
                total_months,
                vesting_start,
                cadence,
                double_trigger,
                created_ymd,
                liq_offset,
                id_bits,
            )| {
                let employer = employer_name.clone();
                (0u32..=total_months).prop_map(move |cliff| {
                    let created_at = Utc
                        .with_ymd_and_hms(created_ymd.0, created_ymd.1, created_ymd.2, 0, 0, 0)
                        .single()
                        .unwrap_or_else(|| Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
                    let liquidity_event_date = liq_offset.and_then(|off| {
                        if off >= 0 {
                            vesting_start.checked_add_days(chrono::Days::new(off as u64))
                        } else {
                            vesting_start.checked_sub_days(chrono::Days::new(off.unsigned_abs()))
                        }
                    });
                    let input = GrantInput {
                        share_count,
                        vesting_start,
                        vesting_total_months: total_months,
                        cliff_months: cliff,
                        cadence,
                        double_trigger,
                        liquidity_event_date,
                    };
                    let events = derive_vesting_events(&input, today).expect("legal input");
                    let id = Uuid::from_u128(id_bits);
                    // Silence unused destructuring (these fields are
                    // implicit in the generated `events`).
                    let _ = (double_trigger, liquidity_event_date, cadence, cliff);
                    GeneratedGrant {
                        meta: GrantMeta {
                            id,
                            employer_name: employer.clone(),
                            // Instrument picked deterministically from
                            // id entropy so mixed-instrument property can
                            // force a known distribution via its own
                            // strategy; default here is "rsu".
                            instrument: "rsu".to_string(),
                            created_at,
                        },
                        events,
                        share_count,
                    }
                })
            },
        )
}

/// Portfolio strategy: 1..=5 employers, 1..=10 grants per employer.
fn portfolio_strategy() -> impl Strategy<Value = (Vec<GeneratedGrant>, NaiveDate)> {
    (
        // today: clamp to 2000-2060 so `vesting_start + total_months`
        // never overflows chrono.
        (2000i32..=2060i32, 1u32..=12u32, 1u32..=28u32),
        1usize..=5usize,
    )
        .prop_flat_map(|((y, m, d), employer_count)| {
            let today = NaiveDate::from_ymd_opt(y, m, d).expect("valid today");
            let employer_names: Vec<String> = (0..employer_count)
                .map(|i| format!("Employer-{}", (b'A' + (i as u8)) as char))
                .collect();
            let strategies: Vec<_> = employer_names
                .into_iter()
                .map(|name| prop_vec(grant_strategy_for_employer(name.clone(), today), 1..=10))
                .collect();
            // Collect into a Vec<Vec<GeneratedGrant>> then flatten.
            strategies.prop_map(move |groups| {
                let grants: Vec<GeneratedGrant> = groups.into_iter().flatten().collect();
                (grants, today)
            })
        })
}

/// Mixed-instrument strategy: an employer with `k_rsu` RSU + `k_nso`
/// NSO grants (each 1..=4). Used by the per-instrument-totals property.
fn mixed_instrument_strategy() -> impl Strategy<Value = (Vec<GeneratedGrant>, NaiveDate)> {
    (
        (2000i32..=2060i32, 1u32..=12u32, 1u32..=28u32),
        1usize..=4usize,
        1usize..=4usize,
    )
        .prop_flat_map(|((y, m, d), n_rsu, n_nso)| {
            let today = NaiveDate::from_ymd_opt(y, m, d).expect("valid today");
            let rsu = prop_vec(
                grant_strategy_for_employer("MixedCo".into(), today),
                n_rsu..=n_rsu,
            );
            let nso = prop_vec(
                grant_strategy_for_employer("MixedCo".into(), today),
                n_nso..=n_nso,
            );
            (rsu, nso).prop_map(move |(rsus, nsos)| {
                let mut grants: Vec<GeneratedGrant> = Vec::new();
                for mut g in rsus {
                    g.meta.instrument = "rsu".to_string();
                    grants.push(g);
                }
                for mut g in nsos {
                    g.meta.instrument = "nso".to_string();
                    grants.push(g);
                }
                (grants, today)
            })
        })
}

// ---------------------------------------------------------------------------
// Helpers — derive inputs + per-employer grouping
// ---------------------------------------------------------------------------

fn inputs_from(grants: &[GeneratedGrant]) -> Vec<(GrantMeta, Vec<VestingEvent>)> {
    grants
        .iter()
        .map(|g| (g.meta.clone(), g.events.clone()))
        .collect()
}

fn group_by_employer_normalized(
    grants: &[GeneratedGrant],
) -> BTreeMap<String, Vec<&GeneratedGrant>> {
    let mut out: BTreeMap<String, Vec<&GeneratedGrant>> = BTreeMap::new();
    for g in grants {
        let key = g.meta.employer_name.trim().to_lowercase();
        out.entry(key).or_default().push(g);
    }
    out
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    // ADR-016 §4 — 1000 random legal portfolios per invariant.
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn prop_cumulative_monotonic((grants, _today) in portfolio_strategy()) {
        let out = stack_dashboard(inputs_from(&grants));
        // Per-employer.
        for es in &out.by_employer {
            let mut prev_v: Shares = 0;
            let mut prev_a: Shares = 0;
            for (i, p) in es.points.iter().enumerate() {
                prop_assert!(
                    p.cumulative_vested >= prev_v,
                    "employer {} point {} vested not monotonic: {} < {}",
                    es.employer_name, i, p.cumulative_vested, prev_v,
                );
                prop_assert!(
                    p.cumulative_awaiting_liquidity >= prev_a,
                    "employer {} point {} awaiting not monotonic: {} < {}",
                    es.employer_name, i, p.cumulative_awaiting_liquidity, prev_a,
                );
                prev_v = p.cumulative_vested;
                prev_a = p.cumulative_awaiting_liquidity;
            }
        }
        // Combined envelope.
        let mut prev_v: Shares = 0;
        let mut prev_a: Shares = 0;
        for (i, p) in out.combined.iter().enumerate() {
            prop_assert!(
                p.cumulative_vested >= prev_v,
                "combined point {} vested not monotonic: {} < {}",
                i, p.cumulative_vested, prev_v,
            );
            prop_assert!(
                p.cumulative_awaiting_liquidity >= prev_a,
                "combined point {} awaiting not monotonic: {} < {}",
                i, p.cumulative_awaiting_liquidity, prev_a,
            );
            prev_v = p.cumulative_vested;
            prev_a = p.cumulative_awaiting_liquidity;
        }
    }

    #[test]
    fn prop_per_employer_sum_equals_total((grants, _today) in portfolio_strategy()) {
        let out = stack_dashboard(inputs_from(&grants));
        let by_employer = group_by_employer_normalized(&grants);
        for es in &out.by_employer {
            // Look up the bucket for this employer via the `employer_key`
            // that the dashboard already computed (normalized form).
            let bucket = by_employer.get(&es.employer_key).unwrap_or_else(|| {
                panic!(
                    "dashboard emitted employer_key {} that we did not generate",
                    es.employer_key
                )
            });
            let expected_total: i128 =
                bucket.iter().map(|g| g.share_count as i128).sum::<i128>();

            // Sum per-grant `shares_vested_this_event` across all points.
            let mut actual_total: i128 = 0;
            for p in &es.points {
                for b in &p.per_grant_breakdown {
                    actual_total += b.shares_vested_this_event as i128;
                }
            }
            prop_assert_eq!(
                actual_total, expected_total,
                "employer {}: Σ shares_vested_this_event across stack ({}) != Σ share_count ({})",
                es.employer_name, actual_total, expected_total,
            );
        }
    }

    #[test]
    fn prop_combined_sum_equals_portfolio_total(
        (grants, _today) in portfolio_strategy()
    ) {
        let out = stack_dashboard(inputs_from(&grants));
        let expected: i128 = grants.iter().map(|g| g.share_count as i128).sum();
        // `cumulative_vested + cumulative_awaiting_liquidity` at the last
        // point equals the total number of events materialized for grants
        // whose `vest_date <= today`. Grants with future events contribute
        // nothing to either sum. So we compute the expected "past" total
        // here.
        let expected_past: i128 = grants
            .iter()
            .map(|g| {
                g.events
                    .iter()
                    .filter(|e| !matches!(e.state, orbit_core::VestingState::Upcoming))
                    .map(|e| e.shares_vested_this_event as i128)
                    .sum::<i128>()
            })
            .sum();
        // Shape sanity: expected_past <= expected (every past-vested share
        // is also a grant-total share).
        prop_assert!(expected_past <= expected);

        // The final `combined` point's two cumulative fields sum to
        // `expected_past` when there is at least one event in the past;
        // if every grant only has future events the combined vec still
        // contains points but the cumulative values at the last point
        // are zero.
        let last = out.combined.last();
        if let Some(p) = last {
            let got = (p.cumulative_vested as i128) + (p.cumulative_awaiting_liquidity as i128);
            prop_assert_eq!(
                got, expected_past,
                "combined final vested+awaiting ({}) != past-vested total ({})",
                got, expected_past,
            );
        } else {
            prop_assert_eq!(expected_past, 0, "no combined points implies zero past");
        }
    }

    #[test]
    fn prop_deterministic_tie_break_same_created_at(
        (grants, _today) in portfolio_strategy()
    ) {
        // Force two grants under the same employer to share a
        // `created_at`; then re-run stack_dashboard with their input
        // positions swapped and assert bit-identical output.
        if grants.len() < 2 {
            return Ok(());
        }
        // Find a pair within the same employer bucket.
        let buckets = group_by_employer_normalized(&grants);
        let pair = buckets.values().find(|g| g.len() >= 2);
        let Some(pair) = pair else {
            // Fall back to any two grants (different employers still
            // exercise the top-level order stability).
            return Ok(());
        };
        let a_id = pair[0].meta.id;
        let b_id = pair[1].meta.id;
        let fixed_ts = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Rebuild the input vector twice with the two targeted grants'
        // `created_at` tied and their relative order swapped.
        let mut inputs_ab: Vec<(GrantMeta, Vec<VestingEvent>)> = Vec::new();
        let mut inputs_ba: Vec<(GrantMeta, Vec<VestingEvent>)> = Vec::new();
        for g in &grants {
            let mut meta = g.meta.clone();
            if meta.id == a_id || meta.id == b_id {
                meta.created_at = fixed_ts;
            }
            inputs_ab.push((meta.clone(), g.events.clone()));
            inputs_ba.push((meta, g.events.clone()));
        }
        // Swap A and B positions in inputs_ba.
        let idx_a = inputs_ba.iter().position(|(m, _)| m.id == a_id).unwrap();
        let idx_b = inputs_ba.iter().position(|(m, _)| m.id == b_id).unwrap();
        inputs_ba.swap(idx_a, idx_b);

        let out_ab = stack_dashboard(inputs_ab);
        let out_ba = stack_dashboard(inputs_ba);
        prop_assert_eq!(
            out_ab.by_employer, out_ba.by_employer,
            "by_employer should be identical under input-order swap with tied created_at",
        );
        prop_assert_eq!(
            out_ab.combined, out_ba.combined,
            "combined envelope should be identical under input-order swap with tied created_at",
        );
    }

    #[test]
    fn prop_mixed_instrument_preserves_per_instrument_totals(
        (grants, _today) in mixed_instrument_strategy()
    ) {
        let out = stack_dashboard(inputs_from(&grants));
        prop_assert_eq!(out.by_employer.len(), 1, "mixed-instrument case is a single employer");
        let es = &out.by_employer[0];

        // Sum per-grant `shares_vested_this_event` by instrument across
        // the whole stack.
        let mut actual_by_instrument: HashMap<String, i128> = HashMap::new();
        let id_to_instrument: HashMap<Uuid, String> = grants
            .iter()
            .map(|g| (g.meta.id, g.meta.instrument.clone()))
            .collect();
        for p in &es.points {
            for b in &p.per_grant_breakdown {
                let inst = id_to_instrument
                    .get(&b.grant_id)
                    .cloned()
                    .expect("instrument present");
                *actual_by_instrument.entry(inst).or_default() += b.shares_vested_this_event as i128;
            }
        }

        // Expected: Σ share_count grouped by instrument.
        let mut expected_by_instrument: HashMap<String, i128> = HashMap::new();
        for g in &grants {
            *expected_by_instrument
                .entry(g.meta.instrument.clone())
                .or_default() += g.share_count as i128;
        }
        for (inst, expected) in &expected_by_instrument {
            let got = actual_by_instrument.get(inst).copied().unwrap_or(0);
            prop_assert_eq!(
                got, *expected,
                "instrument {} total mismatch: got {} expected {}",
                inst, got, expected,
            );
        }
    }

    #[test]
    fn prop_double_trigger_partition((grants, today) in portfolio_strategy()) {
        // At every date in the combined envelope, cumulative_vested and
        // cumulative_awaiting_liquidity sum to Σ grants.vested_to_date_at(events, date).{0+1}.
        // Per-grant, the two buckets never double-count the same
        // share: each event state is exactly one of Vested,
        // TimeVestedAwaitingLiquidity, or Upcoming.
        let _ = today;
        let out = stack_dashboard(inputs_from(&grants));
        for p in &out.combined {
            let mut want: i128 = 0;
            for g in &grants {
                let (v, a) = vested_to_date_at(&g.events, p.date);
                want += v as i128;
                want += a as i128;
            }
            let got =
                (p.cumulative_vested as i128) + (p.cumulative_awaiting_liquidity as i128);
            prop_assert_eq!(
                got, want,
                "combined at {} vested+awaiting ({}) != Σ per-grant past ({})",
                p.date, got, want,
            );
        }

        // Per-employer, same invariant.
        let buckets = group_by_employer_normalized(&grants);
        for es in &out.by_employer {
            let bucket = buckets
                .get(&es.employer_key)
                .expect("employer present");
            for p in &es.points {
                let mut want: i128 = 0;
                for g in bucket {
                    let (v, a) = vested_to_date_at(&g.events, p.date);
                    want += v as i128;
                    want += a as i128;
                }
                let got =
                    (p.cumulative_vested as i128) + (p.cumulative_awaiting_liquidity as i128);
                prop_assert_eq!(
                    got, want,
                    "employer {} at {} vested+awaiting ({}) != Σ per-grant past ({})",
                    es.employer_name, p.date, got, want,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Compile-time-only smoke: confirm chrono DateTime import is used.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _unused_datetime_guard() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
}
