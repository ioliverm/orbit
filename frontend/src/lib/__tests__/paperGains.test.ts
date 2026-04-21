// Paper-gains parity fixture tests (ADR-017 §5). Asserts the TS
// implementation matches the Rust `compute_paper_gains` on every case
// in `paper_gains_cases.json`.

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  computePaperGains,
  type EsppPurchaseForPaperGains,
  type GrantForPaperGains,
  type PaperGainsInput,
  type VestingEventForPaperGains,
} from '../paperGains';
import type { VestingState } from '../vesting';

const thisFile = fileURLToPath(import.meta.url);
const FIXTURE_PATH = resolve(
  dirname(thisFile),
  '../../../../backend/crates/orbit-core/tests/fixtures/paper_gains_cases.json',
);

interface FixtureVestingEvent {
  vestDate: string;
  state: VestingState;
  sharesVestedScaled: number;
  fmvAtVest: string | null;
  fmvCurrency: string | null;
}

interface FixtureEspp {
  purchaseDate: string;
  sharesPurchasedScaled: number;
  fmvAtPurchase: string;
  currency: string;
}

interface FixtureGrant {
  id: string;
  instrument: string;
  nativeCurrency: string;
  ticker: string | null;
  doubleTrigger: boolean;
  liquidityEventDate: string | null;
  vestingEvents: FixtureVestingEvent[];
  esppPurchases: FixtureEspp[];
}

interface FixtureTicker {
  ticker: string;
  price: string;
  currency: string;
}

interface FixtureOverride {
  grantId: string;
  price: string;
  currency: string;
}

interface FixtureExpected {
  completeIds: string[];
  incompleteGrants: string[];
  hasCombinedBand: boolean;
}

interface FixtureCase {
  name: string;
  today: string;
  fxRateEurNative: string | null;
  grants: FixtureGrant[];
  tickerPrices: FixtureTicker[];
  grantOverrides: FixtureOverride[];
  expected: FixtureExpected;
}

interface FixtureFile {
  cases: FixtureCase[];
}

function parseUtcDate(iso: string): Date {
  const [y, m, d] = iso.split('-').map(Number);
  return new Date(Date.UTC(y!, m! - 1, d!));
}

function loadFixtures(): FixtureFile {
  const raw = readFileSync(FIXTURE_PATH, 'utf8');
  return JSON.parse(raw) as FixtureFile;
}

describe('paper_gains_cases.json — TS/Rust parity', () => {
  const fixtures = loadFixtures();

  it('fixture file is present with at least 5 cases', () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(5);
  });

  it.each(fixtures.cases.map((c) => [c.name, c] as const))(
    'case %s matches expected output',
    (_name, fx) => {
      const grants: GrantForPaperGains[] = fx.grants.map((g) => ({
        id: g.id,
        instrument: g.instrument,
        nativeCurrency: g.nativeCurrency,
        ticker: g.ticker,
        doubleTrigger: g.doubleTrigger,
        liquidityEventDate: g.liquidityEventDate
          ? parseUtcDate(g.liquidityEventDate)
          : null,
        vestingEvents: g.vestingEvents.map(
          (e): VestingEventForPaperGains => ({
            vestDate: parseUtcDate(e.vestDate),
            state: e.state,
            sharesVestedScaled: BigInt(e.sharesVestedScaled),
            fmvAtVest: e.fmvAtVest,
            fmvCurrency: e.fmvCurrency,
          }),
        ),
        esppPurchases: g.esppPurchases.map(
          (p): EsppPurchaseForPaperGains => ({
            purchaseDate: parseUtcDate(p.purchaseDate),
            sharesPurchasedScaled: BigInt(p.sharesPurchasedScaled),
            fmvAtPurchase: p.fmvAtPurchase,
            currency: p.currency,
          }),
        ),
      }));

      const input: PaperGainsInput = {
        grants,
        tickerPrices: fx.tickerPrices.map((t) => ({
          ticker: t.ticker,
          price: t.price,
          currency: t.currency,
        })),
        grantOverrides: fx.grantOverrides.map((o) => ({
          grantId: o.grantId,
          price: o.price,
          currency: o.currency,
        })),
        fxRateEurNative: fx.fxRateEurNative,
        today: parseUtcDate(fx.today),
      };

      const result = computePaperGains(input);

      // Complete IDs match (order-insensitive).
      const completeIds = result.perGrant
        .filter((p) => p.complete)
        .map((p) => p.grantId)
        .sort();
      expect(completeIds, `${fx.name}: completeIds`).toEqual(
        [...fx.expected.completeIds].sort(),
      );

      // Incomplete IDs match (order-insensitive).
      expect(
        [...result.incompleteGrants].sort(),
        `${fx.name}: incompleteGrants`,
      ).toEqual([...fx.expected.incompleteGrants].sort());

      // combinedEurBand presence.
      if (fx.expected.hasCombinedBand) {
        expect(result.combinedEurBand, `${fx.name}: combinedEurBand`).not.toBeNull();
      } else {
        expect(result.combinedEurBand, `${fx.name}: combinedEurBand`).toBeNull();
      }
    },
  );
});
