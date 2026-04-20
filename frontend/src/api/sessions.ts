// Session / device list endpoint wrappers (Slice 2 T22, AC-7.*).
// Mirrors the SessionRowDto in backend/crates/orbit-api/src/handlers/auth.rs
// (not to be confused with the older /auth shape in api/auth.ts which
// carries the `/auth/me` and signin/signup DTOs).

import { apiRequest } from './client';

export interface SessionRowDto {
  id: string;
  userAgent: string;
  countryIso2: string | null;
  createdAt: string;
  lastUsedAt: string;
  /** `true` on exactly one row — the session owning this request. */
  isCurrent: boolean;
}

export interface SessionListResponse {
  sessions: SessionRowDto[];
}

export interface RevokeAllOthersResponse {
  revokedCount: number;
}

export function listSessions(): Promise<SessionListResponse> {
  return apiRequest<SessionListResponse>('GET', '/auth/sessions');
}

export function revokeSession(id: string): Promise<void> {
  return apiRequest<void>('DELETE', `/auth/sessions/${encodeURIComponent(id)}`);
}

export function revokeAllOthers(): Promise<RevokeAllOthersResponse> {
  return apiRequest<RevokeAllOthersResponse>('POST', '/auth/sessions/revoke-all-others');
}
