// Reads the `orbit_csrf` cookie so rare call-sites that want to pass the
// token explicitly (e.g. form POSTs outside the apiRequest helper) can do
// so. The apiRequest helper in src/api/client.ts already handles header
// injection for all normal call paths — this hook is intentionally thin.

import { readCookie } from '../api/client';

export function useCSRFToken(): string | null {
  // Cookies are not reactive; a component that mounts after signin picks up
  // the fresh value. Refreshes after rotation require a remount — acceptable
  // for Slice 1 because rotation only happens on refresh endpoints which the
  // UI does not call directly.
  return readCookie('orbit_csrf');
}
