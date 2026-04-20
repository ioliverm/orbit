// Auth-endpoint wrappers. Thin mappings onto /api/v1/auth/*.
// Types mirror the DTOs in backend/crates/orbit-api/src/handlers/auth.rs.

import { apiRequest } from './client';

export type OnboardingStage = 'disclaimer' | 'residency' | 'first_grant' | 'complete';

export interface MeUser {
  id: string;
  email: string;
  locale: string;
  primaryCurrency: string;
}

export interface MeResidencySummary {
  jurisdiction: string;
  subJurisdiction: string | null;
  regimeFlags: string[] | null;
  fromDate: string;
}

export interface MeResponse {
  user: MeUser;
  residency: MeResidencySummary | null;
  onboardingStage: OnboardingStage;
  disclaimerAccepted: boolean;
}

export interface SignupBody {
  email: string;
  password: string;
  localeHint?: string;
}

export interface SigninBody {
  email: string;
  password: string;
}

export interface VerifyEmailBody {
  token: string;
}

export function signup(body: SignupBody): Promise<void> {
  return apiRequest<void>('POST', '/auth/signup', body);
}

export function signin(body: SigninBody): Promise<void> {
  return apiRequest<void>('POST', '/auth/signin', body);
}

export function verifyEmail(body: VerifyEmailBody): Promise<void> {
  return apiRequest<void>('POST', '/auth/verify-email', body);
}

export function signout(): Promise<void> {
  return apiRequest<void>('POST', '/auth/signout');
}

export function me(): Promise<MeResponse> {
  return apiRequest<MeResponse>('GET', '/auth/me');
}
