// Dashboard endpoint wrapper (Slice 2 T22, ADR-016 §4).
// Mirrors backend/crates/orbit-api/src/handlers/dashboard.rs.
//
// The server response carries scaled share counts as numbers (i64 fits
// inside JSON integers in practice — Slice-2 grants cap at ~10^10
// scaled). The wrapper returns them as bigints to match the TS
// parity-mirror in `lib/stackedGrants.ts`.

import { apiRequest } from './client';

export interface WireStackedPoint {
  date: string;
  cumulativeSharesVested: number | string;
  cumulativeTimeVestedAwaitingLiquidity: number | string;
  perGrantBreakdown: Array<{
    grantId: string;
    instrument: string;
    sharesVestedThisEvent: number | string;
    cumulativeForThisGrant: number | string;
    state: 'upcoming' | 'time_vested_awaiting_liquidity' | 'vested';
  }>;
}

export interface WireEmployerStack {
  employerName: string;
  employerKey: string;
  grantIds: string[];
  points: WireStackedPoint[];
}

export interface StackedDashboardResponse {
  byEmployer: WireEmployerStack[];
  combined: WireStackedPoint[];
}

export function getStacked(): Promise<StackedDashboardResponse> {
  return apiRequest<StackedDashboardResponse>('GET', '/dashboard/stacked');
}

/** Coerce a number|string JSON numeric field into bigint. */
export function toBigInt(v: number | string | bigint): bigint {
  if (typeof v === 'bigint') return v;
  if (typeof v === 'number') return BigInt(v);
  return BigInt(v);
}
