// Residency-endpoint wrappers (ADR-014 §3, AC-4.1.*).
// Mirrors the DTOs in backend/crates/orbit-api/src/handlers/residency.rs.

import { apiRequest } from './client';

export interface Autonomia {
  code: string;
  nameEs: string;
  nameEn: string;
  foral: boolean;
}

export interface AutonomiasResponse {
  autonomias: Autonomia[];
}

export interface ResidencyDto {
  id: string;
  jurisdiction: string;
  subJurisdiction: string | null;
  fromDate: string;
  toDate: string | null;
  regimeFlags: string[];
}

export interface ResidencyResponse {
  residency: ResidencyDto;
  primaryCurrency: string;
}

export interface ResidencyBody {
  jurisdiction: 'ES';
  subJurisdiction: string;
  primaryCurrency: 'EUR' | 'USD';
  regimeFlags: string[];
}

export function listAutonomias(): Promise<AutonomiasResponse> {
  return apiRequest<AutonomiasResponse>('GET', '/residency/autonomias');
}

export function createResidency(body: ResidencyBody): Promise<ResidencyResponse> {
  return apiRequest<ResidencyResponse>('POST', '/residency', body);
}

export function getResidency(): Promise<{
  residency: ResidencyDto | null;
  primaryCurrency: string;
}> {
  return apiRequest<{ residency: ResidencyDto | null; primaryCurrency: string }>(
    'GET',
    '/residency',
  );
}
