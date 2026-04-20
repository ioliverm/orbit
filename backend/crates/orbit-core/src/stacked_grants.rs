//! Multi-grant stacked-cumulative algorithm (Slice 2 T21, ADR-016 §4).
//!
//! Lives in `orbit-core` because the same algorithm is shipped by the
//! backend (authoritative) and the frontend (parity mirror) from the same
//! shared fixture file. A pure-Rust implementation in `orbit-core` keeps
//! the surface off the DB: it consumes the `VestingEvent` rows that Slice-1
//! already materializes into `vesting_events` via
//! `orbit_db::vesting_events::list_for_grant` and combines them across
//! grants owned by the same employer.
//!
//! # Why here, not in `orbit-db`
//!
//! The algorithm is deterministic + pure, consumes already-shaped domain
//! structs (`GrantMeta` + `VestingEvent`), and writes nothing. Putting it
//! in `orbit-db` would force a DB-aware signature even though the function
//! never issues a SQL statement. `orbit-core` is also the crate the
//! frontend already parity-tests against via the shared JSON fixtures
//! (vesting_cases.json); extending that pattern to the stacked view keeps
//! backend/frontend drift bounded. ADR-016 §4 explicitly says either
//! location is acceptable; `orbit-core` is the boring choice.
//!
//! # Determinism + tie-break (AC-8.2.8)
//!
//! Merged events sort by `(vest_date ASC, grant.created_at ASC, grant.id
//! ASC)`. All three keys come directly off the `grants` row — the frontend
//! has the same fields available via the `/api/v1/grants` payload. No
//! floating-point anywhere; the share-count sums are `Shares` (scaled i64).
//!
//! Traces to:
//!   - ADR-016 §4 (pseudocode).
//!   - docs/requirements/slice-2-acceptance-criteria.md §8.2.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::vesting::{Shares, VestingEvent, VestingState};

/// Metadata about a grant that the stacked view needs. Everything here
/// is already on the Slice-1 `grants` row — no new columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantMeta {
    pub id: Uuid,
    pub employer_name: String,
    pub instrument: String,
    pub created_at: DateTime<Utc>,
}

/// One point on a single grant's or an envelope's cumulative curve.
///
/// `cumulative_vested` is the sum of `shares_vested_this_event` across
/// every event whose `state = Vested` with `vest_date <= this date`.
/// `cumulative_awaiting_liquidity` is the symmetric sum for events with
/// `state = TimeVestedAwaitingLiquidity`. Upcoming events contribute to
/// neither — they are plotted on the future axis but not in the
/// "vested-to-date" surfaces (AC-8.2.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackedPoint {
    pub date: NaiveDate,
    #[serde(rename = "cumulativeSharesVested")]
    pub cumulative_vested: Shares,
    #[serde(rename = "cumulativeTimeVestedAwaitingLiquidity")]
    pub cumulative_awaiting_liquidity: Shares,
    /// One entry per grant that vested at `date`. Grants that did not
    /// vest at this date do not appear; the renderer inherits the prior
    /// cumulative from the previous point.
    #[serde(rename = "perGrantBreakdown")]
    pub per_grant_breakdown: Vec<PerGrantDelta>,
}

/// One grant's contribution at a given event date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerGrantDelta {
    #[serde(rename = "grantId")]
    pub grant_id: Uuid,
    pub instrument: String,
    #[serde(rename = "sharesVestedThisEvent")]
    pub shares_vested_this_event: Shares,
    #[serde(rename = "cumulativeForThisGrant")]
    pub cumulative_for_this_grant: Shares,
    /// Mirrors `VestingEvent::state` so the UI can dash-fill
    /// `TimeVestedAwaitingLiquidity` contributions (AC-8.2.5). One of
    /// `"upcoming"`, `"time_vested_awaiting_liquidity"`, `"vested"`.
    pub state: String,
}

/// A per-employer stacked curve + the grants that feed it. For the
/// single-grant case (AC-8.2.7) this wraps the one grant's points
/// verbatim; the handler decides whether to render a "Stacked: X" tile
/// or fall back to an individual tile based on `grants.len()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmployerStack {
    #[serde(rename = "employerName")]
    pub employer_name: String,
    /// Normalized (trimmed + lowercased) join key; useful for UI
    /// dedup/highlight but never shown to users.
    #[serde(rename = "employerKey")]
    pub employer_key: String,
    #[serde(rename = "grantIds")]
    pub grant_ids: Vec<Uuid>,
    pub points: Vec<StackedPoint>,
}

/// Top-level dashboard payload. `by_employer` carries one entry per
/// employer (single-grant tiles have a 1-entry `grant_ids`); `combined`
/// is the cross-all-grants envelope for the optional "all grants"
/// overlay referenced in AC-8.2.8's property test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackedDashboard {
    #[serde(rename = "byEmployer")]
    pub by_employer: Vec<EmployerStack>,
    pub combined: Vec<StackedPoint>,
}

/// Primary entrypoint: groups `(grant, events)` pairs by normalized
/// employer name, stacks each group via
/// [`stack_cumulative_for_employer`], and also computes the combined
/// envelope across **all** grants regardless of employer (AC-8.2.8's
/// property test anchor).
///
/// Determinism: within each employer, events merge by the three-level
/// tie-break `(vest_date, grant.created_at, grant.id)`. The output
/// vector of `EmployerStack` is sorted by employer display name
/// (case-sensitive, stable) so the dashboard renders in a predictable
/// order across refreshes. Single-grant employers still produce an
/// `EmployerStack` (with `grant_ids.len() == 1`); the handler decides
/// whether to render it as a stacked tile or a normal single tile
/// (AC-8.2.7).
pub fn stack_dashboard(inputs: Vec<(GrantMeta, Vec<VestingEvent>)>) -> StackedDashboard {
    // 1. Group by normalized employer.
    let mut buckets: BTreeMap<String, Vec<(GrantMeta, Vec<VestingEvent>)>> = BTreeMap::new();
    for pair in &inputs {
        let key = normalize_employer(&pair.0.employer_name);
        buckets.entry(key).or_default().push(pair.clone());
    }

    // 2. Per-employer stacks.
    let mut by_employer: Vec<EmployerStack> = Vec::with_capacity(buckets.len());
    for (key, mut grants) in buckets {
        grants.sort_by(|a, b| {
            b.0.created_at
                .cmp(&a.0.created_at)
                .then_with(|| b.0.id.cmp(&a.0.id))
        });
        let display = grants[0].0.employer_name.clone();
        let points = stack_cumulative_for_employer(&grants);
        let grant_ids: Vec<Uuid> = grants.iter().map(|(m, _)| m.id).collect();
        by_employer.push(EmployerStack {
            employer_name: display,
            employer_key: key,
            grant_ids,
            points,
        });
    }
    by_employer.sort_by(|a, b| a.employer_name.cmp(&b.employer_name));

    // 3. Combined envelope: treat every grant as one big group.
    let combined = stack_cumulative_for_employer(&inputs);

    StackedDashboard {
        by_employer,
        combined,
    }
}

/// Merge and walk the vesting events for one group of grants (same
/// employer in the typical per-employer call; all grants for the
/// combined-envelope call).
///
/// The caller passes `(meta, events)` pairs; the function is agnostic
/// about whether the pairs share an employer. All events merge into a
/// single date-ordered stream; at each distinct `vest_date` we emit one
/// `StackedPoint` whose `per_grant_breakdown` lists every grant that
/// vested on that date.
pub fn stack_cumulative_for_employer(
    grants: &[(GrantMeta, Vec<VestingEvent>)],
) -> Vec<StackedPoint> {
    if grants.is_empty() {
        return Vec::new();
    }

    // 1. Build a metadata lookup for the tie-break sort.
    let meta_by_id: BTreeMap<Uuid, &GrantMeta> = grants.iter().map(|(m, _)| (m.id, m)).collect();

    // 2. Flatten to (grant_id, event) pairs.
    let mut merged: Vec<(Uuid, &VestingEvent)> = grants
        .iter()
        .flat_map(|(m, evs)| evs.iter().map(move |e| (m.id, e)))
        .collect();

    // 3. Three-level deterministic sort (AC-8.2.8): vest_date, then
    //    grant.created_at, then grant.id.
    merged.sort_by(|a, b| {
        a.1.vest_date
            .cmp(&b.1.vest_date)
            .then_with(|| {
                let a_meta = meta_by_id
                    .get(&a.0)
                    .expect("meta present for merged grant_id");
                let b_meta = meta_by_id
                    .get(&b.0)
                    .expect("meta present for merged grant_id");
                a_meta.created_at.cmp(&b_meta.created_at)
            })
            .then_with(|| a.0.cmp(&b.0))
    });

    // 4. Walk, grouping by vest_date.
    let mut points: Vec<StackedPoint> = Vec::new();
    let mut running_vested: BTreeMap<Uuid, Shares> = BTreeMap::new();
    let mut running_awaiting: BTreeMap<Uuid, Shares> = BTreeMap::new();

    let mut i = 0usize;
    while i < merged.len() {
        let current_date = merged[i].1.vest_date;
        let mut breakdown: Vec<PerGrantDelta> = Vec::new();

        // Consume every event that shares `current_date`.
        while i < merged.len() && merged[i].1.vest_date == current_date {
            let (gid, ev) = (merged[i].0, merged[i].1);
            let instrument = meta_by_id
                .get(&gid)
                .map(|m| m.instrument.clone())
                .unwrap_or_default();
            match ev.state {
                VestingState::Vested => {
                    *running_vested.entry(gid).or_default() += ev.shares_vested_this_event;
                }
                VestingState::TimeVestedAwaitingLiquidity => {
                    *running_awaiting.entry(gid).or_default() += ev.shares_vested_this_event;
                }
                VestingState::Upcoming => {
                    // Upcoming events are included in the breakdown so
                    // the chart's future segment is rendered, but they do
                    // not advance the vested-to-date sum.
                }
            }
            let cumulative_for_this_grant = running_vested.get(&gid).copied().unwrap_or(0)
                + running_awaiting.get(&gid).copied().unwrap_or(0);
            breakdown.push(PerGrantDelta {
                grant_id: gid,
                instrument,
                shares_vested_this_event: ev.shares_vested_this_event,
                cumulative_for_this_grant,
                state: match ev.state {
                    VestingState::Upcoming => "upcoming".to_string(),
                    VestingState::TimeVestedAwaitingLiquidity => {
                        "time_vested_awaiting_liquidity".to_string()
                    }
                    VestingState::Vested => "vested".to_string(),
                },
            });
            i += 1;
        }

        let cumulative_vested: Shares = running_vested.values().copied().sum();
        let cumulative_awaiting_liquidity: Shares = running_awaiting.values().copied().sum();

        points.push(StackedPoint {
            date: current_date,
            cumulative_vested,
            cumulative_awaiting_liquidity,
            per_grant_breakdown: breakdown,
        });
    }

    points
}

/// Case-insensitive employer-name compare per AC-8.2.1: trim whitespace,
/// lowercase. The UI never surfaces the normalized form; it is the join
/// key only.
pub fn normalize_employer(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Per-grant vested-to-date at a given date (sum of `Vested` +
/// `TimeVestedAwaitingLiquidity` with `vest_date <= date`). The property
/// test relies on this helper being equivalent to `vesting::vested_to_date`
/// when summed over all grants.
pub fn vested_to_date_at(events: &[VestingEvent], date: NaiveDate) -> (Shares, Shares) {
    let mut vested: Shares = 0;
    let mut awaiting: Shares = 0;
    for e in events {
        if e.vest_date > date {
            continue;
        }
        match e.state {
            VestingState::Vested => vested += e.shares_vested_this_event,
            VestingState::TimeVestedAwaitingLiquidity => awaiting += e.shares_vested_this_event,
            VestingState::Upcoming => {}
        }
    }
    (vested, awaiting)
}

// ---------------------------------------------------------------------------
// Tests (pure, no DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vesting::{derive_vesting_events, Cadence, GrantInput};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn ts(y: i32, mo: u32, day: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, y, mo, day, 0, 0, 0).unwrap()
    }

    fn meta(id: Uuid, employer: &str, instrument: &str, created: DateTime<Utc>) -> GrantMeta {
        GrantMeta {
            id,
            employer_name: employer.to_string(),
            instrument: instrument.to_string(),
            created_at: created,
        }
    }

    fn rsu_grant(
        share_count: Shares,
        vesting_start: NaiveDate,
        total_months: u32,
        cliff: u32,
    ) -> GrantInput {
        GrantInput {
            share_count,
            vesting_start,
            vesting_total_months: total_months,
            cliff_months: cliff,
            cadence: Cadence::Monthly,
            double_trigger: false,
            liquidity_event_date: None,
        }
    }

    #[test]
    fn normalize_employer_trims_and_lowercases() {
        assert_eq!(normalize_employer("  ACME Inc.  "), "acme inc.");
        assert_eq!(normalize_employer("acme inc."), "acme inc.");
    }

    #[test]
    fn empty_inputs_produce_empty_dashboard() {
        let out = stack_dashboard(Vec::new());
        assert!(out.by_employer.is_empty());
        assert!(out.combined.is_empty());
    }

    #[test]
    fn single_grant_produces_one_employer_with_single_id() {
        let id = Uuid::new_v4();
        let g = rsu_grant(crate::vesting::whole_shares(1200), d(2024, 1, 15), 12, 0);
        let events = derive_vesting_events(&g, d(2030, 1, 1)).unwrap();
        let inputs = vec![(meta(id, "Acme", "rsu", ts(2024, 1, 1)), events.clone())];

        let out = stack_dashboard(inputs);
        assert_eq!(out.by_employer.len(), 1);
        let es = &out.by_employer[0];
        assert_eq!(es.employer_name, "Acme");
        assert_eq!(es.grant_ids, vec![id]);
        assert_eq!(es.points.len(), events.len());
        // Combined equals per-employer for a single grant.
        assert_eq!(out.combined, es.points);
    }

    #[test]
    fn two_grants_same_employer_share_a_stack() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let ga = rsu_grant(crate::vesting::whole_shares(1200), d(2024, 1, 15), 12, 0);
        let gb = rsu_grant(crate::vesting::whole_shares(2400), d(2024, 1, 15), 12, 0);
        let events_a = derive_vesting_events(&ga, d(2030, 1, 1)).unwrap();
        let events_b = derive_vesting_events(&gb, d(2030, 1, 1)).unwrap();
        let inputs = vec![
            (
                meta(a_id, "ACME Inc.", "rsu", ts(2024, 1, 1)),
                events_a.clone(),
            ),
            (
                meta(b_id, "acme inc.", "rsu", ts(2024, 6, 1)),
                events_b.clone(),
            ),
        ];

        let out = stack_dashboard(inputs);
        assert_eq!(out.by_employer.len(), 1, "case-insensitive merge");
        let es = &out.by_employer[0];
        assert_eq!(es.grant_ids.len(), 2);
        // Display name is the most-recently-created grant's employer_name
        // (so we don't pick "ACME Inc." when the user later wrote "acme inc.").
        assert_eq!(es.employer_name, "acme inc.");

        // Envelope at every point = sum of per-grant cumulatives.
        for p in &es.points {
            let sum: Shares = p
                .per_grant_breakdown
                .iter()
                .map(|d| d.cumulative_for_this_grant)
                .sum::<Shares>()
                .max(0);
            // per-grant-breakdown only carries grants that vested AT this
            // date — the envelope's running sum is not equal to the sum of
            // breakdown rows in general. Instead we assert the envelope
            // equals the sum of each grant's vested_to_date at `p.date`.
            let _ = sum;
            let expected_vested: Shares = {
                let (va, aa) = vested_to_date_at(&events_a, p.date);
                let (vb, ab) = vested_to_date_at(&events_b, p.date);
                (va + vb) + (aa + ab)
            };
            assert_eq!(
                p.cumulative_vested + p.cumulative_awaiting_liquidity,
                expected_vested,
                "envelope at {}",
                p.date
            );
        }
    }

    #[test]
    fn disjoint_employers_split_into_two_stacks() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let g = rsu_grant(crate::vesting::whole_shares(1200), d(2024, 1, 15), 12, 0);
        let events = derive_vesting_events(&g, d(2030, 1, 1)).unwrap();
        let inputs = vec![
            (meta(a_id, "Alpha", "rsu", ts(2024, 1, 1)), events.clone()),
            (meta(b_id, "Bravo", "nso", ts(2024, 2, 1)), events.clone()),
        ];

        let out = stack_dashboard(inputs);
        assert_eq!(out.by_employer.len(), 2);
        let names: Vec<_> = out.by_employer.iter().map(|e| &e.employer_name).collect();
        assert_eq!(names, vec!["Alpha", "Bravo"]);
    }

    #[test]
    fn mixed_instrument_stack_keeps_instrument_per_breakdown_row() {
        let rsu_id = Uuid::new_v4();
        let nso_id = Uuid::new_v4();
        let g = rsu_grant(crate::vesting::whole_shares(1200), d(2024, 1, 15), 12, 0);
        let events = derive_vesting_events(&g, d(2030, 1, 1)).unwrap();
        let inputs = vec![
            (meta(rsu_id, "Acme", "rsu", ts(2024, 1, 1)), events.clone()),
            (meta(nso_id, "Acme", "nso", ts(2024, 2, 1)), events.clone()),
        ];

        let out = stack_dashboard(inputs);
        assert_eq!(out.by_employer.len(), 1);
        let es = &out.by_employer[0];
        // Each per_grant_breakdown entry at any vest date carries its own
        // instrument label (AC-8.2.4).
        let instruments: std::collections::BTreeSet<&str> = es
            .points
            .iter()
            .flat_map(|p| p.per_grant_breakdown.iter().map(|b| b.instrument.as_str()))
            .collect();
        assert!(instruments.contains("rsu"));
        assert!(instruments.contains("nso"));
    }

    #[test]
    fn double_trigger_awaiting_liquidity_lands_on_awaiting_sum() {
        let id = Uuid::new_v4();
        let g = GrantInput {
            double_trigger: true,
            liquidity_event_date: None,
            ..rsu_grant(crate::vesting::whole_shares(1200), d(2024, 1, 15), 12, 0)
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1)).unwrap();
        let inputs = vec![(meta(id, "Acme", "rsu", ts(2024, 1, 1)), events.clone())];

        let out = stack_dashboard(inputs);
        let es = &out.by_employer[0];
        // Final point: everything is awaiting, nothing is "vested".
        let last = es.points.last().unwrap();
        assert_eq!(last.cumulative_vested, 0);
        assert_eq!(
            last.cumulative_awaiting_liquidity,
            crate::vesting::whole_shares(1200)
        );
    }

    #[test]
    fn determinism_holds_over_input_order_permutation() {
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();
        let ga = rsu_grant(crate::vesting::whole_shares(600), d(2024, 1, 15), 12, 0);
        let gb = rsu_grant(crate::vesting::whole_shares(1200), d(2024, 1, 15), 12, 0);
        let events_a = derive_vesting_events(&ga, d(2030, 1, 1)).unwrap();
        let events_b = derive_vesting_events(&gb, d(2030, 1, 1)).unwrap();

        let input_ab = vec![
            (meta(a_id, "Acme", "rsu", ts(2024, 1, 1)), events_a.clone()),
            (meta(b_id, "Acme", "rsu", ts(2024, 6, 1)), events_b.clone()),
        ];
        let input_ba = vec![
            (meta(b_id, "Acme", "rsu", ts(2024, 6, 1)), events_b.clone()),
            (meta(a_id, "Acme", "rsu", ts(2024, 1, 1)), events_a.clone()),
        ];

        let ab = stack_dashboard(input_ab);
        let ba = stack_dashboard(input_ba);
        // Deterministic output regardless of caller-supplied order.
        assert_eq!(ab.by_employer[0].points, ba.by_employer[0].points);
        assert_eq!(ab.combined, ba.combined);
    }

    #[test]
    fn envelope_equals_sum_of_vested_to_date_at_every_event_date_property() {
        // Property sweep: for any set of grants, `cumulative_vested +
        // cumulative_awaiting_liquidity` at date D equals the sum over
        // grants of `vested_to_date_at(events, D)` summed components.
        let g1 = rsu_grant(crate::vesting::whole_shares(1000), d(2024, 1, 15), 12, 0);
        let g2 = rsu_grant(crate::vesting::whole_shares(500), d(2024, 2, 15), 24, 6);
        let g3 = rsu_grant(crate::vesting::whole_shares(750), d(2024, 3, 15), 48, 12);
        let e1 = derive_vesting_events(&g1, d(2030, 1, 1)).unwrap();
        let e2 = derive_vesting_events(&g2, d(2030, 1, 1)).unwrap();
        let e3 = derive_vesting_events(&g3, d(2030, 1, 1)).unwrap();
        let inputs = vec![
            (
                meta(Uuid::new_v4(), "E1", "rsu", ts(2024, 1, 1)),
                e1.clone(),
            ),
            (
                meta(Uuid::new_v4(), "E2", "nso", ts(2024, 2, 1)),
                e2.clone(),
            ),
            (
                meta(Uuid::new_v4(), "E3", "rsu", ts(2024, 3, 1)),
                e3.clone(),
            ),
        ];
        let out = stack_dashboard(inputs);
        for p in &out.combined {
            let (v1, a1) = vested_to_date_at(&e1, p.date);
            let (v2, a2) = vested_to_date_at(&e2, p.date);
            let (v3, a3) = vested_to_date_at(&e3, p.date);
            assert_eq!(
                p.cumulative_vested + p.cumulative_awaiting_liquidity,
                (v1 + v2 + v3) + (a1 + a2 + a3),
                "envelope at {}",
                p.date
            );
        }
    }

    #[test]
    fn state_marker_vested_promotes_on_liquidity_event_in_past() {
        // With `liquidity_event_date` set to a date before `today`, every
        // past event should report `state = "vested"` in the breakdown.
        let id = Uuid::new_v4();
        let g = GrantInput {
            double_trigger: true,
            liquidity_event_date: Some(d(2024, 6, 1)),
            ..rsu_grant(crate::vesting::whole_shares(120), d(2024, 1, 15), 12, 0)
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1)).unwrap();
        let inputs = vec![(meta(id, "Acme", "rsu", ts(2024, 1, 1)), events)];
        let out = stack_dashboard(inputs);
        let es = &out.by_employer[0];
        for p in &es.points {
            for b in &p.per_grant_breakdown {
                assert_ne!(b.state, "time_vested_awaiting_liquidity");
            }
        }
    }
}
