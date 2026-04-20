// ESPP-purchase endpoint wrappers (Slice 2 T22, ADR-016 §5.1).
// Mirrors the DTOs in backend/crates/orbit-api/src/handlers/espp_purchases.rs.
//
// All endpoints are under the authenticated, onboarding-gated subtree.
// The POST response carries a top-level `migratedFromNotes` flag the UI
// uses to surface the notes-JSON lift toast (AC-4.5.1).

import { apiRequest } from './client';

export type EsppCurrency = 'USD' | 'EUR' | 'GBP';

export interface EsppPurchaseDto {
  id: string;
  grantId: string;
  offeringDate: string;
  purchaseDate: string;
  fmvAtPurchase: string;
  purchasePricePerShare: string;
  sharesPurchased: string;
  sharesPurchasedScaled: number;
  currency: EsppCurrency;
  fmvAtOffering: string | null;
  employerDiscountPercent: string | null;
  notes: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface EsppPurchaseBody {
  offeringDate: string;
  purchaseDate: string;
  fmvAtPurchase: string;
  purchasePricePerShare: string;
  /** Whole shares (the handler scales by 10_000 before persist). */
  sharesPurchased: number;
  currency: EsppCurrency;
  fmvAtOffering?: string;
  employerDiscountPercent?: string;
  notes?: string;
  /** AC-4.2.8 soft-warn override (opt-in on the retry CTA). */
  forceDuplicate?: boolean;
}

export interface EsppListResponse {
  purchases: EsppPurchaseDto[];
}

export interface EsppCreateResponse {
  purchase: EsppPurchaseDto;
  /** True iff the Slice-1 notes.estimated_discount_percent was lifted. */
  migratedFromNotes: boolean;
}

export interface EsppGetResponse {
  purchase: EsppPurchaseDto;
}

export function listPurchases(grantId: string): Promise<EsppListResponse> {
  return apiRequest<EsppListResponse>(
    'GET',
    `/grants/${encodeURIComponent(grantId)}/espp-purchases`,
  );
}

export function createPurchase(
  grantId: string,
  body: EsppPurchaseBody,
): Promise<EsppCreateResponse> {
  return apiRequest<EsppCreateResponse>(
    'POST',
    `/grants/${encodeURIComponent(grantId)}/espp-purchases`,
    body,
  );
}

export function getPurchase(id: string): Promise<EsppGetResponse> {
  return apiRequest<EsppGetResponse>('GET', `/espp-purchases/${encodeURIComponent(id)}`);
}

export function updatePurchase(
  id: string,
  body: EsppPurchaseBody,
): Promise<EsppGetResponse> {
  return apiRequest<EsppGetResponse>(
    'PUT',
    `/espp-purchases/${encodeURIComponent(id)}`,
    body,
  );
}

export function deletePurchase(id: string): Promise<void> {
  return apiRequest<void>('DELETE', `/espp-purchases/${encodeURIComponent(id)}`);
}
