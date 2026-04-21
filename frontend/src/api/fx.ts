// FX endpoints (Slice 3 T30).
//
// Mirrors backend/crates/orbit-api/src/handlers/fx.rs. Reference-data
// reads — no auth header, no RLS. The endpoints live at `/api/v1/fx/*`.

import { apiRequest } from './client';

export type FxStaleness = 'fresh' | 'walkback' | 'stale' | 'unavailable';

export interface FxRateResponse {
  quote: string;
  rateDate: string | null;
  rate: string | null;
  walkback: number | null;
  staleness: FxStaleness;
}

export interface FxLatestResponse {
  quote: string;
  rateDate: string | null;
  rate: string | null;
}

export function getRate(quote: string, on?: string): Promise<FxRateResponse> {
  const q = encodeURIComponent(quote);
  const path = on ? `/fx/rate?quote=${q}&on=${encodeURIComponent(on)}` : `/fx/rate?quote=${q}`;
  return apiRequest<FxRateResponse>('GET', path);
}

export function getLatest(quote: string): Promise<FxLatestResponse> {
  return apiRequest<FxLatestResponse>('GET', `/fx/latest?quote=${encodeURIComponent(quote)}`);
}
