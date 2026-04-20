// Playwright config — scopes test discovery to the `e2e/` directory so
// Vitest specs under `src/**/*.test.{ts,tsx}` are not mistakenly loaded by
// Playwright. Defaults everywhere else.
//
// Gating: the Slice-1 demo spec skips on missing `ORBIT_E2E_BASE_URL`
// (see `e2e/slice-1-demo.spec.ts` header comment). CI does not bring up a
// live stack until Slice 8.

import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  // Keep the matcher tight so stray `src/**/*.test.ts` files don't leak in.
  testMatch: /.*\.spec\.ts$/,
  fullyParallel: false,
  retries: 0,
  workers: 1,
  reporter: [['list']],
});
