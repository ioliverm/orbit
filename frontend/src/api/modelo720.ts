// Modelo 720 user-input endpoint wrappers (Slice 2 T22, ADR-016 §5.3).
// Mirrors backend/crates/orbit-api/src/handlers/modelo_720_inputs.rs.
//
// Two categories are live in Slice 2: `bank_accounts`, `real_estate`.
// `securities` is a stub in the UI (disabled) and will be derived in
// Slice 3 from the new FX pipeline.

import { apiRequest } from './client';

export type Modelo720Category = 'bank_accounts' | 'real_estate';

export type UpsertOutcome =
  | 'inserted'
  | 'closed_and_created'
  | 'updated_same_day'
  | 'no_op';

export interface Modelo720InputDto {
  id: string;
  category: Modelo720Category;
  amountEur: string;
  referenceDate: string;
  fromDate: string;
  toDate: string | null;
  createdAt: string;
}

export interface Modelo720UpsertBody {
  category: Modelo720Category;
  /** Decimal string, e.g. `"25000.00"`. Non-negative. */
  totalEur: string;
  /** Defaults to today (UTC) when omitted. */
  referenceDate?: string;
}

export interface Modelo720UpsertResponse {
  current: Modelo720InputDto;
  outcome: UpsertOutcome;
  /** Present only when outcome === "no_op". */
  unchanged?: boolean;
}

export interface Modelo720CurrentResponse {
  current: Modelo720InputDto | null;
}

export interface Modelo720HistoryResponse {
  history: Modelo720InputDto[];
}

export function upsertInputs(
  body: Modelo720UpsertBody,
): Promise<Modelo720UpsertResponse> {
  return apiRequest<Modelo720UpsertResponse>('POST', '/modelo-720-inputs', body);
}

export function getCurrent(
  category: Modelo720Category,
): Promise<Modelo720CurrentResponse> {
  return apiRequest<Modelo720CurrentResponse>(
    'GET',
    `/modelo-720-inputs/current?category=${encodeURIComponent(category)}`,
  );
}

export function getHistory(
  category: Modelo720Category,
): Promise<Modelo720HistoryResponse> {
  return apiRequest<Modelo720HistoryResponse>(
    'GET',
    `/modelo-720-inputs?category=${encodeURIComponent(category)}`,
  );
}
