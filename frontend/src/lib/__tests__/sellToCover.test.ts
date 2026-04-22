// Shared-fixture parity test for `sellToCover.compute`.
//
// Reads the same JSON the Rust suite at
// `backend/crates/orbit-core/tests/sell_to_cover_fixtures.rs` consumes;
// any drift between the Rust and TS implementations of `compute`
// (AC-6.*) fails both suites. Closes the ADR-018 §Consequences
// "client/server drift-risk" mitigation for the sell-to-cover
// algorithm.
//
// Why readFileSync + path.resolve(): matches the pattern from
// `vesting.fixtures.test.ts` — the fixture lives under the Rust
// crate's `tests/` directory, outside the Vite/Vitest source tree, so
// a static import would force a copy and re-introduce the drift.

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { compute, isComputeError, scaledDecimalString } from '../sellToCover';

const thisFile = fileURLToPath(import.meta.url);
const FIXTURE_PATH = resolve(
  dirname(thisFile),
  '../../../../backend/crates/orbit-core/tests/fixtures/sell_to_cover_cases.json',
);

interface FixtureInput {
  fmvAtVestScaled: number;
  sharesVestedScaled: number;
  taxWithholdingPercentScaled: number;
  shareSellPriceScaled: number;
}

interface FixtureExpected {
  grossAmountScaled: number;
  sharesSoldForTaxesScaled: number;
  netSharesDeliveredScaled: number;
  cashWithheldScaled: number;
}

interface FixtureCase {
  name: string;
  input: FixtureInput;
  expected?: FixtureExpected;
  expectedError?: 'NegativeNetShares' | 'ZeroSellPriceWithPositiveTax';
}

interface FixtureFile {
  cases: FixtureCase[];
}

function loadFixtures(): FixtureFile {
  const raw = readFileSync(FIXTURE_PATH, 'utf8');
  return JSON.parse(raw) as FixtureFile;
}

/**
 * The fixture encodes the tax fraction as a scaled integer (e.g.
 * `4500` for 0.4500). The TS `compute` accepts the fraction as a
 * decimal string to match the DTO wire shape; translate here so the
 * fixture can stay single-source.
 */
function taxScaledToDecimalString(scaled: number): string {
  return scaledDecimalString(BigInt(scaled));
}

describe('sell_to_cover_cases.json — cross-implementation parity (ADR-018 §4)', () => {
  const fixtures = loadFixtures();

  it('fixture file is present and contains at least 10 canonical cases', () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(10);
  });

  it.each(fixtures.cases.map((c) => [c.name, c] as const))(
    'case %s matches the Rust oracle bitwise',
    (_name, fx) => {
      const out = compute({
        fmvAtVestScaled: BigInt(fx.input.fmvAtVestScaled),
        sharesVestedScaled: BigInt(fx.input.sharesVestedScaled),
        taxWithholdingPercent: taxScaledToDecimalString(fx.input.taxWithholdingPercentScaled),
        shareSellPriceScaled: BigInt(fx.input.shareSellPriceScaled),
      });

      if (fx.expectedError) {
        expect(isComputeError(out), `${fx.name}: expected error`).toBe(true);
        if (isComputeError(out)) {
          const expectedKind =
            fx.expectedError === 'NegativeNetShares'
              ? 'negativeNetShares'
              : 'zeroSellPriceWithPositiveTax';
          expect(out.kind, `${fx.name}: error kind`).toBe(expectedKind);
        }
        return;
      }

      expect(isComputeError(out), `${fx.name}: unexpected error`).toBe(false);
      if (isComputeError(out)) return; // narrowing

      const exp = fx.expected!;
      expect(out.grossAmountScaled, `${fx.name}: grossAmountScaled`).toBe(
        BigInt(exp.grossAmountScaled),
      );
      expect(out.sharesSoldForTaxesScaled, `${fx.name}: sharesSoldForTaxesScaled`).toBe(
        BigInt(exp.sharesSoldForTaxesScaled),
      );
      expect(out.netSharesDeliveredScaled, `${fx.name}: netSharesDeliveredScaled`).toBe(
        BigInt(exp.netSharesDeliveredScaled),
      );
      expect(out.cashWithheldScaled, `${fx.name}: cashWithheldScaled`).toBe(
        BigInt(exp.cashWithheldScaled),
      );
    },
  );
});
