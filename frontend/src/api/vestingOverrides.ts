// Vesting-event override endpoints (Slice 3 T30, extended in Slice 3b T38/T39).
//
// Mirrors backend/crates/orbit-api/src/handlers/vesting_events.rs. The
// PUT carries `expectedUpdatedAt` for optimistic concurrency (AC-10.5);
// 409 Conflict is surfaced through AppError with code
// `resource.stale_client_state` (mapped via the shared client wrapper).
//
// # Slice 3b additions (ADR-018 §3)
//
// The PUT body grows four optional keys:
//   - `taxWithholdingPercent` — fraction in `[0, 1]` as a decimal
//     string, or `null` to clear.
//   - `shareSellPrice` — per-share sell price, same wire shape as
//     `fmvAtVest` (string or null).
//   - `shareSellCurrency` — `"USD" | "EUR" | "GBP"`, defaults server-
//     side to `fmvCurrency` when omitted.
//   - `clearSellToCoverOverride` — narrow clear (preserves FMV/date/
//     shares; only wipes the sell-to-cover triplet).
//
// The response DTO grows five captured + four derived fields. Derived
// values are `null` when the triplet is incomplete or the compute
// errors (the caller renders empty cells in that case — AC-7.2.2).

import { apiRequest } from './client';
import type { PriceCurrency } from './currentPrices';

export interface VestingOverrideBody {
  vestDate?: string;
  /** Whole-share integer (legacy) or decimal string with up to 4 dp
   *  (preferred; preserves fractional precision on round-trip). */
  sharesVested?: number | string;
  /** `undefined` = leave FMV alone; `null` = clear; string = set. */
  fmvAtVest?: string | null;
  fmvCurrency?: PriceCurrency | null;
  clearOverride?: boolean;
  expectedUpdatedAt: string;

  // --- Slice 3b additions (ADR-018 §3) ---
  /** Fraction in `[0, 1]` as a decimal string (e.g. `"0.4500"`).
   *  `undefined` = key absent (default-sourcing may fire when a sell
   *  price is being set). `null` = explicit clear (suppresses
   *  default-sourcing). */
  taxWithholdingPercent?: string | null;
  shareSellPrice?: string | null;
  shareSellCurrency?: PriceCurrency | null;
  /** Narrow revert: clears only the sell-to-cover triplet; preserves
   *  FMV, vest_date, shares, and the Slice-3 override flag. Mutually
   *  exclusive with `clearOverride` and with the triplet fields. */
  clearSellToCoverOverride?: boolean;
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

  // --- Slice 3b captured (ADR-018 §3) ---
  taxWithholdingPercent?: string | null;
  shareSellPrice?: string | null;
  shareSellCurrency?: string | null;
  isSellToCoverOverride?: boolean;
  sellToCoverOverriddenAt?: string | null;

  // --- Slice 3b derived (null when triplet incomplete) ---
  grossAmount?: string | null;
  sharesSoldForTaxes?: string | null;
  netSharesDelivered?: string | null;
  cashWithheld?: string | null;
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
