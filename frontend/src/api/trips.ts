// Art. 7.p trip endpoint wrappers (Slice 2 T22, ADR-016 §5.2).
// Mirrors the DTOs in backend/crates/orbit-api/src/handlers/trips.rs.
//
// The five-criterion checklist lives in `eligibilityCriteria` as a
// `{ key: boolean | null }` object. AC-5.2.3 rejects any `null` on
// submit; the UI may pre-fill with `null` for a partial-edit round-trip
// but must reject unanswered items before POST/PUT.

import { apiRequest } from './client';

export type Art7pCriterionKey =
  | 'services_outside_spain'
  | 'non_spanish_employer'
  | 'not_tax_haven'
  | 'no_double_exemption'
  | 'within_annual_cap';

export const ART_7P_CRITERION_KEYS: readonly Art7pCriterionKey[] = [
  'services_outside_spain',
  'non_spanish_employer',
  'not_tax_haven',
  'no_double_exemption',
  'within_annual_cap',
];

export type EligibilityAnswer = boolean | null;

export type EligibilityCriteria = Record<Art7pCriterionKey, EligibilityAnswer>;

export interface TripDto {
  id: string;
  destinationCountry: string;
  fromDate: string;
  toDate: string;
  employerPaid: boolean;
  purpose: string | null;
  eligibilityCriteria: EligibilityCriteria;
  createdAt: string;
  updatedAt: string;
}

export interface TripBody {
  destinationCountry: string;
  fromDate: string;
  toDate: string;
  employerPaid: boolean;
  purpose?: string;
  eligibilityCriteria: EligibilityCriteria;
}

export interface AnnualCapTracker {
  year: number;
  tripCount: number;
  dayCountDeclared: number;
  employerPaidTripCount: number;
  criteriaMetCountByKey: Record<Art7pCriterionKey, number>;
}

export interface TripListResponse {
  trips: TripDto[];
  annualCapTracker: AnnualCapTracker;
}

export interface TripGetResponse {
  trip: TripDto;
}

export function listTrips(year?: number): Promise<TripListResponse> {
  const path = typeof year === 'number' ? `/trips?year=${year}` : '/trips';
  return apiRequest<TripListResponse>('GET', path);
}

export function createTrip(body: TripBody): Promise<TripGetResponse> {
  return apiRequest<TripGetResponse>('POST', '/trips', body);
}

export function getTrip(id: string): Promise<TripGetResponse> {
  return apiRequest<TripGetResponse>('GET', `/trips/${encodeURIComponent(id)}`);
}

export function updateTrip(id: string, body: TripBody): Promise<TripGetResponse> {
  return apiRequest<TripGetResponse>('PUT', `/trips/${encodeURIComponent(id)}`, body);
}

export function deleteTrip(id: string): Promise<void> {
  return apiRequest<void>('DELETE', `/trips/${encodeURIComponent(id)}`);
}

/** Count of truthy answers; non-true (false, null) count as "not met". */
export function criteriaMetCount(c: EligibilityCriteria): number {
  let n = 0;
  for (const k of ART_7P_CRITERION_KEYS) {
    if (c[k] === true) n += 1;
  }
  return n;
}

/** Count of answered (non-null) criteria. */
export function criteriaAnsweredCount(c: EligibilityCriteria): number {
  let n = 0;
  for (const k of ART_7P_CRITERION_KEYS) {
    if (c[k] !== null) n += 1;
  }
  return n;
}
