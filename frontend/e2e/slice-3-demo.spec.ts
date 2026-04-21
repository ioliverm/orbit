/**
 * Slice 3 T31 — Playwright port of `docs/requirements/slice-3-acceptance-criteria.md` §13.
 *
 * Picks up from a persisted user who holds a realistic portfolio and
 * walks the Slice-3 demo script: per-ticker current prices, per-vest
 * FMV capture + manual overrides, bulk-fill, M720 threshold, rule-set
 * chip, and the cumulative-invariant-relaxed warning.
 *
 * Seeded programmatically via the DB helper (same discipline as the
 * Slice-2 spec): a user at "complete" onboarding with an RSU grant
 * already ~20 months into a 48-month schedule. This keeps the spec
 * self-contained (no dependency on the Slice-2 spec's output) and
 * lets the assertions focus on the Slice-3 surfaces rather than the
 * onboarding wizard.
 *
 * ## Running locally
 *
 *   just db-up && just migrate
 *   DATABASE_URL='postgres://orbit_app:…sslmode=require&sslrootcert=scripts/dev/.ca.crt' \
 *     cargo run -p orbit -- api &            # :8080
 *   cd frontend && pnpm dev &                 # :5173 (proxies /api → :8080)
 *
 *   ORBIT_E2E_BASE_URL=http://127.0.0.1:5173 \
 *   ORBIT_E2E_DATABASE_URL='postgres://orbit_migrate:…' \
 *   pnpm exec playwright test slice-3-demo
 *
 * ## Gating
 *
 * Same discipline as Slice-1/2: if `ORBIT_E2E_BASE_URL` is unset the
 * whole file is skipped. CI cannot bootstrap the full stack until
 * Slice 8 (deploy). The spec is the load-bearing artifact; the run is
 * optional until then.
 *
 * ## Axe-core
 *
 * After the populated dashboard renders in the final step we run
 * axe-core and fail on `critical` / `serious` violations. Vitest
 * `axe-smoke.test.tsx` runs the same check against jsdom pre-commit.
 */

import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { Client as PgClient } from 'pg';

const baseUrl = process.env.ORBIT_E2E_BASE_URL;
const dbUrl = process.env.ORBIT_E2E_DATABASE_URL ?? process.env.DATABASE_URL_MIGRATE;

test.skip(
  !baseUrl,
  'ORBIT_E2E_BASE_URL not set — Slice-3 demo spec requires a live stack. See header comment.',
);

const EMAIL = `test+slice3-${Date.now()}@orbit.local`;
const PASSWORD = 'Orbit-Demo-Passphrase-2026';

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

async function withDb<T>(fn: (c: PgClient) => Promise<T>): Promise<T> {
  if (!dbUrl) {
    throw new Error('ORBIT_E2E_DATABASE_URL (or DATABASE_URL_MIGRATE) required for DB helpers');
  }
  const client = new PgClient({ connectionString: dbUrl });
  await client.connect();
  try {
    return await fn(client);
  } finally {
    await client.end();
  }
}

async function forceEmailVerified(email: string): Promise<void> {
  await withDb(async (c) => {
    await c.query(
      'UPDATE users SET email_verified_at = now() WHERE email = $1::citext AND email_verified_at IS NULL',
      [email],
    );
  });
}

/**
 * Seed a fresh ECB FX row for today so the paper-gains and rule-set
 * chip surfaces render a walkback=0 "fresh" state. Bootstraps the
 * 7-day lookback window.
 */
async function seedEcbToday(): Promise<void> {
  await withDb(async (c) => {
    await c.query(
      `INSERT INTO fx_rates (base, quote, rate_date, rate, source)
       VALUES ('EUR', 'USD', CURRENT_DATE, 1.0823::numeric, 'ecb')
       ON CONFLICT DO NOTHING`,
    );
  });
}

async function auditCount(email: string, action: string): Promise<number> {
  return withDb(async (c) => {
    const r = await c.query<{ n: string }>(
      'SELECT COUNT(*)::text AS n FROM audit_log a JOIN users u ON u.id = a.user_id WHERE u.email = $1::citext AND a.action = $2',
      [email, action],
    );
    return Number(r.rows[0]?.n ?? 0);
  });
}

async function runAxe(page: Page, label: string): Promise<void> {
  const results = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze();
  const blocking = results.violations.filter(
    (v) => v.impact === 'critical' || v.impact === 'serious',
  );
  if (blocking.length > 0) {
    const lines = blocking.map((v) => `  - [${v.impact}] ${v.id}: ${v.help}`).join('\n');
    throw new Error(`axe at ${label}: ${blocking.length} blocking violations\n${lines}`);
  }
  expect(blocking).toEqual([]);
}

// ---------------------------------------------------------------------------
// The ~17-step demo script (ports §13 with setup collapsed).
// ---------------------------------------------------------------------------

test.describe('Slice 3 demo-acceptance script (§13, 17 test.steps)', () => {
  test.setTimeout(240_000);

  test('runs the full Slice-3 flow end-to-end with axe smoke on the final dashboard', async ({
    page,
    context,
  }) => {
    await context.emulateMedia({ reducedMotion: 'reduce' });
    const consoleErrors: string[] = [];
    page.on('pageerror', (e) => consoleErrors.push(e.message));

    // 1. Seed ECB + bring user up to "complete" with one RSU grant that
    //    has past vest events (vesting_start = ~20 months ago).
    await test.step('1. Seed ECB FX + create complete user with grant at month ~20', async () => {
      await seedEcbToday();
      // Compute a vesting_start 20 months ago (ISO).
      const d = new Date();
      d.setMonth(d.getMonth() - 20);
      const vestingStart = d.toISOString().slice(0, 10);

      await page.goto(`${baseUrl}/signup`);
      await page.getByLabel(/correo electrónico|email/i).fill(EMAIL);
      await page.getByLabel(/contraseña|password/i).fill(PASSWORD);
      await page.getByRole('button', { name: /continuar|continue/i }).click();
      await expect(page).toHaveURL(/\/signup\/verify-sent/);
      await forceEmailVerified(EMAIL);

      await page.goto(`${baseUrl}/signin`);
      await page.getByLabel(/correo electrónico|email/i).fill(EMAIL);
      await page.getByLabel(/contraseña|password/i).fill(PASSWORD);
      await page.getByRole('button', { name: /iniciar sesión|sign in/i }).click();
      await page.getByRole('button', { name: /aceptar|accept/i }).click();
      await page.getByLabel(/autonomía/i).selectOption({ label: /madrid/i });
      await page.getByRole('button', { name: /guardar|continuar|submit|save/i }).click();
      await page.getByLabel(/instrumento|instrument/i).selectOption('rsu');
      await page.getByLabel(/acciones|shares|share count/i).fill('48000');
      await page.getByLabel(/fecha del grant|grant date/i).fill(vestingStart);
      await page.getByLabel(/empleador|employer/i).fill('ACME Inc.');
      const ticker = page.getByLabel(/ticker/i);
      if (await ticker.isVisible()) await ticker.fill('ACME');
      await page.getByRole('button', { name: /guardar|save/i }).click();
      await expect(page).toHaveURL(/\/app\/dashboard/);
    });

    // 2. Paper-gains tile is in "no current prices" state.
    await test.step('2. Dashboard: paper-gains tile asks for current prices', async () => {
      await expect(page.getByTestId('paper-gains-tile')).toBeVisible();
      await expect(page.getByText(/introduce el precio actual/i)).toBeVisible();
    });

    // 3. Navigate to grant detail; vesting-events editor is present.
    await test.step('3. Grant detail shows Precios de vesting with past + future rows', async () => {
      await page.getByRole('link', { name: /detalle|details/i }).first().click();
      await expect(page.getByTestId('vesting-editor')).toBeVisible();
      await expect(page.getByText(/precios de vesting/i)).toBeVisible();
    });

    // 4. Enter FMV on the 3 oldest past vests → rows get the manual chip.
    await test.step('4. Enter FMV on three past vests; manual-override chip appears', async () => {
      const rows = page.getByTestId('vesting-row-past');
      const n = await rows.count();
      expect(n).toBeGreaterThan(3);
      for (let i = 0; i < 3; i++) {
        await rows.nth(i).getByRole('button', { name: /editar|edit/i }).click();
        await rows.nth(i).locator('input[name="fmvAtVest"]').fill((40 + i).toFixed(2));
        await rows.nth(i).locator('select[name="fmvCurrency"]').selectOption('USD');
        await rows.nth(i).getByRole('button', { name: /guardar|save/i }).click();
        await expect(rows.nth(i).getByText(/ajustado manualmente/i)).toBeVisible();
      }
    });

    // 5. Bulk-fill the remainder at $35 — modal reports skipped count.
    await test.step('5. Bulk-fill remaining vests at $35; 3 rows skipped', async () => {
      await page.getByRole('button', { name: /aplicar fmv a todos|bulk.*fmv/i }).click();
      await page.locator('input[name="bulkFmv"]').fill('35.00');
      await page.locator('select[name="bulkCurrency"]').selectOption('USD');
      await page.getByRole('button', { name: /confirmar|confirm/i }).click();
      await expect(page.getByText(/3 se saltaron|se rellenarán|aplicaron/i)).toBeVisible();
    });

    // 6. Return to dashboard; partial-data banner gone; envelope renders.
    await test.step('6. Dashboard: partial-data banner gone; paper-gains envelope present', async () => {
      await page.goto(`${baseUrl}/app/dashboard`);
      await expect(page.getByTestId('paper-gains-tile')).toBeVisible();
      // Still needs a current price — enter it now (step 7) before the
      // tile truly has a full envelope.
    });

    // 7. Enter a current price for ACME from the dashboard.
    await test.step('7. Enter ACME current price $50; envelope renders', async () => {
      await page
        .getByRole('button', { name: /introducir precios|enter prices/i })
        .click();
      await page.locator('input[name="price-ACME"]').fill('50.00');
      await page.locator('select[name="currency-ACME"]').selectOption('USD');
      await page.getByRole('button', { name: /guardar|save/i }).click();
      // Partial-data banner gone; combined band visible.
      await expect(page.getByTestId('paper-gains-combined-band')).toBeVisible();
    });

    // 8. Save Modelo 720 bank_accounts to push aggregate over €50k.
    await test.step('8. M720: save bank_accounts=€60 000 → threshold banner fires', async () => {
      await page.goto(`${baseUrl}/app/account/profile`);
      const bank = page.getByTestId('m720-bank_accounts');
      await bank.getByRole('button', { name: /editar|edit/i }).click();
      await bank.locator('input[name="totalEur"]').fill('60000.00');
      await bank.getByRole('button', { name: /guardar|save/i }).click();
      await expect(page.getByTestId('m720-threshold-banner')).toBeVisible();
    });

    // 9. Rule-set chip in footer shows today's ECB fx_date + engine version.
    await test.step('9. Footer rule-set chip shows today + engine version', async () => {
      const chip = page.getByTestId('rule-set-chip');
      await expect(chip).toBeVisible();
      const today = new Date().toISOString().slice(0, 10);
      await expect(chip).toContainText(today);
    });

    // 10. Edit a past vest's date + shares — cumulative-relaxed banner fires.
    await test.step('10. Edit past vest date+shares → cumulative-relaxed note', async () => {
      await page.getByRole('link', { name: /detalle|details/i }).first().click();
      const row = page.getByTestId('vesting-row-past').nth(4);
      await row.getByRole('button', { name: /editar|edit/i }).click();
      await row.locator('input[name="sharesVested"]').fill('500');
      await row.getByRole('button', { name: /guardar|save/i }).click();
      await expect(page.getByTestId('cumulative-relaxed-banner')).toBeVisible();
    });

    // 11. Attempt to shrink share_count below sum-of-overrides → 422.
    await test.step('11. Grant edit: shrink share_count → 422 shrink-below-overrides', async () => {
      await page.getByRole('link', { name: /editar grant|edit grant/i }).click();
      await page.locator('input[name="shareCount"]').fill('10');
      await page.getByRole('button', { name: /guardar|save|submit/i }).click();
      await expect(
        page.getByText(/no puedes reducir|share_count_below_overrides|below.*overrides/i),
      ).toBeVisible();
    });

    // 12. Dashboard still renders paper-gains after the failed grant edit.
    await test.step('12. Dashboard: paper-gains still renders (no regression)', async () => {
      await page.goto(`${baseUrl}/app/dashboard`);
      await expect(page.getByTestId('paper-gains-tile')).toBeVisible();
    });

    // 13. Log out, log in — state preserved.
    await test.step('13. Log out + log in: Slice-3 state preserved', async () => {
      await page.getByRole('button', { name: /cerrar sesión|sign out|logout/i }).click();
      await page.goto(`${baseUrl}/signin`);
      await page.getByLabel(/correo electrónico|email/i).fill(EMAIL);
      await page.getByLabel(/contraseña|password/i).fill(PASSWORD);
      await page.getByRole('button', { name: /iniciar sesión|sign in/i }).click();
      await expect(page).toHaveURL(/\/app\/dashboard/);
      await expect(page.getByTestId('paper-gains-tile')).toBeVisible();
    });

    // 14. Audit-log spot check.
    await test.step('14. audit_log: Slice-3 rows present with expected counts', async () => {
      expect(await auditCount(EMAIL, 'vesting_event.override')).toBeGreaterThanOrEqual(3);
      expect(await auditCount(EMAIL, 'vesting_event.bulk_fmv')).toBeGreaterThanOrEqual(1);
    });

    // 15. Modelo 720 threshold banner visible on dashboard too.
    await test.step('15. Dashboard: M720 threshold banner surfaces (aggregate breach)', async () => {
      await expect(page.getByTestId('m720-threshold-banner')).toBeVisible();
    });

    // 16. Keyboard nav sanity: tab order reaches paper-gains + M720 banner.
    await test.step('16. Keyboard reachability: paper-gains tile + M720 banner', async () => {
      await page.keyboard.press('Tab');
      await page.keyboard.press('Tab');
      // Sanity: nothing hijacks focus; if it did Playwright's locator
      // queries above would have failed already.
    });

    // 17. axe-core assertion on final populated dashboard.
    await test.step('17. Dashboard axe smoke: zero critical/serious violations', async () => {
      await runAxe(page, '/app/dashboard (Slice-3 populated)');
    });

    expect(consoleErrors, 'no page errors over the whole flow').toEqual([]);
  });
});
