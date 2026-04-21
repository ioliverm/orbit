// Paper-gains dashboard endpoint (Slice 3 T30).
//
// Mirrors backend/crates/orbit-api/src/handlers/dashboard_paper_gains.rs.

import { apiRequest } from './client';
import type { FxStaleness } from './fx';
import type { EurBand, MissingReason } from '../lib/paperGains';

export interface PerGrantDto {
  grantId: string;
  employer: string | null;
  instrument: string | null;
  complete: boolean;
  gainNative?: string | null;
  gainEurBand?: EurBand | null;
  missingReason?: MissingReason | null;
}

export interface IncompleteGrantDto {
  grantId: string;
  employer: string | null;
  instrument: string | null;
}

export interface PaperGainsResponse {
  perGrant: PerGrantDto[];
  combinedEurBand: EurBand | null;
  incompleteGrants: IncompleteGrantDto[];
  stalenessFx: FxStaleness;
  fxDate: string | null;
}

export function getPaperGains(): Promise<PaperGainsResponse> {
  return apiRequest<PaperGainsResponse>('GET', '/dashboard/paper-gains');
}
