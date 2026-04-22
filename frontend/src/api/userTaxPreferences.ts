// User tax-preferences endpoints (Slice 3b T38 / T39, ADR-018 §3).
//
// Mirrors `backend/crates/orbit-api/src/handlers/user_tax_preferences.rs`.
//
// The store is time-series (close-and-create on every save per
// ADR-016) but the "current" row is what the Profile form binds to —
// the history list powers the read-only "Histórico" table below the
// form.
//
// Audit allowlist (SEC-101-strict, ADR-018 §5) lives server-side; the
// client never receives the audit payload.

import { apiRequest } from './client';

/** ISO-3166 alpha-2 curated to the v1 list (ADR-018 §1). */
export type UserTaxCountry = 'ES' | 'PT' | 'FR' | 'IT' | 'DE' | 'NL' | 'GB';

export interface UserTaxPreferenceDto {
  id: string;
  countryIso2: string;
  /** NUMERIC(5,4) passthrough as a decimal string in `[0, 1]` (e.g.
   *  `"0.4500"`). `null` when the country has no percent field (non-ES)
   *  or the user left it blank. */
  rendimientoDelTrabajoPercent: string | null;
  sellToCoverEnabled: boolean;
  fromDate: string;
  /** `null` on the currently-open row (AC-4.5.1). */
  toDate: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface UserTaxPreferencesCurrentResponse {
  /** `null` when the user has never saved preferences. */
  current: UserTaxPreferenceDto | null;
}

export interface UserTaxPreferencesHistoryResponse {
  preferences: UserTaxPreferenceDto[];
}

export interface UserTaxPreferencesUpsertBody {
  countryIso2: UserTaxCountry;
  /** Decimal string in `[0, 1]` (e.g. `"0.4500"`). Send `null` when
   *  the country is not Spain (field hidden) or the user left it
   *  blank. */
  rendimientoDelTrabajoPercent: string | null;
  sellToCoverEnabled: boolean;
}

/** Outcome mirrors `UpsertOutcome` in the backend (ADR-016
 *  close-and-create). */
export type UserTaxPreferencesUpsertOutcome =
  | 'inserted'
  | 'closed_and_created'
  | 'updated_same_day'
  | 'no_op';

export interface UserTaxPreferencesUpsertResponse {
  current: UserTaxPreferenceDto;
  outcome: UserTaxPreferencesUpsertOutcome;
  /** Only present (true) when outcome is `no_op`. */
  unchanged?: boolean;
}

export function getCurrentTaxPreferences(): Promise<UserTaxPreferencesCurrentResponse> {
  return apiRequest<UserTaxPreferencesCurrentResponse>(
    'GET',
    '/user-tax-preferences/current',
  );
}

export function getTaxPreferencesHistory(): Promise<UserTaxPreferencesHistoryResponse> {
  return apiRequest<UserTaxPreferencesHistoryResponse>('GET', '/user-tax-preferences');
}

export function upsertTaxPreferences(
  body: UserTaxPreferencesUpsertBody,
): Promise<UserTaxPreferencesUpsertResponse> {
  return apiRequest<UserTaxPreferencesUpsertResponse>(
    'POST',
    '/user-tax-preferences',
    body,
  );
}
