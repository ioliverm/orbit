// Grant-endpoint wrappers (T13b — ADR-014 §3, AC-4.2.*, AC-6.*).
// Mirrors the DTOs in backend/crates/orbit-api/src/handlers/grants.rs.
//
// The server emits `shareCount` as a whole-share string and
// `shareCountScaled` as an i64 in 10_000ths. We keep both so clients that
// want exact arithmetic can use the scaled representation (see lib/vesting).

import { apiRequest } from './client';

export type Instrument = 'rsu' | 'nso' | 'espp' | 'iso';
export type InstrumentStored = 'rsu' | 'nso' | 'espp' | 'iso_mapped_to_nso';
export type Cadence = 'monthly' | 'quarterly';
export type VestingState = 'upcoming' | 'time_vested_awaiting_liquidity' | 'vested';

export interface GrantDto {
  id: string;
  instrument: InstrumentStored;
  grantDate: string;
  shareCount: string;
  shareCountScaled: number;
  strikeAmount: string | null;
  strikeCurrency: string | null;
  vestingStart: string;
  vestingTotalMonths: number;
  cliffMonths: number;
  vestingCadence: Cadence;
  doubleTrigger: boolean;
  liquidityEventDate: string | null;
  doubleTriggerSatisfiedBy: string | null;
  employerName: string;
  ticker: string | null;
  notes: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface VestingEventDto {
  /** Slice 3: present on persisted rows, absent on derivation-preview rows. */
  id?: string;
  vestDate: string;
  sharesVestedThisEvent: string;
  sharesVestedThisEventScaled: number;
  cumulativeSharesVested: string;
  cumulativeSharesVestedScaled: number;
  state: VestingState;
  /** Slice 3 additions. Present on the persisted-row emit path. */
  fmvAtVest?: string | null;
  fmvCurrency?: string | null;
  isUserOverride?: boolean;
  updatedAt?: string;
}

export interface GrantBody {
  instrument: Instrument;
  grantDate: string;
  shareCount: number;
  strikeAmount?: string;
  strikeCurrency?: 'USD' | 'EUR' | 'GBP';
  vestingStart: string;
  vestingTotalMonths: number;
  cliffMonths: number;
  vestingCadence: Cadence;
  doubleTrigger: boolean;
  liquidityEventDate?: string;
  employerName: string;
  ticker?: string;
  notes?: string;
  esppEstimatedDiscountPct?: number;
}

export interface GrantCreateResponse {
  grant: GrantDto;
  vestingEvents: VestingEventDto[];
}

export interface GrantListResponse {
  grants: GrantDto[];
}

export interface GrantGetResponse {
  grant: GrantDto;
  /** Slice 3: true iff the grant has ≥1 vesting_events row with is_user_override = true. */
  overridesWarning?: boolean;
  overrideCount?: number;
}

export interface VestingResponse {
  vestingEvents: VestingEventDto[];
  vestedToDate: string;
  vestedToDateScaled: number;
  awaitingLiquidity: string;
  awaitingLiquidityScaled: number;
}

export function createGrant(body: GrantBody): Promise<GrantCreateResponse> {
  return apiRequest<GrantCreateResponse>('POST', '/grants', body);
}

export function listGrants(): Promise<GrantListResponse> {
  return apiRequest<GrantListResponse>('GET', '/grants');
}

export function getGrant(id: string): Promise<GrantGetResponse> {
  return apiRequest<GrantGetResponse>('GET', `/grants/${encodeURIComponent(id)}`);
}

export function updateGrant(id: string, body: GrantBody): Promise<GrantCreateResponse> {
  return apiRequest<GrantCreateResponse>('PUT', `/grants/${encodeURIComponent(id)}`, body);
}

export function deleteGrant(id: string): Promise<void> {
  return apiRequest<void>('DELETE', `/grants/${encodeURIComponent(id)}`);
}

export function getGrantVesting(id: string): Promise<VestingResponse> {
  return apiRequest<VestingResponse>('GET', `/grants/${encodeURIComponent(id)}/vesting`);
}
