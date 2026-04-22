// TypeScript mirror of `backend/crates/orbit-core/src/sell_to_cover.rs`
// (Slice 3b T38 + T39, ADR-018 §4).
//
// This module is used by the vesting-event dialog's "Valores derivados"
// panel (AC-7.2.*) to render the live breakdown as the user edits. The
// server is still the source of truth — on save the backend recomputes
// the same four outputs via `orbit_core::sell_to_cover::compute` and
// returns them on the DTO. We mirror the algorithm bitwise so the
// dialog's derived panel matches the server's eventual values.
//
// Shared fixture `backend/crates/orbit-core/tests/fixtures/
// sell_to_cover_cases.json` is read by both the Rust suite and
// `frontend/src/lib/__tests__/sellToCover.test.ts` to pin parity.
//
// Numeric model
// -------------
// Mirrors `crate::vesting::Shares` semantics: scaled bigint in units of
// 1/10_000 (see vesting.ts). Internal arithmetic stays in bigint so the
// same i128-equivalent intermediate values land without drift. Rounding
// on `shares_sold_for_taxes` is integer ceiling at 4 dp — the Spanish
// withholding-practice convention (broker sells UP so remittance does
// not under-collect, ADR-018 §10.2).
//
// API shape
// ---------
// The public input accepts the tax fraction as a decimal STRING (e.g.
// "0.4500") because the dialog stores the user-facing value as a string
// and the DTO wire format is also a string. We parse it to a scaled
// bigint internally via `parseScaledDecimal`, matching
// `parse_scaled_numeric` in the Rust handler.

import { SHARES_SCALE } from './vesting';

export interface SellToCoverInput {
  /** Per-share FMV at vest, scaled (one unit = 1/SHARES_SCALE of the
   *  native currency — matches vesting's `Shares` convention). */
  fmvAtVestScaled: bigint;
  /** Scaled share count (one unit = 1/SHARES_SCALE of a share). */
  sharesVestedScaled: bigint;
  /** Tax-withholding fraction as a decimal string in `[0, 1]`
   *  (e.g. `"0.4500"`). The profile-form value at `/preferences` is
   *  already stored in this shape; the dialog's percent input is
   *  converted from user-facing percent (`45`) to the fraction
   *  (`0.45`) before calling `compute`. */
  taxWithholdingPercent: string;
  /** Per-share sell price at vest, scaled. */
  shareSellPriceScaled: bigint;
}

export interface SellToCoverResult {
  /** `fmv × shares_vested`, scaled. Income recognition amount in the
   *  FMV currency. */
  grossAmountScaled: bigint;
  /** `ceil_4dp( tax × gross / sell_price )`, scaled. Number of shares
   *  the broker sells to cover the withholding obligation. */
  sharesSoldForTaxesScaled: bigint;
  /** `shares_vested − shares_sold`, scaled. The shares actually
   *  delivered to the user. */
  netSharesDeliveredScaled: bigint;
  /** `shares_sold × sell_price`, scaled. Cash the broker remits for
   *  withholding; `>= tax × gross` by the ceiling on `shares_sold`. */
  cashWithheldScaled: bigint;
}

export type ComputeError =
  /** `shares_vested − shares_sold_for_taxes` goes negative.
   *  Fires when `tax = 1` AND `sell_price < fmv`. */
  | { kind: 'negativeNetShares' }
  /** `share_sell_price == 0` with `tax > 0`. */
  | { kind: 'zeroSellPriceWithPositiveTax' };

export function isComputeError(v: SellToCoverResult | ComputeError): v is ComputeError {
  return (v as ComputeError).kind !== undefined;
}

/**
 * Compute the sell-to-cover breakdown for one vesting event. Pure —
 * no DOM, no async, no I/O.
 *
 * Mirrors `orbit_core::sell_to_cover::compute`. See the Rust module
 * for the derivation of each step and the overflow argument.
 */
export function compute(input: SellToCoverInput): SellToCoverResult | ComputeError {
  const fmv = input.fmvAtVestScaled;
  const shares = input.sharesVestedScaled;
  const sellPrice = input.shareSellPriceScaled;
  const scale = SHARES_SCALE;

  const taxScaled = parseScaledDecimal(input.taxWithholdingPercent);
  if (taxScaled === null) {
    // Unparsable tax fraction → treat as zero (no withholding). The
    // backend validator rejects malformed strings before the pure
    // function ever sees them, so this branch is defensive.
    return {
      grossAmountScaled: (fmv * shares) / scale,
      sharesSoldForTaxesScaled: 0n,
      netSharesDeliveredScaled: shares,
      cashWithheldScaled: 0n,
    };
  }

  // Edge: zero-vest short-circuit. All zeros regardless of tax / FMV /
  // sell_price (AC-6.4.4). Also side-steps the zero-sell-price guard.
  if (shares === 0n) {
    return {
      grossAmountScaled: 0n,
      sharesSoldForTaxesScaled: 0n,
      netSharesDeliveredScaled: 0n,
      cashWithheldScaled: 0n,
    };
  }

  // Step 1 — gross = fmv × shares. Both operands scaled by SHARES_SCALE,
  // product scaled by SHARES_SCALE^2; divide once to bring back to a
  // single-scale value.
  const gross = (fmv * shares) / scale;

  // Edge: zero tax short-circuit. No shares sold; user receives every
  // vested share verbatim; no cash withheld. Valid regardless of
  // sell_price (including zero — we don't need to divide).
  if (taxScaled === 0n) {
    return {
      grossAmountScaled: gross,
      sharesSoldForTaxesScaled: 0n,
      netSharesDeliveredScaled: shares,
      cashWithheldScaled: 0n,
    };
  }

  // Step 2 — defensive: zero sell price with positive tax.
  if (sellPrice === 0n) {
    return { kind: 'zeroSellPriceWithPositiveTax' };
  }

  // Step 3 — shares_sold = ceil( (tax × gross) / sell_price ). All
  // three operands are SHARES_SCALE-scaled; see Rust module for the
  // algebra that reduces to `(tax × gross) / sell_price` in scaled-
  // bigint space.
  const numerator = taxScaled * gross;
  const sharesSoldScaled = ceilDivBigInt(numerator, sellPrice);

  // Step 4 — net shares delivered.
  const netScaled = shares - sharesSoldScaled;
  if (netScaled < 0n) {
    return { kind: 'negativeNetShares' };
  }

  // Step 5 — cash withheld = shares_sold × sell_price. Both scaled;
  // product double-scaled; divide once.
  const cashWithheld = (sharesSoldScaled * sellPrice) / scale;

  return {
    grossAmountScaled: gross,
    sharesSoldForTaxesScaled: sharesSoldScaled,
    netSharesDeliveredScaled: netScaled,
    cashWithheldScaled: cashWithheld,
  };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Integer ceiling division on non-negative bigint values. Returns
 * `ceil(a / b)`. Mirrors `ceil_div_i128` in the Rust module. Caller
 * must guarantee `b > 0` (zero is caught earlier in `compute`).
 */
function ceilDivBigInt(a: bigint, b: bigint): bigint {
  // `(a + b - 1) / b` is the standard non-negative ceiling-div idiom.
  return (a + b - 1n) / b;
}

/**
 * Parse a decimal string like `"0.45"` / `"0.4500"` / `"1.0000"` into
 * a scaled bigint in units of 1/SHARES_SCALE (so `"0.4500"` → `4500n`).
 * Truncates (not rounds) beyond 4 decimal places. Returns `null` on
 * parse failure.
 *
 * Mirrors `parse_scaled_numeric` in
 * `orbit-api/handlers/vesting_events.rs`.
 */
export function parseScaledDecimal(raw: string): bigint | null {
  const trimmed = raw.trim();
  if (trimmed === '') return null;
  let sign = 1n;
  let rest = trimmed;
  if (rest.startsWith('+')) rest = rest.slice(1);
  else if (rest.startsWith('-')) {
    sign = -1n;
    rest = rest.slice(1);
  }
  if (rest === '') return null;
  const dot = rest.indexOf('.');
  const intPart = dot === -1 ? rest : rest.slice(0, dot);
  let fracPart = dot === -1 ? '' : rest.slice(dot + 1);
  if (!/^\d*$/.test(intPart) || !/^\d*$/.test(fracPart)) return null;
  if (intPart === '' && fracPart === '') return null;
  if (fracPart.length > 4) fracPart = fracPart.slice(0, 4);
  while (fracPart.length < 4) fracPart += '0';
  const intVal = intPart === '' ? 0n : BigInt(intPart);
  const fracVal = BigInt(fracPart);
  return sign * (intVal * SHARES_SCALE + fracVal);
}

/**
 * Render a scaled bigint as a decimal string with 4 dp (matches the
 * NUMERIC(20,4)/(5,4) wire convention). Preserves sign. Mirrors
 * `scaled_decimal_string` in the Rust handler.
 */
export function scaledDecimalString(v: bigint): string {
  const sign = v < 0n ? '-' : '';
  const abs = v < 0n ? -v : v;
  const intPart = abs / SHARES_SCALE;
  const fracPart = abs % SHARES_SCALE;
  return `${sign}${intPart.toString()}.${fracPart.toString().padStart(4, '0')}`;
}
