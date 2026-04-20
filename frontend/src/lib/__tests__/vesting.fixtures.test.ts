// Shared-fixture parity test. Reads the same JSON file that
// `backend/crates/orbit-core/tests/vesting_fixtures.rs` consumes; any drift
// between the Rust and TS implementations of `deriveVestingEvents`
// (AC-4.3.5) fails both suites.
//
// Closes the ADR-014 §Consequences "client/server drift-risk" mitigation.
//
// Why readFileSync + path.resolve(): the fixture lives under the Rust
// crate's `tests/` directory, which is not inside the Vite/Vitest source
// tree. A static `import` would force a copy into `frontend/`, which
// re-introduces the drift we're trying to prevent. Reading at test time
// keeps a single source of truth.

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  deriveVestingEvents,
  type Cadence,
  type GrantInput,
  type VestingState,
} from '../vesting';

const thisFile = fileURLToPath(import.meta.url);
const FIXTURE_PATH = resolve(
  dirname(thisFile),
  '../../../../backend/crates/orbit-core/tests/fixtures/vesting_cases.json',
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

interface FixtureExpected {
  eventCount: number;
  sumScaled: number;
  firstVestDate: string;
  lastVestDate: string;
  firstSharesScaled?: number;
  lastCumulativeScaled: number;
  allStates: VestingState[];
  eventDates?: string[];
}

interface FixtureCase {
  name: string;
  inputs: FixtureInputs;
  today: string;
  expected: FixtureExpected;
}

interface FixtureFile {
  cases: FixtureCase[];
}

function parseUtcDate(iso: string): Date {
  // `YYYY-MM-DD` — interpret as UTC midnight, matching the Rust NaiveDate
  // semantics used by the backend.
  const parts = iso.split('-').map(Number);
  const y = parts[0]!;
  const m = parts[1]!;
  const d = parts[2]!;
  return new Date(Date.UTC(y, m - 1, d));
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

describe('vesting_cases.json — cross-implementation parity (AC-4.3.5)', () => {
  const fixtures = loadFixtures();

  it('fixture file is present and contains at least 10 canonical cases', () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(10);
  });

  it.each(fixtures.cases.map((c) => [c.name, c] as const))(
    'case %s matches expected derivation output',
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
      const events = deriveVestingEvents(input, today);

      // eventCount
      expect(events.length, `${fx.name}: eventCount`).toBe(fx.expected.eventCount);

      // sumScaled
      const sum = events.reduce((a, e) => a + e.sharesVestedThisEventScaled, 0n);
      expect(sum, `${fx.name}: sumScaled`).toBe(BigInt(fx.expected.sumScaled));

      // first / last dates
      const first = events[0]!;
      const last = events[events.length - 1]!;
      expect(dateToIso(first.vestDate), `${fx.name}: firstVestDate`).toBe(
        fx.expected.firstVestDate,
      );
      expect(dateToIso(last.vestDate), `${fx.name}: lastVestDate`).toBe(fx.expected.lastVestDate);

      if (fx.expected.firstSharesScaled !== undefined) {
        expect(first.sharesVestedThisEventScaled, `${fx.name}: firstSharesScaled`).toBe(
          BigInt(fx.expected.firstSharesScaled),
        );
      }

      expect(last.cumulativeSharesVestedScaled, `${fx.name}: lastCumulativeScaled`).toBe(
        BigInt(fx.expected.lastCumulativeScaled),
      );

      // allStates: every state must be in the allowed set.
      for (const e of events) {
        expect(fx.expected.allStates, `${fx.name}: state ${e.state}`).toContain(e.state);
      }

      // eventDates (optional): exact sequence match.
      if (fx.expected.eventDates) {
        const actual = events.map((e) => dateToIso(e.vestDate));
        expect(actual, `${fx.name}: eventDates`).toEqual(fx.expected.eventDates);
      }
    },
  );
});
