// TypeScript parity mirror of `backend/crates/orbit-core/src/paper_gains.rs`
// (ADR-017 §5).
//
// The dashboard paper-gains tile does not itself call this function — the
// authoritative aggregate is produced server-side by
// `GET /api/v1/dashboard/paper-gains`. This mirror exists for two reasons:
//
//   1. Shared fixture (`paper_gains_cases.json`) — bit-identical output
//      between this function and the Rust implementation is the drift
//      guard-rail, same discipline as Slice-1 vesting + Slice-2 stacked.
//   2. If a future Slice adds a client-only preview (e.g., "what would
//      paper gains look like if I overrode this FMV?"), the pure function
//      is ready.
//
// Decimal model: same as the Rust side — string decimals at the edge,
// f64 internally. The multiplicative chain is short (price × shares ×
// fx × spread) and well below f64's precision budget for Slice-3 values.

import type { VestingState } from './vesting';
import { SHARES_SCALE } from './vesting';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface VestingEventForPaperGains {
  vestDate: Date;
  state: VestingState;
  /** Scaled i64 (1/10_000ths of a share). */
  sharesVestedScaled: bigint;
  fmvAtVest: string | null;
  fmvCurrency: string | null;
}

export interface EsppPurchaseForPaperGains {
  purchaseDate: Date;
  sharesPurchasedScaled: bigint;
  fmvAtPurchase: string;
  currency: string;
}

export interface GrantForPaperGains {
  id: string;
  instrument: string;
  nativeCurrency: string;
  ticker: string | null;
  doubleTrigger: boolean;
  liquidityEventDate: Date | null;
  vestingEvents: VestingEventForPaperGains[];
  esppPurchases: EsppPurchaseForPaperGains[];
}

export interface TickerPriceForPaperGains {
  ticker: string;
  price: string;
  currency: string;
}

export interface GrantPriceOverrideForPaperGains {
  grantId: string;
  price: string;
  currency: string;
}

export interface PaperGainsInput {
  grants: GrantForPaperGains[];
  tickerPrices: TickerPriceForPaperGains[];
  grantOverrides: GrantPriceOverrideForPaperGains[];
  /** EUR per unit of native currency. `null` on ECB-unavailable. */
  fxRateEurNative: string | null;
  today: Date;
}

export type MissingReason =
  | 'fmv_missing'
  | 'no_current_price'
  | 'nso_deferred'
  | 'double_trigger_pre_liquidity';

export interface EurBand {
  /** 3% retail spread (worst-case). */
  low: string;
  /** 1.5% central. */
  mid: string;
  /** 0% wholesale (best-case). */
  high: string;
}

export interface PerGrantGains {
  grantId: string;
  complete: boolean;
  gainNative: string | null;
  gainEurBand: EurBand | null;
  missingReason: MissingReason | null;
}

export interface PaperGainsResult {
  perGrant: PerGrantGains[];
  combinedEurBand: EurBand | null;
  incompleteGrants: string[];
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

export function computePaperGains(input: PaperGainsInput): PaperGainsResult {
  const fx = parseDecimal(input.fxRateEurNative);
  const perGrant: PerGrantGains[] = [];
  const incompleteGrants: string[] = [];

  let combinedLow = 0;
  let combinedMid = 0;
  let combinedHigh = 0;
  let combinedAny = false;

  for (const g of input.grants) {
    const row = computeGrant(g, input, fx);

    if (row.complete && row.gainEurBand) {
      combinedLow += parseDecimal(row.gainEurBand.low) ?? 0;
      combinedMid += parseDecimal(row.gainEurBand.mid) ?? 0;
      combinedHigh += parseDecimal(row.gainEurBand.high) ?? 0;
      combinedAny = true;
    } else if (
      row.missingReason === 'fmv_missing' ||
      row.missingReason === 'no_current_price'
    ) {
      incompleteGrants.push(row.grantId);
    }
    perGrant.push(row);
  }

  const combinedEurBand: EurBand | null =
    combinedAny && fx !== null
      ? {
          low: formatEur(combinedLow),
          mid: formatEur(combinedMid),
          high: formatEur(combinedHigh),
        }
      : null;

  return { perGrant, combinedEurBand, incompleteGrants };
}

// ---------------------------------------------------------------------------
// Per-grant computation
// ---------------------------------------------------------------------------

function computeGrant(
  g: GrantForPaperGains,
  input: PaperGainsInput,
  fx: number | null,
): PerGrantGains {
  // AC-5.4.3 NSO deferral.
  if (g.instrument === 'nso' || g.instrument === 'iso_mapped_to_nso' || g.instrument === 'iso') {
    return {
      grantId: g.id,
      complete: false,
      gainNative: null,
      gainEurBand: null,
      missingReason: 'nso_deferred',
    };
  }

  // AC-5.4.4 double-trigger pre-liquidity.
  if (g.doubleTrigger && g.liquidityEventDate === null) {
    return {
      grantId: g.id,
      complete: false,
      gainNative: null,
      gainEurBand: null,
      missingReason: 'double_trigger_pre_liquidity',
    };
  }

  const price = resolvePrice(g, input);
  if (price === null) {
    return {
      grantId: g.id,
      complete: false,
      gainNative: null,
      gainEurBand: null,
      missingReason: 'no_current_price',
    };
  }

  let gain = 0;
  let complete = true;

  if (g.instrument === 'rsu') {
    const res = rsuGain(g, price, input.today);
    gain = res.gain;
    complete = res.complete;
  } else if (g.instrument === 'espp') {
    gain = esppGain(g, price);
  } else {
    return {
      grantId: g.id,
      complete: false,
      gainNative: null,
      gainEurBand: null,
      missingReason: 'no_current_price',
    };
  }

  if (!complete) {
    return {
      grantId: g.id,
      complete: false,
      gainNative: null,
      gainEurBand: null,
      missingReason: 'fmv_missing',
    };
  }

  const gainEurBand: EurBand | null = fx === null ? null : applyBands(gain, fx);

  return {
    grantId: g.id,
    complete: true,
    gainNative: formatNative(gain),
    gainEurBand,
    missingReason: null,
  };
}

function resolvePrice(g: GrantForPaperGains, input: PaperGainsInput): number | null {
  // Per-grant override wins (AC-5.3.1).
  const o = input.grantOverrides.find((x) => x.grantId === g.id);
  if (o) return parseDecimal(o.price);
  const tickerRaw = g.ticker;
  if (!tickerRaw) return null;
  const t = tickerRaw.trim().toUpperCase();
  const row = input.tickerPrices.find((x) => x.ticker.trim().toUpperCase() === t);
  if (!row) return null;
  return parseDecimal(row.price);
}

function rsuGain(
  g: GrantForPaperGains,
  price: number,
  today: Date,
): { gain: number; complete: boolean } {
  let gain = 0;
  let complete = true;
  let hadPastRow = false;

  for (const ev of g.vestingEvents) {
    if (cmpDate(ev.vestDate, today) > 0) continue;
    hadPastRow = true;

    if (ev.state === 'vested') {
      // contribute
    } else if (ev.state === 'time_vested_awaiting_liquidity') {
      if (ev.fmvAtVest === null) complete = false;
      continue;
    } else {
      // 'upcoming' — unreachable given vestDate <= today; defensive.
      continue;
    }

    const fmv = parseDecimal(ev.fmvAtVest);
    if (fmv === null) {
      complete = false;
      continue;
    }
    const shares = Number(ev.sharesVestedScaled) / Number(SHARES_SCALE);
    gain += (price - fmv) * shares;
  }

  if (!hadPastRow) return { gain: 0, complete: true };
  return { gain, complete };
}

function esppGain(g: GrantForPaperGains, price: number): number {
  let gain = 0;
  for (const p of g.esppPurchases) {
    const fmv = parseDecimal(p.fmvAtPurchase);
    if (fmv === null) continue;
    const shares = Number(p.sharesPurchasedScaled) / Number(SHARES_SCALE);
    gain += (price - fmv) * shares;
  }
  return gain;
}

function applyBands(gainNative: number, fx: number): EurBand {
  const base = gainNative * fx;
  return {
    low: formatEur(base * (1 - 0.03)),
    mid: formatEur(base * (1 - 0.015)),
    high: formatEur(base),
  };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function parseDecimal(s: string | null | undefined): number | null {
  if (s === null || s === undefined) return null;
  const n = parseFloat(s.trim());
  return Number.isFinite(n) ? n : null;
}

function formatEur(v: number): string {
  return v.toFixed(2);
}

function formatNative(v: number): string {
  return v.toFixed(4);
}

function cmpDate(a: Date, b: Date): number {
  const au = Date.UTC(a.getUTCFullYear(), a.getUTCMonth(), a.getUTCDate());
  const bu = Date.UTC(b.getUTCFullYear(), b.getUTCMonth(), b.getUTCDate());
  if (au < bu) return -1;
  if (au > bu) return 1;
  return 0;
}
