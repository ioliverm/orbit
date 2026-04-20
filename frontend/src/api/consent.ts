// Consent wrappers. Slice 1 ships only the disclaimer; DSR endpoints
// (export / erasure / restriction / rectification) land in Slice 7.

import { apiRequest } from './client';

export const DISCLAIMER_VERSION = 'v1-2026-04';

export interface AcceptDisclaimerBody {
  version: string;
}

export function acceptDisclaimer(version: string = DISCLAIMER_VERSION): Promise<void> {
  return apiRequest<void>('POST', '/consent/disclaimer', { version });
}
