// TypeScript parity tests for the vesting algorithm. These hand-mirror the
// canonical fixtures used by `orbit-core::vesting::tests`:
//
//   * RSU 30,000 shares / 48 months / 12-month cliff / monthly (the wizard
//     default from first-grant-form.html).
//   * Quarterly, 12-month total, no cliff.
//   * Cliff == total (single final event).
//   * Fractional share count (1_234_567 scaled = 123.4567 shares).
//   * Day-of-month clamping (Jan 31 + 1m → Feb 29 in a leap year).
//
// Any drift between these and the Rust fixtures fails AC-4.3.5 (determinism)
// and blocks the PR.

import { describe, expect, it } from 'vitest';
import {
  addMonths,
  deriveVestingEvents,
  SHARES_SCALE,
  type GrantInput,
  utcDate,
  vestedToDate,
  wholeShares,
} from '../vesting';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function base(shareCountScaled: bigint): GrantInput {
  return {
    shareCountScaled,
    vestingStart: utcDate(2024, 9, 15),
    vestingTotalMonths: 48,
    cliffMonths: 12,
    cadence: 'monthly',
    doubleTrigger: false,
    liquidityEventDate: null,
  };
}

// ---------------------------------------------------------------------------
// addMonths — day-of-month clamping
// ---------------------------------------------------------------------------

describe('addMonths', () => {
  it('Jan 31 + 1 month → Feb 29 in a leap year (2024)', () => {
    const d = addMonths(utcDate(2024, 1, 31), 1);
    expect(d.getUTCFullYear()).toBe(2024);
    expect(d.getUTCMonth()).toBe(1); // Feb
    expect(d.getUTCDate()).toBe(29);
  });

  it('Jan 31 + 1 month → Feb 28 in a non-leap year (2023)', () => {
    const d = addMonths(utcDate(2023, 1, 31), 1);
    expect(d.getUTCDate()).toBe(28);
  });

  it('Mar 31 + 1 month → Apr 30', () => {
    const d = addMonths(utcDate(2025, 3, 31), 1);
    expect(d.getUTCMonth()).toBe(3); // Apr
    expect(d.getUTCDate()).toBe(30);
  });

  it('adds 48 months preserving the 15th', () => {
    const d = addMonths(utcDate(2024, 9, 15), 48);
    expect(d.getUTCFullYear()).toBe(2028);
    expect(d.getUTCMonth()).toBe(8); // Sep
    expect(d.getUTCDate()).toBe(15);
  });
});

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

describe('deriveVestingEvents — validation', () => {
  it('rejects non-positive share count', () => {
    const g = { ...base(0n) };
    expect(() => deriveVestingEvents(g, utcDate(2026, 1, 1))).toThrowError(
      /non_positive_share_count/,
    );
  });

  it('rejects zero total months', () => {
    const g = { ...base(wholeShares(100)), vestingTotalMonths: 0 };
    expect(() => deriveVestingEvents(g, utcDate(2026, 1, 1))).toThrowError(
      /total_months_out_of_range/,
    );
  });

  it('rejects total months over cap', () => {
    const g = { ...base(wholeShares(100)), vestingTotalMonths: 241 };
    expect(() => deriveVestingEvents(g, utcDate(2026, 1, 1))).toThrowError(
      /total_months_out_of_range/,
    );
  });

  it('rejects cliff > total', () => {
    const g = { ...base(wholeShares(100)), cliffMonths: 49 };
    expect(() => deriveVestingEvents(g, utcDate(2026, 1, 1))).toThrowError(/cliff_exceeds_total/);
  });
});

// ---------------------------------------------------------------------------
// Canonical RSU 30k / 48m / 12m cliff / monthly
// Mirrors Rust tests: sum_equals_total_for_standard_monthly_cliff,
// first_event_is_at_cliff_with_accumulated_portion.
// ---------------------------------------------------------------------------

describe('deriveVestingEvents — RSU-30k-48m-12m-monthly', () => {
  it('first event at the cliff = 12/48 * 30_000 = 7_500', () => {
    const events = deriveVestingEvents(base(wholeShares(30_000)), utcDate(2030, 1, 1));
    const first = events[0]!;
    expect(first.vestDate.getUTCFullYear()).toBe(2025);
    expect(first.vestDate.getUTCMonth()).toBe(8); // Sep
    expect(first.vestDate.getUTCDate()).toBe(15);
    expect(first.sharesVestedThisEventScaled).toBe(wholeShares(7_500));
    expect(first.cumulativeSharesVestedScaled).toBe(wholeShares(7_500));
  });

  it('sum of shares_vested_this_event equals share_count exactly', () => {
    const events = deriveVestingEvents(base(wholeShares(30_000)), utcDate(2030, 1, 1));
    const sum = events.reduce((acc, e) => acc + e.sharesVestedThisEventScaled, 0n);
    expect(sum).toBe(wholeShares(30_000));
    expect(events[events.length - 1]!.cumulativeSharesVestedScaled).toBe(sum);
  });

  it('no event before the cliff', () => {
    const events = deriveVestingEvents(base(wholeShares(48_000)), utcDate(2030, 1, 1));
    const cliffDate = utcDate(2025, 9, 15);
    for (const e of events) {
      expect(e.vestDate.getTime()).toBeGreaterThanOrEqual(cliffDate.getTime());
    }
  });

  it('produces 37 events (1 cliff + 36 monthly)', () => {
    const events = deriveVestingEvents(base(wholeShares(30_000)), utcDate(2030, 1, 1));
    expect(events.length).toBe(37);
  });
});

// ---------------------------------------------------------------------------
// Quarterly fixture (Rust: quarterly_events_are_exactly_three_months_apart)
// ---------------------------------------------------------------------------

describe('deriveVestingEvents — quarterly 12 months, no cliff', () => {
  it('emits 4 events exactly three months apart', () => {
    const g: GrantInput = {
      ...base(wholeShares(1_000)),
      cliffMonths: 0,
      vestingTotalMonths: 12,
      cadence: 'quarterly',
    };
    const events = deriveVestingEvents(g, utcDate(2030, 1, 1));
    expect(events.length).toBe(4);
    const dates = events.map((e) => ({
      y: e.vestDate.getUTCFullYear(),
      m: e.vestDate.getUTCMonth() + 1,
      d: e.vestDate.getUTCDate(),
    }));
    expect(dates[0]).toEqual({ y: 2024, m: 12, d: 15 });
    expect(dates[1]).toEqual({ y: 2025, m: 3, d: 15 });
    expect(dates[2]).toEqual({ y: 2025, m: 6, d: 15 });
    expect(dates[3]).toEqual({ y: 2025, m: 9, d: 15 });
  });

  it('sum equals total for 12_345 shares over 12 months quarterly', () => {
    const g: GrantInput = { ...base(wholeShares(12_345)), cadence: 'quarterly' };
    const events = deriveVestingEvents(g, utcDate(2030, 1, 1));
    const sum = events.reduce((acc, e) => acc + e.sharesVestedThisEventScaled, 0n);
    expect(sum).toBe(wholeShares(12_345));
  });
});

// ---------------------------------------------------------------------------
// Cliff == total → single final event
// ---------------------------------------------------------------------------

describe('deriveVestingEvents — cliff equals total', () => {
  it('emits a single event at the end with the full total', () => {
    const g: GrantInput = { ...base(wholeShares(1_000)), cliffMonths: 48 };
    const events = deriveVestingEvents(g, utcDate(2030, 1, 1));
    expect(events.length).toBe(1);
    const e = events[0]!;
    expect(e.sharesVestedThisEventScaled).toBe(wholeShares(1_000));
    expect(e.cumulativeSharesVestedScaled).toBe(wholeShares(1_000));
    expect(e.vestDate.getUTCFullYear()).toBe(2028);
    expect(e.vestDate.getUTCMonth()).toBe(8); // Sep
    expect(e.vestDate.getUTCDate()).toBe(15);
  });
});

// ---------------------------------------------------------------------------
// Fractional shares (1_234_567 scaled = 123.4567 whole)
// Rust: sum_equals_total_fractional.
// ---------------------------------------------------------------------------

describe('deriveVestingEvents — fractional', () => {
  it('sum equals total for 1_234_567 scaled', () => {
    const g = base(1_234_567n);
    const events = deriveVestingEvents(g, utcDate(2030, 1, 1));
    const sum = events.reduce((acc, e) => acc + e.sharesVestedThisEventScaled, 0n);
    expect(sum).toBe(1_234_567n);
  });
});

// ---------------------------------------------------------------------------
// Double-trigger states (AC-4.3.4)
// ---------------------------------------------------------------------------

describe('deriveVestingEvents — double-trigger states', () => {
  it('double_trigger + null liquidity → all past events are time_vested_awaiting_liquidity', () => {
    const g: GrantInput = {
      ...base(wholeShares(4_800)),
      doubleTrigger: true,
      liquidityEventDate: null,
    };
    const events = deriveVestingEvents(g, utcDate(2030, 1, 1));
    for (const e of events) expect(e.state).toBe('time_vested_awaiting_liquidity');
  });

  it('double_trigger + past liquidity → all past events are vested', () => {
    const g: GrantInput = {
      ...base(wholeShares(4_800)),
      doubleTrigger: true,
      liquidityEventDate: utcDate(2025, 1, 1),
    };
    const events = deriveVestingEvents(g, utcDate(2030, 1, 1));
    for (const e of events) expect(e.state).toBe('vested');
  });

  it('future events are upcoming regardless of double_trigger flag', () => {
    const g: GrantInput = {
      ...base(wholeShares(4_800)),
      doubleTrigger: true,
      liquidityEventDate: null,
    };
    // today = 2025-01-01 is before cliff (2025-09-15).
    const events = deriveVestingEvents(g, utcDate(2025, 1, 1));
    for (const e of events) expect(e.state).toBe('upcoming');
  });
});

// ---------------------------------------------------------------------------
// vestedToDate
// ---------------------------------------------------------------------------

describe('vestedToDate', () => {
  it('13/48 * 30_000 = 8_125 on 2025-10-15 (cliff + 1 month)', () => {
    const g = base(wholeShares(30_000));
    const events = deriveVestingEvents(g, utcDate(2025, 10, 15));
    const { vested, awaiting } = vestedToDate(events, utcDate(2025, 10, 15));
    expect(awaiting).toBe(0n);
    expect(vested).toBe(wholeShares(8_125));
  });

  it('double-trigger awaiting → awaiting accumulates, vested stays 0', () => {
    const g: GrantInput = {
      ...base(wholeShares(30_000)),
      doubleTrigger: true,
      liquidityEventDate: null,
    };
    const events = deriveVestingEvents(g, utcDate(2025, 10, 15));
    const { vested, awaiting } = vestedToDate(events, utcDate(2025, 10, 15));
    expect(vested).toBe(0n);
    expect(awaiting).toBe(wholeShares(8_125));
  });
});

// ---------------------------------------------------------------------------
// Determinism (AC-4.3.5)
// ---------------------------------------------------------------------------

describe('determinism', () => {
  it('same input produces same event list', () => {
    const g = base(wholeShares(30_000));
    const a = deriveVestingEvents(g, utcDate(2026, 3, 1));
    const b = deriveVestingEvents(g, utcDate(2026, 3, 1));
    expect(a.length).toBe(b.length);
    for (let i = 0; i < a.length; i++) {
      expect(a[i]!.vestDate.getTime()).toBe(b[i]!.vestDate.getTime());
      expect(a[i]!.sharesVestedThisEventScaled).toBe(b[i]!.sharesVestedThisEventScaled);
      expect(a[i]!.cumulativeSharesVestedScaled).toBe(b[i]!.cumulativeSharesVestedScaled);
      expect(a[i]!.state).toBe(b[i]!.state);
    }
  });
});

// ---------------------------------------------------------------------------
// Parameter sweep — mirrors Rust `sweep_sum_equals_total`.
// ---------------------------------------------------------------------------

describe('parameter sweep', () => {
  it('sum equals share_count across a broad sweep', () => {
    const sharesCases: bigint[] = [
      1n,
      SHARES_SCALE,
      wholeShares(1),
      wholeShares(7),
      wholeShares(100),
      wholeShares(12_345),
      wholeShares(1_000_000),
      1_234_567n,
    ];
    const monthsCases = [1, 3, 12, 24, 36, 48, 60, 120, 240];
    const cliffCases = [0, 1, 3, 6, 12];
    for (const shareCount of sharesCases) {
      for (const totalMonths of monthsCases) {
        for (const cliff of cliffCases) {
          if (cliff > totalMonths) continue;
          for (const cadence of ['monthly', 'quarterly'] as const) {
            const g: GrantInput = {
              shareCountScaled: shareCount,
              vestingStart: utcDate(2024, 1, 15),
              vestingTotalMonths: totalMonths,
              cliffMonths: cliff,
              cadence,
              doubleTrigger: false,
              liquidityEventDate: null,
            };
            const events = deriveVestingEvents(g, utcDate(2030, 1, 1));
            const sum = events.reduce((acc, e) => acc + e.sharesVestedThisEventScaled, 0n);
            expect(sum).toBe(shareCount);
            let prev = 0n;
            for (const e of events) {
              expect(e.cumulativeSharesVestedScaled).toBeGreaterThanOrEqual(prev);
              prev = e.cumulativeSharesVestedScaled;
            }
          }
        }
      }
    }
  });
});
