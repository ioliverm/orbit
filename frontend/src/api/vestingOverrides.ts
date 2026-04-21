// Vesting-event override endpoints (Slice 3 T30).
//
// Mirrors backend/crates/orbit-api/src/handlers/vesting_events.rs. The
// PUT carries `expectedUpdatedAt` for optimistic concurrency (AC-10.5);
// 409 Conflict is surfaced through AppError with code
// `resource.stale_client_state` (mapped via the shared client wrapper).

import { apiRequest } from './client';
import type { PriceCurrency } from './currentPrices';

export interface VestingOverrideBody {
  vestDate?: string;
  sharesVested?: number;
  /** `undefined` = leave FMV alone; `null` = clear; string = set. */
  fmvAtVest?: string | null;
  fmvCurrency?: PriceCurrency | null;
  clearOverride?: boolean;
  expectedUpdatedAt: string;
}

export interface VestingEventRowDto {
  id: string;
  grantId: string;
  vestDate: string;
  sharesVestedThisEvent: string;
  sharesVestedThisEventScaled: number;
  cumulativeSharesVested: string;
  fmvAtVest: string | null;
  fmvCurrency: string | null;
  isUserOverride: boolean;
  updatedAt: string;
}

export function putOverride(
  grantId: string,
  eventId: string,
  body: VestingOverrideBody,
): Promise<VestingEventRowDto> {
  return apiRequest<VestingEventRowDto>(
    'PUT',
    `/grants/${encodeURIComponent(grantId)}/vesting-events/${encodeURIComponent(eventId)}`,
    body,
  );
}

export interface BulkFmvBody {
  fmv: string;
  currency: PriceCurrency;
}

export interface BulkFmvResponse {
  appliedCount: number;
  skippedCount: number;
}

export function postBulkFmv(grantId: string, body: BulkFmvBody): Promise<BulkFmvResponse> {
  return apiRequest<BulkFmvResponse>(
    'POST',
    `/grants/${encodeURIComponent(grantId)}/vesting-events/bulk-fmv`,
    body,
  );
}
