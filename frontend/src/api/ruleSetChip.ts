// Rule-set chip endpoint (Slice 3 T30).
//
// Mirrors backend/crates/orbit-api/src/handlers/rule_set_chip.rs. Per
// AC-7.1.6 the chip in Slice 3 only exposes `fxDate` + `engineVersion`;
// tax rule-set stamping arrives in Slice 4.

import { apiRequest } from './client';

export interface RuleSetChipResponse {
  fxDate: string | null;
  stalenessDays: number | null;
  engineVersion: string;
}

export function getRuleSetChip(): Promise<RuleSetChipResponse> {
  return apiRequest<RuleSetChipResponse>('GET', '/rule-set-chip');
}
