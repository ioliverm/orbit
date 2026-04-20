// Shared-fixture parity test for the stacked-grants algorithm
// (Slice 2 T22, ADR-016 §4). Reads the same JSON file that
// `backend/crates/orbit-core/tests/stacked_grants_fixtures.rs` consumes;
// any drift between the Rust and TS implementations of
// `stackAllGrants` (AC-8.2.8) fails both suites.
//
// Pattern mirrors `vesting.fixtures.test.ts`: `readFileSync` against the
// Rust crate's `tests/fixtures/` directory keeps a single source of
// truth instead of copying the fixture into `frontend/`.

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
import {
  stackAllGrants,
  stackCumulativeForEmployer,
  type GrantMeta,
  type VestingEvent,
} from '../stackedGrants';

const thisFile = fileURLToPath(import.meta.url);
const FIXTURE_PATH = resolve(
  dirname(thisFile),
  '../../../../backend/crates/orbit-core/tests/fixtures/stacked_grants_cases.json',
);

interface FixtureVesting {
  shareCountScaled: number;
  vestingStart: string;
  vestingTotalMonths: number;
  cliffMonths: number;
  cadence: Cadence;
  doubleTrigger: boolean;
  liquidityEventDate: string | null;
}

interface FixtureGrant {
  id: string;
  employerName: string;
  instrument: string;
  createdAt: string;
  vesting: FixtureVesting;
}

interface FixtureExpected {
  employerCount: number;
  employerNames: string[];
  employerGrantIdCounts: number[];
  combinedPointCount: number;
  finalCumulativeVestedScaled: number;
  finalCumulativeAwaitingScaled: number;
  instrumentsPresentInBreakdown?: string[];
}

interface FixtureCase {
  name: string;
  today: string;
  grants: FixtureGrant[];
  expected: FixtureExpected;
}

interface FixtureFile {
  cases: FixtureCase[];
}

function parseUtcDate(iso: string): Date {
  const parts = iso.split('-').map(Number);
  return new Date(Date.UTC(parts[0]!, parts[1]! - 1, parts[2]!));
}

function loadFixtures(): FixtureFile {
  const raw = readFileSync(FIXTURE_PATH, 'utf8');
  return JSON.parse(raw) as FixtureFile;
}

function eventsForGrant(g: FixtureGrant, today: string): VestingEvent[] {
  const input: GrantInput = {
    shareCountScaled: BigInt(g.vesting.shareCountScaled),
    vestingStart: parseUtcDate(g.vesting.vestingStart),
    vestingTotalMonths: g.vesting.vestingTotalMonths,
    cliffMonths: g.vesting.cliffMonths,
    cadence: g.vesting.cadence,
    doubleTrigger: g.vesting.doubleTrigger,
    liquidityEventDate: g.vesting.liquidityEventDate
      ? parseUtcDate(g.vesting.liquidityEventDate)
      : null,
  };
  const derived = deriveVestingEvents(input, parseUtcDate(today));
  return derived.map((e) => ({
    grantId: g.id,
    vestDate: isoDate(e.vestDate),
    sharesVestedThisEventScaled: e.sharesVestedThisEventScaled,
    cumulativeSharesVestedScaled: e.cumulativeSharesVestedScaled,
    state: e.state as VestingState,
  }));
}

function isoDate(d: Date): string {
  const y = d.getUTCFullYear().toString().padStart(4, '0');
  const m = (d.getUTCMonth() + 1).toString().padStart(2, '0');
  const day = d.getUTCDate().toString().padStart(2, '0');
  return `${y}-${m}-${day}`;
}

describe('stacked_grants_cases.json — cross-implementation parity (AC-8.2.8)', () => {
  const fixtures = loadFixtures();

  it('fixture file is present and contains at least one canonical case', () => {
    expect(fixtures.cases.length).toBeGreaterThan(0);
  });

  it.each(fixtures.cases.map((c) => [c.name, c] as const))(
    'case %s matches expected stacked-dashboard output',
    (_name, fx) => {
      const metas: GrantMeta[] = fx.grants.map((g) => ({
        id: g.id,
        employerName: g.employerName,
        instrument: g.instrument,
        createdAt: g.createdAt,
      }));
      const events = fx.grants.flatMap((g) => eventsForGrant(g, fx.today));
      const out = stackAllGrants(metas, events);

      // employerCount
      expect(out.byEmployer.length, `${fx.name}: employerCount`).toBe(
        fx.expected.employerCount,
      );

      // employerNames (sorted ascending by display name per the Rust impl)
      expect(
        out.byEmployer.map((e) => e.employerName),
        `${fx.name}: employerNames`,
      ).toEqual(fx.expected.employerNames);

      // per-employer grantId counts
      expect(
        out.byEmployer.map((e) => e.grantIds.length),
        `${fx.name}: employerGrantIdCounts`,
      ).toEqual(fx.expected.employerGrantIdCounts);

      // combinedPointCount
      expect(out.combined.length, `${fx.name}: combinedPointCount`).toBe(
        fx.expected.combinedPointCount,
      );

      // Final envelope sums.
      const last = out.combined[out.combined.length - 1]!;
      expect(
        last.cumulativeVested,
        `${fx.name}: finalCumulativeVestedScaled`,
      ).toBe(BigInt(fx.expected.finalCumulativeVestedScaled));
      expect(
        last.cumulativeAwaitingLiquidity,
        `${fx.name}: finalCumulativeAwaitingScaled`,
      ).toBe(BigInt(fx.expected.finalCumulativeAwaitingScaled));

      // Optional: instruments that must appear in at least one breakdown row.
      if (fx.expected.instrumentsPresentInBreakdown) {
        const seen = new Set<string>();
        for (const es of out.byEmployer) {
          for (const p of es.points) {
            for (const b of p.perGrantBreakdown) seen.add(b.instrument);
          }
        }
        for (const expected of fx.expected.instrumentsPresentInBreakdown) {
          expect(seen, `${fx.name}: instrument ${expected} in breakdown`).toContain(
            expected,
          );
        }
      }
    },
  );

  it('stackCumulativeForEmployer for a single employer matches the per-employer stack from stackAllGrants', () => {
    const fx = fixtures.cases.find(
      (c) => c.name === 'two-rsus-same-employer-case-insensitive-merge',
    )!;
    const metas: GrantMeta[] = fx.grants.map((g) => ({
      id: g.id,
      employerName: g.employerName,
      instrument: g.instrument,
      createdAt: g.createdAt,
    }));
    const events = fx.grants.flatMap((g) => eventsForGrant(g, fx.today));
    const viaAll = stackAllGrants(metas, events);
    const viaEmp = stackCumulativeForEmployer(
      viaAll.byEmployer[0]!.employerName,
      metas,
      events,
    );
    expect(viaEmp).toEqual(viaAll.byEmployer[0]!.points);
  });
});
