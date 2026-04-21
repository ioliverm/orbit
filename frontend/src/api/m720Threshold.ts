// Modelo 720 threshold dashboard endpoint (Slice 3 T30).
//
// Mirrors backend/crates/orbit-api/src/handlers/dashboard_m720.rs.

import { apiRequest } from './client';

export interface FxSensitivityBand {
  low: string;
  mid: string;
  high: string;
}

export interface M720ThresholdResponse {
  bankAccountsEur: string | null;
  realEstateEur: string | null;
  securitiesEur: string | null;
  perCategoryBreach: boolean;
  aggregateBreach: boolean;
  thresholdEur: string;
  fxSensitivityBand: FxSensitivityBand | null;
  fxDate: string | null;
}

export function getThreshold(): Promise<M720ThresholdResponse> {
  return apiRequest<M720ThresholdResponse>('GET', '/dashboard/modelo-720-threshold');
}
