// TypeScript mirror of `backend/crates/orbit-core/src/vesting.rs` (ADR-014 §2).
//
// This module is used by the live preview on the first-grant form (AC-4.2.5)
// and the dashboard / grant-detail timelines (AC-5.2.1, AC-6.1.2). The
// server is still the source of truth — on submit, the server recomputes
// `vesting_events` and returns them; we mirror the algorithm so the client
// can render a preview bitwise-identical to the eventual server output
// (AC-4.3.5 determinism).
//
// Numeric model
// -------------
// `share_count` is `NUMERIC(20,4)` in the DDL (10^16 max, 4 decimals). The
// backend carries the value as a scaled i64 in units of 1/10_000 — so do
// we, using `bigint` to avoid the `Number.MAX_SAFE_INTEGER` (~9e15) ceiling.
// Multiply-then-divide is done on `bigint` so the same intermediate value
// (`share_count * total_months`, ≤ 2.4e18) stays exact.
//
// Cliff semantics mirror the Rust module verbatim:
//   * cliff == 0 → events start at `step_months`.
//   * cliff > 0  → one event at `vesting_start + cliff months`, accumulated
//                  portion; periodic events every `step_months` thereafter.
//   * cliff == total → single final event with the full total.
//   * Last event absorbs rounding remainder so sum == total exactly.
//
// Double-trigger state-machine mirrors `state_for` in the Rust module.

export const SHARES_SCALE = 10_000n;

export type Cadence = 'monthly' | 'quarterly';

export type VestingState = 'upcoming' | 'time_vested_awaiting_liquidity' | 'vested';

/** Input to [`deriveVestingEvents`]. Matches `GrantInput` in Rust. */
export interface GrantInput {
  /** Scaled shares (1 share = 10_000). */
  shareCountScaled: bigint;
  vestingStart: Date;
  vestingTotalMonths: number;
  cliffMonths: number;
  cadence: Cadence;
  doubleTrigger: boolean;
  liquidityEventDate: Date | null;
}

export interface VestingEvent {
  vestDate: Date;
  /** Scaled shares vested on this event. */
  sharesVestedThisEventScaled: bigint;
  /** Scaled cumulative shares through this event. */
  cumulativeSharesVestedScaled: bigint;
  state: VestingState;
}

export type VestingErrorCode =
  | 'non_positive_share_count'
  | 'total_months_out_of_range'
  | 'cliff_exceeds_total'
  | 'date_overflow';

export class VestingError extends Error {
  public readonly code: VestingErrorCode;
  constructor(code: VestingErrorCode) {
    super(code);
    this.name = 'VestingError';
    this.code = code;
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Convert a whole-share number to the scaled bigint representation. */
export function wholeShares(n: number | bigint): bigint {
  return BigInt(n) * SHARES_SCALE;
}

/** Convert scaled bigint to a number of whole shares (floor). */
export function scaledToWhole(scaled: bigint): bigint {
  return scaled / SHARES_SCALE;
}

function stepMonths(c: Cadence): number {
  return c === 'quarterly' ? 3 : 1;
}

/** Days in a given year/month (month is 1..12). */
function daysInMonth(year: number, month: number): number {
  // month is 1-based. Using Date UTC with day=0 gives the last day of the
  // previous month, so (year, month, 0) -> days in `month`.
  return new Date(Date.UTC(year, month, 0)).getUTCDate();
}

/**
 * Add `months` to `base`, clamping day-of-month to the last valid day of
 * the target month (Jan 31 + 1m → Feb 28/29). Mirrors chrono's
 * `checked_add_months`. Operates in UTC to match the backend's
 * `NaiveDate` (which has no timezone).
 */
export function addMonths(base: Date, months: number): Date {
  const y = base.getUTCFullYear();
  const m = base.getUTCMonth(); // 0..11
  const d = base.getUTCDate();
  const totalMonths = m + months;
  const newY = y + Math.floor(totalMonths / 12);
  const newMIdx = ((totalMonths % 12) + 12) % 12; // safe for negative
  const maxD = daysInMonth(newY, newMIdx + 1);
  const newD = Math.min(d, maxD);
  return new Date(Date.UTC(newY, newMIdx, newD));
}

/** Floor-division on scaled shares: floor(total * i / months). */
function floorShares(iMonths: number, totalMonths: number, totalShares: bigint): bigint {
  const prod = totalShares * BigInt(iMonths);
  return prod / BigInt(totalMonths); // bigint `/` is trunc toward zero = floor for non-negative.
}

/** Compare Dates by UTC calendar date (y/m/d). Returns -1/0/1. */
function cmpDate(a: Date, b: Date): number {
  const ay = a.getUTCFullYear();
  const by = b.getUTCFullYear();
  if (ay !== by) return ay < by ? -1 : 1;
  const am = a.getUTCMonth();
  const bm = b.getUTCMonth();
  if (am !== bm) return am < bm ? -1 : 1;
  const ad = a.getUTCDate();
  const bd = b.getUTCDate();
  if (ad !== bd) return ad < bd ? -1 : 1;
  return 0;
}

function validate(g: GrantInput): void {
  if (g.shareCountScaled <= 0n) {
    throw new VestingError('non_positive_share_count');
  }
  if (g.vestingTotalMonths <= 0 || g.vestingTotalMonths > 240) {
    throw new VestingError('total_months_out_of_range');
  }
  if (g.cliffMonths < 0 || g.cliffMonths > g.vestingTotalMonths) {
    throw new VestingError('cliff_exceeds_total');
  }
}

function stateFor(g: GrantInput, today: Date, vestDate: Date): VestingState {
  if (cmpDate(vestDate, today) > 0) return 'upcoming';
  if (!g.doubleTrigger) return 'vested';
  if (g.liquidityEventDate === null) return 'time_vested_awaiting_liquidity';
  // Liquidity event set: if it has occurred by `today`, fully vested; else awaiting.
  return cmpDate(g.liquidityEventDate, today) <= 0 ? 'vested' : 'time_vested_awaiting_liquidity';
}

// ---------------------------------------------------------------------------
// Derivation
// ---------------------------------------------------------------------------

/**
 * Derive the full vesting schedule for `grant` as of `today`. Returns
 * events in chronological order. Throws `VestingError` on invalid input.
 */
export function deriveVestingEvents(g: GrantInput, today: Date): VestingEvent[] {
  validate(g);

  const total = g.shareCountScaled;
  const totalMonths = g.vestingTotalMonths;
  const cliff = g.cliffMonths;
  const step = stepMonths(g.cadence);

  const events: VestingEvent[] = [];
  let cumulative: bigint = 0n;

  // --- Cliff event (if any) ----------------------------------------------
  let nextM: number;
  if (cliff > 0 && cliff < totalMonths) {
    const atCliff = floorShares(cliff, totalMonths, total);
    const vestDate = addMonths(g.vestingStart, cliff);
    events.push({
      vestDate,
      sharesVestedThisEventScaled: atCliff,
      cumulativeSharesVestedScaled: atCliff,
      state: stateFor(g, today, vestDate),
    });
    cumulative = atCliff;
    nextM = Math.min(cliff + step, totalMonths);
  } else if (cliff === 0) {
    nextM = Math.min(step, totalMonths);
  } else {
    // cliff === totalMonths → only the final event.
    nextM = totalMonths;
  }

  // --- Periodic events ---------------------------------------------------
  let m = nextM;
  while (m <= totalMonths) {
    const target = m === totalMonths ? total : floorShares(m, totalMonths, total);
    const delta = target - cumulative;
    const vestDate = addMonths(g.vestingStart, m);
    events.push({
      vestDate,
      sharesVestedThisEventScaled: delta,
      cumulativeSharesVestedScaled: target,
      state: stateFor(g, today, vestDate),
    });
    cumulative = target;
    if (m === totalMonths) break;
    const next = m + step;
    m = next > totalMonths ? totalMonths : next;
  }

  return events;
}

/**
 * Cumulative view of a schedule as of `today`: (fully-vested, awaiting-liquidity).
 * Mirrors Rust `vested_to_date`.
 */
export function vestedToDate(
  events: VestingEvent[],
  today: Date,
): { vested: bigint; awaiting: bigint } {
  let vested: bigint = 0n;
  let awaiting: bigint = 0n;
  for (const e of events) {
    if (cmpDate(e.vestDate, today) > 0) continue;
    if (e.state === 'vested') vested += e.sharesVestedThisEventScaled;
    else if (e.state === 'time_vested_awaiting_liquidity')
      awaiting += e.sharesVestedThisEventScaled;
    // 'upcoming' ignored (unreachable when vestDate <= today under normal state assignment).
  }
  return { vested, awaiting };
}

/** Build a UTC Date from y/m/d with m=1..12. Convenience for callers. */
export function utcDate(year: number, month: number, day: number): Date {
  return new Date(Date.UTC(year, month - 1, day));
}
