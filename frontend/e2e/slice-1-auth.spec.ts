// Slice 1 T14a — minimal Playwright smoke for the auth loop.
//
// Full 17-step E2E lands in T15 per the acceptance-criteria doc. This test
// is the placeholder that exercises the happy path when both a dev backend
// and a dev frontend are up. It is skipped when `ORBIT_E2E_BASE_URL` is
// unset so CI doesn't require a live stack.
//
// Run manually:
//   ORBIT_E2E_BASE_URL=http://127.0.0.1:5173 pnpm exec playwright test

import { expect, test } from '@playwright/test';

const baseUrl = process.env.ORBIT_E2E_BASE_URL;

test.skip(!baseUrl, 'ORBIT_E2E_BASE_URL not set — skipping E2E. T15 wires full run.');

test('auth surfaces render without console errors', async ({ page }) => {
  if (!baseUrl) return;
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(err.message));

  await page.goto(`${baseUrl}/signin`);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();

  await page.goto(`${baseUrl}/signup`);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();

  await page.goto(`${baseUrl}/password-reset/request`);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();

  expect(errors).toEqual([]);
});
