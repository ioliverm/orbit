// Per-ticker + per-grant current-price endpoints (Slice 3 T30).
//
// Mirrors backend/crates/orbit-api/src/handlers/current_prices.rs. All
// state-changing verbs carry CSRF via the shared client wrapper.

import { apiRequest } from './client';

export type PriceCurrency = 'USD' | 'EUR' | 'GBP';

export interface TickerPriceDto {
  ticker: string;
  price: string;
  currency: string;
  enteredAt: string;
}

export interface TickerPriceListResponse {
  prices: TickerPriceDto[];
}

export interface TickerPriceBody {
  price: string;
  currency: PriceCurrency;
}

export interface GrantOverrideResponse {
  override: { price: string; currency: string; enteredAt: string } | null;
}

export interface GrantOverrideBody {
  price: string;
  currency: PriceCurrency;
}

// ---------------------------------------------------------------------------
// Per-ticker
// ---------------------------------------------------------------------------

export function listPrices(): Promise<TickerPriceListResponse> {
  return apiRequest<TickerPriceListResponse>('GET', '/current-prices');
}

export function upsertPrice(ticker: string, body: TickerPriceBody): Promise<TickerPriceDto> {
  return apiRequest<TickerPriceDto>(
    'PUT',
    `/current-prices/${encodeURIComponent(ticker)}`,
    body,
  );
}

export function deletePrice(ticker: string): Promise<void> {
  return apiRequest<void>('DELETE', `/current-prices/${encodeURIComponent(ticker)}`);
}

// ---------------------------------------------------------------------------
// Per-grant override
// ---------------------------------------------------------------------------

export function getGrantOverride(grantId: string): Promise<GrantOverrideResponse> {
  return apiRequest<GrantOverrideResponse>(
    'GET',
    `/grants/${encodeURIComponent(grantId)}/current-price-override`,
  );
}

export function upsertGrantOverride(
  grantId: string,
  body: GrantOverrideBody,
): Promise<GrantOverrideResponse> {
  return apiRequest<GrantOverrideResponse>(
    'PUT',
    `/grants/${encodeURIComponent(grantId)}/current-price-override`,
    body,
  );
}

export function deleteGrantOverride(grantId: string): Promise<void> {
  return apiRequest<void>(
    'DELETE',
    `/grants/${encodeURIComponent(grantId)}/current-price-override`,
  );
}
