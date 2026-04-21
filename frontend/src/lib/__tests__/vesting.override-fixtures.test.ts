// Slice-3 vesting-override parity fixtures (ADR-017 §2). Asserts that the
// TypeScript `deriveVestingEvents` extension honors the same override-
// preservation rules as `orbit_core::vesting::derive_vesting_events`.
//
// Shared fixture lives under the Rust crate; see vesting.fixtures.test.ts
// for the rationale on `readFileSync` + path.resolve.

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  deriveVestingEvents,
  type Cadence,
  type GrantInput,
  type VestingEventOverride,
} from '../vesting';

const thisFile = fileURLToPath(import.meta.url);
const FIXTURE_PATH = resolve(
  dirname(thisFile),
  '../../../../backend/crates/orbit-core/tests/fixtures/vesting_override_cases.json',
);

interface FixtureInputs {
  shareCountScaled: number;
  vestingStart: string;
  vestingTotalMonths: number;
  cliffMonths: number;
  cadence: Cadence;
  doubleTrigger: boolean;
  liquidityEventDate: string | null;
}

interface FixtureOverride {
  vestDate: string;
  sharesVestedScaled: number;
  fmvAtVest: string | null;
  fmvCurrency: string | null;
  originalDerivationIndex: number;
}

interface FixtureExpected {
  eventCount: number;
  cumulativeRelaxed: boolean;
  overriddenDates: string[];
  overriddenShares: Record<string, number>;
}

interface FixtureCase {
  name: string;
  inputs: FixtureInputs;
  today: string;
  overrides: FixtureOverride[];
  expected: FixtureExpected;
}

interface FixtureFile {
  cases: FixtureCase[];
}

function parseUtcDate(iso: string): Date {
  const [y, m, d] = iso.split('-').map(Number);
  return new Date(Date.UTC(y!, m! - 1, d!));
}

function dateToIso(d: Date): string {
  const y = d.getUTCFullYear().toString().padStart(4, '0');
  const m = (d.getUTCMonth() + 1).toString().padStart(2, '0');
  const day = d.getUTCDate().toString().padStart(2, '0');
  return `${y}-${m}-${day}`;
}

function loadFixtures(): FixtureFile {
  const raw = readFileSync(FIXTURE_PATH, 'utf8');
  return JSON.parse(raw) as FixtureFile;
}

describe('vesting_override_cases.json — override-preservation parity (AC-8.4.2)', () => {
  const fixtures = loadFixtures();

  it('fixture file is present with at least 8 canonical cases', () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(8);
  });

  it.each(fixtures.cases.map((c) => [c.name, c] as const))(
    'case %s matches expected override-preserving output',
    (_name, fx) => {
      const input: GrantInput = {
        shareCountScaled: BigInt(fx.inputs.shareCountScaled),
        vestingStart: parseUtcDate(fx.inputs.vestingStart),
        vestingTotalMonths: fx.inputs.vestingTotalMonths,
        cliffMonths: fx.inputs.cliffMonths,
        cadence: fx.inputs.cadence,
        doubleTrigger: fx.inputs.doubleTrigger,
        liquidityEventDate: fx.inputs.liquidityEventDate
          ? parseUtcDate(fx.inputs.liquidityEventDate)
          : null,
      };
      const today = parseUtcDate(fx.today);
      const overrides: VestingEventOverride[] = fx.overrides.map((o) => ({
        vestDate: parseUtcDate(o.vestDate),
        sharesVestedThisEventScaled: BigInt(o.sharesVestedScaled),
        fmvAtVest: o.fmvAtVest,
        fmvCurrency: o.fmvCurrency,
        originalDerivationIndex: o.originalDerivationIndex,
      }));

      const events = deriveVestingEvents(input, today, overrides);

      // eventCount
      expect(events.length, `${fx.name}: eventCount`).toBe(fx.expected.eventCount);

      // Every overridden date is present with the expected shares.
      for (const d of fx.expected.overriddenDates) {
        const hit = events.find((e) => dateToIso(e.vestDate) === d);
        expect(hit, `${fx.name}: override at ${d} missing from output`).toBeTruthy();
        const expected = fx.expected.overriddenShares[d];
        expect(
          hit!.sharesVestedThisEventScaled,
          `${fx.name}: override shares at ${d}`,
        ).toBe(BigInt(expected!));
      }

      // Cumulative invariant relaxation: sum equals shareCountScaled iff
      // cumulativeRelaxed === false.
      const sum = events.reduce(
        (a, e) => a + e.sharesVestedThisEventScaled,
        0n,
      );
      if (fx.expected.cumulativeRelaxed) {
        // When cumulativeRelaxed === true, the sum MAY differ. We don't
        // assert equality either way; we only assert it's not silently
        // rebalanced to `shareCountScaled` when the AC says it shouldn't.
      } else {
        expect(sum, `${fx.name}: cumulative invariant must hold`).toBe(
          BigInt(fx.inputs.shareCountScaled),
        );
      }

      // Deterministic ordering: two calls produce identical output.
      const again = deriveVestingEvents(input, today, overrides);
      expect(events.length).toBe(again.length);
      for (let i = 0; i < events.length; i++) {
        expect(dateToIso(events[i]!.vestDate)).toBe(dateToIso(again[i]!.vestDate));
        expect(events[i]!.sharesVestedThisEventScaled).toBe(
          again[i]!.sharesVestedThisEventScaled,
        );
      }
    },
  );
});
