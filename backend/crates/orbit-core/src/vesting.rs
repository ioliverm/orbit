//! Pure vesting-derivation algorithm per ADR-014 §3.
//!
//! This module is the source of truth for `vesting_events` row generation on
//! grant create/update. The frontend re-implements the same algorithm for the
//! live preview (AC-4.2.5); on submit the server's output is authoritative
//! (AC-4.3.5 determinism).
//!
//! # Numeric model
//!
//! `share_count` is declared `NUMERIC(20,4)` in the DDL. Four decimal places
//! is enough for every fractional-share surface Slice 1 touches (ESPP
//! fractional allocations and post-split grants). To avoid a `rust_decimal`
//! dep, we carry shares internally as **scaled `i128` in units of 1/10_000**
//! (we call this a "milli-share-x10"). The boundary API exposes plain
//! `Shares` (an `i64` in the same scaled unit) and helpers to convert from
//! / to whole-share counts. Every multiply-then-divide is on `i128` so that
//! `share_count.max * total_months.max` cannot overflow.
//!
//! The DDL's `NUMERIC(20,4)` bound + the `vesting_total_months <= 240` +
//! `share_count > 0` CHECK constraints give a comfortable headroom:
//!
//! ```text
//! max(share_count_scaled) = 10^16  (20 digits, 4 after the dot)
//! * 240 months                      = 2.4 * 10^18
//! ```
//!
//! `i128::MAX` is ~1.7e38, so we are nowhere near the ceiling.
//!
//! # Cliff semantics
//!
//! Per ADR-014 §3 pseudocode:
//!
//! * `cliff == 0` — events start at `step_months`.
//! * `cliff > 0` — a single event at `vesting_start + months(cliff)` with the
//!   accumulated portion `floor_shares(cliff, months, total)`, followed by
//!   events every `step_months`.
//! * `cliff == months` — a single event at the end with `total` shares.
//! * Last event absorbs any rounding remainder so `sum == total` exactly
//!   (AC-4.3.1).
//!
//! # Double-trigger
//!
//! State machine per ADR-014 `state_for`:
//!
//! * `vest_date > today` → `Upcoming` (regardless of double-trigger flag).
//! * `double_trigger == false` → `Vested`.
//! * `double_trigger == true` and `liquidity_event_date` not set → `TimeVestedAwaitingLiquidity`.
//! * `double_trigger == true` and `liquidity_event_date` set → `Vested`
//!   (the liquidity-event edge case in the ADR resolves to `Vested` as
//!   long as the liquidity event has occurred by `today`). Slice 1 does
//!   not render the "liquidity_trigger" audit note referenced in the ADR;
//!   deferred to Slice 2 with the audit-trail expansion.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Scaled share count — one unit = 1/10_000 of a share.
///
/// Matches the `NUMERIC(20,4)` precision of `grants.share_count` and
/// `vesting_events.shares_vested_this_event`. Keep arithmetic in this unit
/// until the very edge of the system (DB row or JSON response) to avoid
/// floating-point drift.
pub type Shares = i64;

/// Scale factor from whole shares to [`Shares`]. Four decimal places.
pub const SHARES_SCALE: i64 = 10_000;

/// Vesting cadence (DB enum `vesting_cadence`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    Monthly,
    Quarterly,
}

impl Cadence {
    fn step_months(self) -> u32 {
        match self {
            Cadence::Monthly => 1,
            Cadence::Quarterly => 3,
        }
    }
}

/// Vesting event state (DB enum `vesting_events.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VestingState {
    /// `vest_date` is strictly in the future as of the reference `today`.
    Upcoming,
    /// Time-vested but the double-trigger liquidity event has not occurred
    /// (AC-4.3.4). Double-trigger RSU only.
    TimeVestedAwaitingLiquidity,
    /// Fully vested and available as of `today`.
    Vested,
}

/// Input to [`derive_vesting_events`].
///
/// Only the fields that affect the derivation are listed here; everything
/// else on `grants` (employer, ticker, notes, strike) is metadata. The
/// caller is expected to have validated these fields upstream (`validator`
/// crate + DB CHECK constraints), but the algorithm re-validates cliff vs
/// total and positive share count to stay standalone-testable.
#[derive(Debug, Clone)]
pub struct GrantInput {
    pub share_count: Shares,
    pub vesting_start: NaiveDate,
    pub vesting_total_months: u32,
    pub cliff_months: u32,
    pub cadence: Cadence,
    pub double_trigger: bool,
    pub liquidity_event_date: Option<NaiveDate>,
}

/// A single derived vesting event.
///
/// # Slice 3 additions
///
/// `fmv_at_vest` and `fmv_currency` are populated only when the event
/// originated from a `VestingEventOverride` (Slice 3 override-preservation
/// branch). Algorithm-derived rows carry `None` for both — the DB layer's
/// `vesting_events` row is the authoritative home for captured FMV, and
/// this struct is simply the wire shape the derivation function returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VestingEvent {
    pub vest_date: NaiveDate,
    pub shares_vested_this_event: Shares,
    pub cumulative_shares_vested: Shares,
    pub state: VestingState,
    /// Slice 3: FMV carried verbatim from an override; `None` for
    /// algorithm-derived rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fmv_at_vest: Option<String>,
    /// Slice 3: currency carried verbatim from an override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fmv_currency: Option<String>,
}

/// An existing user override passed into [`derive_vesting_events`] so that
/// a grant-param change preserves the row (AC-8.4.2). The tuple
/// `(vest_date, original_derivation_index)` is the deterministic ordering
/// key when multiple overrides happen to fall on the same `vest_date`
/// (ADR-017 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VestingEventOverride {
    pub vest_date: NaiveDate,
    pub shares_vested_this_event: Shares,
    pub fmv_at_vest: Option<String>,
    pub fmv_currency: Option<String>,
    pub original_derivation_index: usize,
}

/// Errors produced by the validator + derivation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VestingError {
    #[error("share_count must be > 0")]
    NonPositiveShareCount,
    #[error("vesting_total_months must be > 0 and <= 240")]
    TotalMonthsOutOfRange,
    #[error("cliff_months must be <= vesting_total_months")]
    CliffExceedsTotal,
    #[error("vesting_start is outside the representable calendar range")]
    DateOverflow,
}

// ---------------------------------------------------------------------------
// Derivation
// ---------------------------------------------------------------------------

/// Derive the full vesting schedule for `grant` as of `today`.
///
/// Returns events in chronological order. Every invariant from AC-4.3.1..5
/// is exercised by the unit + property tests in this module.
///
/// # Slice 3 override preservation (ADR-017 §2)
///
/// `existing_overrides` carries the user's previously-edited rows across a
/// grant-param change (AC-8.4.2). When the slice is empty the function is
/// bit-identical to its Slice-1 shape: pure derivation from `grant`, the
/// cumulative invariant `sum == share_count` holds (asserted in debug
/// builds).
///
/// When any override is present:
///
///   * Every override is returned **verbatim** at its `vest_date`, carrying
///     its shares and FMV unchanged. The `state` is recomputed against
///     `today`.
///   * Algorithm-derived slots that do NOT collide with an override by
///     `vest_date` are emitted from the standard derivation.
///   * The cumulative-invariant is **relaxed** — the sum MAY differ from
///     `grant.share_count` per AC-8.5.2. The caller renders the UI banner
///     per AC-8.5.3.
///   * Events are sorted by `(vest_date ASC, original_derivation_index
///     ASC)` for deterministic output across runs.
pub fn derive_vesting_events(
    grant: &GrantInput,
    today: NaiveDate,
    existing_overrides: &[VestingEventOverride],
) -> Result<Vec<VestingEvent>, VestingError> {
    validate(grant)?;

    if existing_overrides.is_empty() {
        return derive_no_overrides(grant, today);
    }

    derive_with_overrides(grant, today, existing_overrides)
}

/// Slice-1 path — unchanged behavior, cumulative invariant asserted.
fn derive_no_overrides(
    grant: &GrantInput,
    today: NaiveDate,
) -> Result<Vec<VestingEvent>, VestingError> {
    let total = grant.share_count;
    let total_months = grant.vesting_total_months;
    let cliff = grant.cliff_months;
    let step = grant.cadence.step_months();

    let mut events: Vec<VestingEvent> = Vec::with_capacity(
        // Upper bound: total_months / step_months + 1 (cliff event).
        (total_months as usize) / (step as usize) + 2,
    );
    let mut cumulative: Shares = 0;

    // --- Cliff event (if any) ----------------------------------------------
    // If cliff == total_months, we still emit one event at the end, which the
    // main loop below handles via the `m == total_months` remainder branch.
    // Handle the cliff > 0 && cliff < total_months case here.
    let next_m: u32 = if cliff > 0 && cliff < total_months {
        let at_cliff = floor_shares(cliff, total_months, total);
        let vest_date = add_months(grant.vesting_start, cliff)?;
        events.push(VestingEvent {
            vest_date,
            shares_vested_this_event: at_cliff,
            cumulative_shares_vested: at_cliff,
            state: state_for(grant, today, vest_date),
            fmv_at_vest: None,
            fmv_currency: None,
        });
        cumulative = at_cliff;
        // Clamp so that a cadence whose `cliff + step` overshoots the total
        // still produces the final remainder event at `total_months` below.
        (cliff + step).min(total_months)
    } else if cliff == 0 {
        step.min(total_months)
    } else {
        // cliff == total_months: skip the pre-final loop; handle below.
        total_months
    };

    // --- Periodic events ---------------------------------------------------
    let mut m = next_m;
    while m <= total_months {
        let target = if m == total_months {
            // Last event absorbs any rounding remainder (AC-4.3.1).
            total
        } else {
            floor_shares(m, total_months, total)
        };
        let delta = target - cumulative;
        // Guard against a non-monotonic step introduced by a future refactor.
        debug_assert!(delta >= 0, "delta must be non-negative");
        let vest_date = add_months(grant.vesting_start, m)?;
        events.push(VestingEvent {
            vest_date,
            shares_vested_this_event: delta,
            cumulative_shares_vested: target,
            state: state_for(grant, today, vest_date),
            fmv_at_vest: None,
            fmv_currency: None,
        });
        cumulative = target;
        // Break if we just emitted the final event; `m + step` could overshoot.
        if m == total_months {
            break;
        }
        let next = m + step;
        // Clamp to `total_months` so that a non-dividing cadence (e.g. 13
        // months monthly-stepped by 3) still emits the final remainder event.
        m = if next > total_months {
            total_months
        } else {
            next
        };
    }

    // Sum invariant (AC-4.3.1). A debug_assert keeps this testable without
    // gating production correctness on it.
    debug_assert_eq!(cumulative, total, "sum of shares must equal total");

    Ok(events)
}

/// Slice-3 override-aware path. Builds the base schedule, substitutes
/// matching overrides by `vest_date`, merges any overrides that fall
/// outside the derivation window (they survive — AC-8.4.2), then sorts
/// by `(vest_date, original_derivation_index)` for deterministic output.
/// The cumulative-invariant is NOT asserted here; the caller renders
/// AC-8.5.3 when the sum diverges.
fn derive_with_overrides(
    grant: &GrantInput,
    today: NaiveDate,
    existing_overrides: &[VestingEventOverride],
) -> Result<Vec<VestingEvent>, VestingError> {
    let base = derive_no_overrides(grant, today)?;

    // Build a merged list: every base slot becomes either a passthrough
    // (original_derivation_index = slot's position) or a substitution
    // (the override at that vest_date wins). Overrides whose vest_date
    // does not land on any base slot are appended as extra events
    // carrying their own original_derivation_index.
    let mut merged: Vec<(usize, VestingEvent)> =
        Vec::with_capacity(base.len() + existing_overrides.len());
    let mut consumed: Vec<bool> = vec![false; existing_overrides.len()];

    for (slot_idx, slot) in base.iter().enumerate() {
        // Match by vest_date. First unconsumed override wins (overrides
        // with the same vest_date break ties via their original index
        // after the sort — this loop is stable because we walk slots
        // in order).
        let hit = existing_overrides
            .iter()
            .enumerate()
            .find(|(i, o)| !consumed[*i] && o.vest_date == slot.vest_date);
        match hit {
            Some((i, over)) => {
                consumed[i] = true;
                merged.push((
                    over.original_derivation_index,
                    override_to_event(over, grant, today),
                ));
            }
            None => {
                merged.push((slot_idx, slot.clone()));
            }
        }
    }

    // Overrides without a matching base slot — preserve them (AC-8.4.2
    // override-outside-window case).
    for (i, over) in existing_overrides.iter().enumerate() {
        if !consumed[i] {
            merged.push((
                over.original_derivation_index,
                override_to_event(over, grant, today),
            ));
        }
    }

    // Sort by (vest_date ASC, original_derivation_index ASC).
    merged.sort_by(|a, b| {
        a.1.vest_date
            .cmp(&b.1.vest_date)
            .then_with(|| a.0.cmp(&b.0))
    });

    // Recompute cumulative_shares_vested for the merged sequence so the
    // returned events remain internally consistent as a prefix-sum.
    let mut cumulative: Shares = 0;
    let events: Vec<VestingEvent> = merged
        .into_iter()
        .map(|(_, mut e)| {
            cumulative = cumulative.saturating_add(e.shares_vested_this_event);
            e.cumulative_shares_vested = cumulative;
            e
        })
        .collect();

    Ok(events)
}

fn override_to_event(
    over: &VestingEventOverride,
    grant: &GrantInput,
    today: NaiveDate,
) -> VestingEvent {
    VestingEvent {
        vest_date: over.vest_date,
        shares_vested_this_event: over.shares_vested_this_event,
        // Cumulative is recomputed by the merging caller.
        cumulative_shares_vested: 0,
        state: state_for(grant, today, over.vest_date),
        fmv_at_vest: over.fmv_at_vest.clone(),
        fmv_currency: over.fmv_currency.clone(),
    }
}

/// Cumulative view of a schedule as of `today`: (fully vested, awaiting-liquidity).
///
/// `fully_vested` counts only events in state `Vested` whose `vest_date <=
/// today`. `awaiting_liquidity` counts events in state
/// `TimeVestedAwaitingLiquidity` whose `vest_date <= today`. Upcoming events
/// are excluded from both. (ADR-014 `vested_to_date`, used by AC-5.2.1 and
/// AC-6.1.4.)
pub fn vested_to_date(events: &[VestingEvent], today: NaiveDate) -> (Shares, Shares) {
    let mut vested: Shares = 0;
    let mut awaiting: Shares = 0;
    for e in events {
        if e.vest_date > today {
            continue;
        }
        match e.state {
            VestingState::Vested => vested += e.shares_vested_this_event,
            VestingState::TimeVestedAwaitingLiquidity => awaiting += e.shares_vested_this_event,
            VestingState::Upcoming => {
                // Unreachable given `vest_date <= today`, but if a caller has
                // desynced `today` between state-assignment and this call
                // we err on the conservative side and count as neither.
            }
        }
    }
    (vested, awaiting)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn validate(g: &GrantInput) -> Result<(), VestingError> {
    if g.share_count <= 0 {
        return Err(VestingError::NonPositiveShareCount);
    }
    if g.vesting_total_months == 0 || g.vesting_total_months > 240 {
        return Err(VestingError::TotalMonthsOutOfRange);
    }
    if g.cliff_months > g.vesting_total_months {
        return Err(VestingError::CliffExceedsTotal);
    }
    Ok(())
}

/// Floor-division on scaled shares. `i * total / months` in `i128`, truncating
/// at the `Shares` boundary. Never `FromStr`-ed from JSON, so bounds are
/// checked against `i64::MAX` at the contract edge, not here.
fn floor_shares(i_months: u32, total_months: u32, total_shares: Shares) -> Shares {
    let prod: i128 = (total_shares as i128) * (i_months as i128);
    // Integer floor. For non-negative operands `/` is toward zero = floor.
    let q = prod / (total_months as i128);
    q as Shares
}

/// Add `months` to `base`, clamping day-of-month to the last valid day of the
/// target month (e.g. Jan 31 + 1 month → Feb 28 or 29). This matches the
/// "month arithmetic" convention most vesting contracts use: if the grant
/// vests on "the 31st of each month", months that do not have a 31st vest
/// on their last day.
///
/// Returns `VestingError::DateOverflow` if the result is outside chrono's
/// representable calendar range (never happens in practice: `NaiveDate`
/// covers years -262144..=262143).
fn add_months(base: NaiveDate, months: u32) -> Result<NaiveDate, VestingError> {
    // chrono's stable `checked_add_months` does exactly this (day-clamp to
    // end-of-month). Available since chrono 0.4.24.
    base.checked_add_months(chrono::Months::new(months))
        .ok_or(VestingError::DateOverflow)
}

fn state_for(g: &GrantInput, today: NaiveDate, vest_date: NaiveDate) -> VestingState {
    if vest_date > today {
        return VestingState::Upcoming;
    }
    if !g.double_trigger {
        return VestingState::Vested;
    }
    match g.liquidity_event_date {
        None => VestingState::TimeVestedAwaitingLiquidity,
        Some(liq) if liq <= today => VestingState::Vested,
        // Liquidity event is set but has not occurred yet by `today`.
        Some(_) => VestingState::TimeVestedAwaitingLiquidity,
    }
}

/// Convenience constructor: whole shares → scaled [`Shares`].
pub fn whole_shares(n: i64) -> Shares {
    n.saturating_mul(SHARES_SCALE)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn base(share_count: Shares) -> GrantInput {
        GrantInput {
            share_count,
            vesting_start: d(2024, 9, 15),
            vesting_total_months: 48,
            cliff_months: 12,
            cadence: Cadence::Monthly,
            double_trigger: false,
            liquidity_event_date: None,
        }
    }

    // --- Validation --------------------------------------------------------

    #[test]
    fn rejects_non_positive_share_count() {
        let g = GrantInput {
            share_count: 0,
            ..base(0)
        };
        assert_eq!(
            derive_vesting_events(&g, d(2026, 1, 1), &[]).unwrap_err(),
            VestingError::NonPositiveShareCount
        );
    }

    #[test]
    fn rejects_zero_total_months() {
        let g = GrantInput {
            vesting_total_months: 0,
            ..base(whole_shares(100))
        };
        assert_eq!(
            derive_vesting_events(&g, d(2026, 1, 1), &[]).unwrap_err(),
            VestingError::TotalMonthsOutOfRange
        );
    }

    #[test]
    fn rejects_total_months_over_cap() {
        let g = GrantInput {
            vesting_total_months: 241,
            ..base(whole_shares(100))
        };
        assert_eq!(
            derive_vesting_events(&g, d(2026, 1, 1), &[]).unwrap_err(),
            VestingError::TotalMonthsOutOfRange
        );
    }

    #[test]
    fn rejects_cliff_exceeds_total() {
        let g = GrantInput {
            cliff_months: 49,
            ..base(whole_shares(100))
        };
        assert_eq!(
            derive_vesting_events(&g, d(2026, 1, 1), &[]).unwrap_err(),
            VestingError::CliffExceedsTotal
        );
    }

    // --- AC-4.3.1 — sum equals total --------------------------------------

    #[test]
    fn sum_equals_total_for_standard_monthly_cliff() {
        // 30,000 shares, 48 months, 12-month cliff, monthly: canonical RSU.
        let g = base(whole_shares(30_000));
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
        assert_eq!(sum, whole_shares(30_000));
        assert_eq!(events.last().unwrap().cumulative_shares_vested, sum);
    }

    #[test]
    fn sum_equals_total_for_quarterly() {
        // AC-4.3.3 — quarterly cadence.
        let g = GrantInput {
            cadence: Cadence::Quarterly,
            ..base(whole_shares(12_345))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
        assert_eq!(sum, whole_shares(12_345));
    }

    #[test]
    fn sum_equals_total_no_cliff_monthly() {
        let g = GrantInput {
            cliff_months: 0,
            vesting_total_months: 36,
            ..base(whole_shares(1_000))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
        assert_eq!(sum, whole_shares(1_000));
    }

    #[test]
    fn sum_equals_total_fractional() {
        // 123.4567 shares — fractional ESPP-style grant. Because SHARES_SCALE
        // is 10_000, `whole_shares` won't round; we set the scaled value
        // directly.
        let g = base(1_234_567);
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
        assert_eq!(sum, 1_234_567);
    }

    // --- AC-4.3.2 — cliff behavior -----------------------------------------

    #[test]
    fn first_event_is_at_cliff_with_accumulated_portion() {
        let g = base(whole_shares(48_000)); // 48k over 48 months, 12-month cliff.
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        assert_eq!(events[0].vest_date, d(2025, 9, 15));
        // 12/48 * 48,000 = 12,000.
        assert_eq!(events[0].cumulative_shares_vested, whole_shares(12_000));
        assert_eq!(events[0].shares_vested_this_event, whole_shares(12_000));
    }

    #[test]
    fn no_event_before_cliff() {
        let g = base(whole_shares(48_000));
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        let cliff_date = d(2025, 9, 15);
        assert!(events.iter().all(|e| e.vest_date >= cliff_date));
    }

    // --- AC-4.3.3 — quarterly grains --------------------------------------

    #[test]
    fn quarterly_events_are_exactly_three_months_apart() {
        let g = GrantInput {
            cadence: Cadence::Quarterly,
            cliff_months: 0,
            vesting_total_months: 12,
            ..base(whole_shares(1_000))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        // 12 months / 3-month step → 4 events.
        assert_eq!(events.len(), 4);
        let dates: Vec<_> = events.iter().map(|e| e.vest_date).collect();
        assert_eq!(dates[0], d(2024, 12, 15));
        assert_eq!(dates[1], d(2025, 3, 15));
        assert_eq!(dates[2], d(2025, 6, 15));
        assert_eq!(dates[3], d(2025, 9, 15));
    }

    // --- AC-4.3.4 — double-trigger states ---------------------------------

    #[test]
    fn double_trigger_without_liquidity_marks_time_vested_awaiting() {
        let g = GrantInput {
            double_trigger: true,
            liquidity_event_date: None,
            ..base(whole_shares(4_800))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        // Every past event should be `TimeVestedAwaitingLiquidity`.
        for e in &events {
            assert_eq!(e.state, VestingState::TimeVestedAwaitingLiquidity);
        }
    }

    #[test]
    fn double_trigger_with_liquidity_in_past_marks_vested() {
        let g = GrantInput {
            double_trigger: true,
            liquidity_event_date: Some(d(2025, 1, 1)),
            ..base(whole_shares(4_800))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        for e in &events {
            assert_eq!(e.state, VestingState::Vested);
        }
    }

    #[test]
    fn double_trigger_with_future_liquidity_is_awaiting() {
        let g = GrantInput {
            double_trigger: true,
            liquidity_event_date: Some(d(2035, 1, 1)),
            ..base(whole_shares(4_800))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        for e in &events {
            assert!(matches!(
                e.state,
                VestingState::TimeVestedAwaitingLiquidity | VestingState::Upcoming
            ));
        }
    }

    #[test]
    fn future_events_are_upcoming_regardless_of_double_trigger() {
        let g = GrantInput {
            double_trigger: true,
            liquidity_event_date: None,
            ..base(whole_shares(4_800))
        };
        let events = derive_vesting_events(&g, d(2025, 1, 1), &[]).unwrap();
        // `today` is 2025-01-01; the cliff event is 2025-09-15 → still Upcoming.
        assert!(events.iter().all(|e| e.state == VestingState::Upcoming));
    }

    // --- AC-4.3.5 — determinism --------------------------------------------

    #[test]
    fn same_input_same_output() {
        let g = base(whole_shares(30_000));
        let a = derive_vesting_events(&g, d(2026, 3, 1), &[]).unwrap();
        let b = derive_vesting_events(&g, d(2026, 3, 1), &[]).unwrap();
        assert_eq!(a, b);
    }

    // --- Monotonicity ------------------------------------------------------

    #[test]
    fn cumulative_is_monotonic_non_decreasing() {
        let g = base(whole_shares(7_777));
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        let mut prev = 0;
        for e in &events {
            assert!(e.cumulative_shares_vested >= prev);
            prev = e.cumulative_shares_vested;
        }
    }

    // --- Edge cases --------------------------------------------------------

    #[test]
    fn cliff_equals_total_single_event_at_end() {
        let g = GrantInput {
            cliff_months: 48,
            ..base(whole_shares(1_000))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].shares_vested_this_event, whole_shares(1_000));
        assert_eq!(events[0].cumulative_shares_vested, whole_shares(1_000));
        // `vesting_start + 48 months` = 2024-09-15 + 48m = 2028-09-15.
        assert_eq!(events[0].vest_date, d(2028, 9, 15));
    }

    #[test]
    fn leap_year_does_not_disturb_monthly_events() {
        // vesting_start right before Feb in a leap year: Jan 31, 2024.
        // Monthly + no cliff.
        let g = GrantInput {
            vesting_start: d(2024, 1, 31),
            cliff_months: 0,
            vesting_total_months: 3,
            cadence: Cadence::Monthly,
            ..base(whole_shares(300))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        assert_eq!(events.len(), 3);
        // Jan 31 + 1 month → Feb 29 (leap year); + 2 months → Mar 31; + 3 → Apr 30.
        assert_eq!(events[0].vest_date, d(2024, 2, 29));
        assert_eq!(events[1].vest_date, d(2024, 3, 31));
        assert_eq!(events[2].vest_date, d(2024, 4, 30));
    }

    #[test]
    fn day_of_month_clamps_to_last_day() {
        // Mar 31 + 1 month → Apr 30.
        let g = GrantInput {
            vesting_start: d(2025, 3, 31),
            cliff_months: 0,
            vesting_total_months: 1,
            cadence: Cadence::Monthly,
            ..base(whole_shares(10))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        assert_eq!(events[0].vest_date, d(2025, 4, 30));
    }

    #[test]
    fn non_divisible_cadence_still_sums_to_total() {
        // 13 months with a quarterly cadence: events at months 3,6,9,12,13
        // (the final tail absorbs the remainder).
        let g = GrantInput {
            cliff_months: 0,
            vesting_total_months: 13,
            cadence: Cadence::Quarterly,
            ..base(whole_shares(1_000))
        };
        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
        let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
        assert_eq!(sum, whole_shares(1_000));
        // Last event should fall at month 13.
        let last = events.last().unwrap();
        assert_eq!(
            last.vest_date,
            d(2024, 9, 15)
                .checked_add_months(chrono::Months::new(13))
                .unwrap()
        );
    }

    // --- vested_to_date ----------------------------------------------------

    #[test]
    fn vested_to_date_matches_events_before_today() {
        // 30,000 shares over 48 months, 12-month cliff, monthly.
        // vesting_start = 2024-09-15; today = 2025-10-15 should be
        // 13/48 * 30,000 = 8,125 shares (13 monthly tranches through month 13).
        let g = base(whole_shares(30_000));
        let events = derive_vesting_events(&g, d(2025, 10, 15), &[]).unwrap();
        let (vested, awaiting) = vested_to_date(&events, d(2025, 10, 15));
        assert_eq!(awaiting, 0);
        // 12-month cliff yields 7,500 shares on 2025-09-15; monthly after that.
        // 2025-10-15 is exactly month 13 → 8,125.
        assert_eq!(vested, whole_shares(8_125));
    }

    #[test]
    fn vested_to_date_separates_awaiting_liquidity() {
        let g = GrantInput {
            double_trigger: true,
            liquidity_event_date: None,
            ..base(whole_shares(30_000))
        };
        let events = derive_vesting_events(&g, d(2025, 10, 15), &[]).unwrap();
        let (vested, awaiting) = vested_to_date(&events, d(2025, 10, 15));
        assert_eq!(vested, 0);
        assert_eq!(awaiting, whole_shares(8_125));
    }

    // --- Property-like sweeps ---------------------------------------------

    /// Sum-of-shares invariant across a broad parameter sweep (AC-4.3.1 /
    /// AC-4.3.5). Not a full QuickCheck but exercises enough (share_count,
    /// months, cliff, cadence) combinations to catch rounding drift.
    #[test]
    fn sweep_sum_equals_total() {
        let shares_cases: &[Shares] = &[
            1,
            SHARES_SCALE,
            whole_shares(1),
            whole_shares(7),
            whole_shares(100),
            whole_shares(12_345),
            whole_shares(1_000_000),
            // A fractional share count.
            1_234_567,
        ];
        for &share_count in shares_cases {
            for &total_months in &[1u32, 3, 12, 24, 36, 48, 60, 120, 240] {
                for &cliff in &[0u32, 1, 3, 6, 12] {
                    if cliff > total_months {
                        continue;
                    }
                    for cadence in [Cadence::Monthly, Cadence::Quarterly] {
                        let g = GrantInput {
                            share_count,
                            vesting_start: d(2024, 1, 15),
                            vesting_total_months: total_months,
                            cliff_months: cliff,
                            cadence,
                            double_trigger: false,
                            liquidity_event_date: None,
                        };
                        let events = derive_vesting_events(&g, d(2030, 1, 1), &[]).unwrap();
                        let sum: Shares = events.iter().map(|e| e.shares_vested_this_event).sum();
                        assert_eq!(
                            sum, share_count,
                            "sum != share_count for \
                             share_count={share_count}, months={total_months}, \
                             cliff={cliff}, cadence={cadence:?}"
                        );
                        // Monotonic cumulative.
                        let mut prev = 0;
                        for e in &events {
                            assert!(e.cumulative_shares_vested >= prev);
                            prev = e.cumulative_shares_vested;
                        }
                        // No event before the cliff.
                        let cliff_date = d(2024, 1, 15)
                            .checked_add_months(chrono::Months::new(cliff))
                            .unwrap();
                        if cliff > 0 {
                            assert!(
                                events.iter().all(|e| e.vest_date >= cliff_date),
                                "event before cliff for \
                                 share_count={share_count}, months={total_months}, \
                                 cliff={cliff}, cadence={cadence:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
