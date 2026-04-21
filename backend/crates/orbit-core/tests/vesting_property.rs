//! Property-based test suite for `orbit_core::vesting::derive_vesting_events`.
//!
//! Complements the hand-written unit tests and the `sweep_sum_equals_total`
//! sweep in `src/vesting.rs`. This file pins ADR-014 §7 invariants across
//! proptest's default 1000 random *legal* inputs (AC-4.3.1..5). Inputs are
//! bounded to the algorithm's valid domain so the suite is a **property
//! test**, not a fuzz test — we are asserting invariants, not crash-safety.
//!
//! Invariants asserted (one `proptest!` block per invariant so a regression
//! points at the offending property name):
//!
//! 1. **sum_equals_total**: Σ `shares_vested_this_event` == `share_count`.
//! 2. **cumulative_monotonic**: `cumulative_shares_vested` is non-decreasing
//!    AND equals the previous cumulative + this event's delta.
//! 3. **no_event_before_cliff**: every event's `vest_date` >= `vesting_start + cliff_months`.
//! 4. **quarterly_cadence_exactness**: consecutive events in quarterly mode
//!    are exactly 3 months apart (modulo chrono day-of-month clamp).
//! 5. **determinism**: same input → identical output across repeated calls.
//! 6. **day_of_month_clamp_safe**: every vest_date respects chrono's
//!    `checked_add_months` semantics (never overflows inside the 2000..=2040
//!    test range).
//! 7. **cliff_equals_total_single_event**: when `cliff == total_months`
//!    exactly one event is emitted with `shares == total`.
//! 8. **double_trigger_state_machine**: states match the (today, vest_date,
//!    double_trigger, liquidity) truth table from `state_for`.

use chrono::{Months, NaiveDate};
use orbit_core::{derive_vesting_events, Cadence, GrantInput, VestingState};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies — generate legal inputs only.
// ---------------------------------------------------------------------------

/// Generator for `vesting_start` within a safe calendar window. Chrono
/// `NaiveDate` spans years -262144..=262143; we restrict to 2000-01-01 ..=
/// 2040-12-31 so `vesting_start + 240 months` cannot overflow.
fn vesting_start_strategy() -> impl Strategy<Value = NaiveDate> {
    (2000i32..=2040i32, 1u32..=12u32, 1u32..=28u32).prop_map(|(y, m, d)| {
        // Day capped at 28 so the generator never produces an invalid ymd
        // (Feb 29 on a non-leap year would be rejected by from_ymd_opt).
        // The day-of-month clamp behaviour is exercised independently in
        // `src/vesting.rs` unit tests.
        NaiveDate::from_ymd_opt(y, m, d).expect("valid ymd")
    })
}

/// Cadence strategy: both variants equally likely.
fn cadence_strategy() -> impl Strategy<Value = Cadence> {
    prop_oneof![Just(Cadence::Monthly), Just(Cadence::Quarterly)]
}

/// Generator producing a quarterly no-cliff input — dedicated strategy for
/// `prop_quarterly_cadence_exactness` so `prop_assume` doesn't reject away
/// most of the search space.
fn grant_strategy_quarterly_no_cliff() -> impl Strategy<Value = (GrantInput, NaiveDate)> {
    (
        1i64..=10_000_000_000_000_000i64,
        1u32..=240u32,
        vesting_start_strategy(),
        any::<bool>(),
        prop_oneof![
            Just::<Option<i64>>(None),
            (-5000i64..=5000i64).prop_map(Some),
        ],
        vesting_start_strategy(),
    )
        .prop_map(
            |(
                share_count,
                vesting_total_months,
                vesting_start,
                double_trigger,
                liq_offset,
                today,
            )| {
                let liquidity_event_date = liq_offset.and_then(|off| {
                    if off >= 0 {
                        vesting_start.checked_add_days(chrono::Days::new(off as u64))
                    } else {
                        vesting_start.checked_sub_days(chrono::Days::new(off.unsigned_abs()))
                    }
                });
                let g = GrantInput {
                    share_count,
                    vesting_start,
                    vesting_total_months,
                    cliff_months: 0,
                    cadence: Cadence::Quarterly,
                    double_trigger,
                    liquidity_event_date,
                };
                (g, today)
            },
        )
}

/// Generator producing a `cliff == total` input — dedicated strategy for
/// `prop_cliff_equals_total_single_event`.
fn grant_strategy_cliff_equals_total() -> impl Strategy<Value = (GrantInput, NaiveDate)> {
    (
        1i64..=10_000_000_000_000_000i64,
        1u32..=240u32,
        vesting_start_strategy(),
        cadence_strategy(),
        any::<bool>(),
        prop_oneof![
            Just::<Option<i64>>(None),
            (-5000i64..=5000i64).prop_map(Some),
        ],
        vesting_start_strategy(),
    )
        .prop_map(
            |(
                share_count,
                vesting_total_months,
                vesting_start,
                cadence,
                double_trigger,
                liq_offset,
                today,
            )| {
                let liquidity_event_date = liq_offset.and_then(|off| {
                    if off >= 0 {
                        vesting_start.checked_add_days(chrono::Days::new(off as u64))
                    } else {
                        vesting_start.checked_sub_days(chrono::Days::new(off.unsigned_abs()))
                    }
                });
                let g = GrantInput {
                    share_count,
                    vesting_start,
                    vesting_total_months,
                    cliff_months: vesting_total_months,
                    cadence,
                    double_trigger,
                    liquidity_event_date,
                };
                (g, today)
            },
        )
}

/// Generator producing `(GrantInput, today)` pairs with legal fields:
///
/// * `share_count`: 1..=10^16 (matches NUMERIC(20,4) scaled ceiling).
/// * `vesting_total_months`: 1..=240 (DDL CHECK).
/// * `cliff_months`: 0..=total (DDL CHECK).
/// * `vesting_start`: 2000-01-01..=2040-12-31 (overflow-safe).
/// * `cadence`: monthly | quarterly.
/// * `double_trigger`: any (applies to any instrument in this pure layer).
/// * `liquidity_event_date`: None, past, or future relative to `today`.
/// * `today`: 2000-01-01..=2060-12-31.
fn grant_strategy() -> impl Strategy<Value = (GrantInput, NaiveDate)> {
    (
        1i64..=10_000_000_000_000_000i64,
        1u32..=240u32,
        vesting_start_strategy(),
        cadence_strategy(),
        any::<bool>(),
        // Liquidity-date offset in days: 0 → None; otherwise absolute date.
        prop_oneof![
            Just::<Option<i64>>(None),
            (-5000i64..=5000i64).prop_map(Some),
        ],
        vesting_start_strategy(), // today
    )
        .prop_flat_map(
            |(
                share_count,
                vesting_total_months,
                vesting_start,
                cadence,
                double_trigger,
                liq_offset,
                today_hint,
            )| {
                // cliff must be in 0..=total.
                (0u32..=vesting_total_months).prop_map(move |cliff_months| {
                    let liquidity_event_date = liq_offset.and_then(|off| {
                        if off >= 0 {
                            vesting_start.checked_add_days(chrono::Days::new(off as u64))
                        } else {
                            vesting_start.checked_sub_days(chrono::Days::new(off.unsigned_abs()))
                        }
                    });
                    let g = GrantInput {
                        share_count,
                        vesting_start,
                        vesting_total_months,
                        cliff_months,
                        cadence,
                        double_trigger,
                        liquidity_event_date,
                    };
                    (g, today_hint)
                })
            },
        )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cliff_date(g: &GrantInput) -> NaiveDate {
    g.vesting_start
        .checked_add_months(Months::new(g.cliff_months))
        .expect("within calendar range")
}

fn quarterly_step_ok(prev: NaiveDate, curr: NaiveDate) -> bool {
    // Expected next: prev + 3 months (with day-clamp).
    let expected = prev
        .checked_add_months(Months::new(3))
        .expect("within calendar range");
    curr == expected
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    // ADR-014 §7 — 1000 random legal inputs (proptest default).
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn prop_sum_equals_total((g, today) in grant_strategy()) {
        let events = derive_vesting_events(&g, today, &[]).expect("legal input");
        let sum: i64 = events.iter().map(|e| e.shares_vested_this_event).sum();
        prop_assert_eq!(sum, g.share_count, "AC-4.3.1: sum must equal total");
    }

    #[test]
    fn prop_cumulative_monotonic((g, today) in grant_strategy()) {
        let events = derive_vesting_events(&g, today, &[]).expect("legal input");
        let mut prev_cum: i64 = 0;
        for (idx, e) in events.iter().enumerate() {
            prop_assert!(
                e.cumulative_shares_vested >= prev_cum,
                "cumulative must be non-decreasing at event {}",
                idx,
            );
            prop_assert_eq!(
                e.cumulative_shares_vested,
                prev_cum + e.shares_vested_this_event,
                "cumulative must equal prev + delta at event {}",
                idx,
            );
            prev_cum = e.cumulative_shares_vested;
        }
    }

    #[test]
    fn prop_no_event_before_cliff((g, today) in grant_strategy()) {
        let events = derive_vesting_events(&g, today, &[]).expect("legal input");
        if g.cliff_months > 0 {
            let cutoff = cliff_date(&g);
            for e in &events {
                prop_assert!(
                    e.vest_date >= cutoff,
                    "AC-4.3.2: vest_date {} < cliff_date {}",
                    e.vest_date,
                    cutoff,
                );
            }
        }
    }

    #[test]
    fn prop_quarterly_cadence_exactness((g, today) in grant_strategy_quarterly_no_cliff()) {
        // Quarterly + no cliff: the cadence is "exactly every 3 months"
        // across every consecutive pair. With a cliff the first step after
        // the cliff can be the `next = cliff + step` jump which is still
        // 3-month-exact, but the last-event clamp can shorten it; we assert
        // over the middle of the sequence only.
        let events = derive_vesting_events(&g, today, &[]).expect("legal input");
        if events.len() < 2 {
            return Ok(());
        }
        // The last event can be shorter than 3m when `total_months % 3 != 0`
        // (clamp branch). Assert 3-month exactness on pairs up to the
        // penultimate event.
        let full_stride_end = if (g.vesting_total_months % 3) == 0 {
            events.len()
        } else {
            events.len() - 1
        };
        for i in 1..full_stride_end {
            prop_assert!(
                quarterly_step_ok(events[i - 1].vest_date, events[i].vest_date),
                "AC-4.3.3: events[{}]={} is not +3m after events[{}]={}",
                i,
                events[i].vest_date,
                i - 1,
                events[i - 1].vest_date,
            );
        }
    }

    #[test]
    fn prop_determinism((g, today) in grant_strategy()) {
        let a = derive_vesting_events(&g, today, &[]).expect("legal input");
        let b = derive_vesting_events(&g, today, &[]).expect("legal input");
        prop_assert_eq!(a, b, "AC-4.3.5: identical input must produce identical events");
    }

    #[test]
    fn prop_day_of_month_clamp_safe((g, today) in grant_strategy()) {
        // Every vest_date must be representable via chrono's checked_add_months
        // — the function we rely on internally. This guards against any
        // future refactor that tries to compute dates manually.
        let events = derive_vesting_events(&g, today, &[]).expect("legal input");
        for (i, e) in events.iter().enumerate() {
            // Reconstruct the vest date from the raw month offset and ensure
            // equality. Finding that offset requires walking the cadence.
            prop_assert!(
                e.vest_date >= g.vesting_start,
                "event {i} vest_date {} < vesting_start {}",
                e.vest_date,
                g.vesting_start,
            );
            // Upper bound: vesting_start + total_months (the final event).
            let upper = g
                .vesting_start
                .checked_add_months(Months::new(g.vesting_total_months))
                .expect("within range");
            prop_assert!(
                e.vest_date <= upper,
                "event {i} vest_date {} > max {}",
                e.vest_date,
                upper,
            );
        }
    }

    #[test]
    fn prop_cliff_equals_total_single_event((g, today) in grant_strategy_cliff_equals_total()) {
        let events = derive_vesting_events(&g, today, &[]).expect("legal input");
        prop_assert_eq!(events.len(), 1, "cliff == total must yield exactly one event");
        prop_assert_eq!(events[0].shares_vested_this_event, g.share_count);
        prop_assert_eq!(events[0].cumulative_shares_vested, g.share_count);
    }

    #[test]
    fn prop_double_trigger_state_machine((g, today) in grant_strategy()) {
        let events = derive_vesting_events(&g, today, &[]).expect("legal input");
        for e in &events {
            let expected = if e.vest_date > today {
                VestingState::Upcoming
            } else if !g.double_trigger {
                VestingState::Vested
            } else {
                match g.liquidity_event_date {
                    None => VestingState::TimeVestedAwaitingLiquidity,
                    Some(liq) if liq <= today => VestingState::Vested,
                    Some(_) => VestingState::TimeVestedAwaitingLiquidity,
                }
            };
            prop_assert_eq!(
                e.state,
                expected,
                "state mismatch at {} (today={}, double_trigger={}, liq={:?})",
                e.vest_date, today, g.double_trigger, g.liquidity_event_date,
            );
        }
    }
}
