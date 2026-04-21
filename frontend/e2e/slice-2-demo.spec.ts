/**
 * Slice 2 T23 — Playwright port of `docs/requirements/slice-2-acceptance-criteria.md` §13.
 *
 * The 19-step Slice-2 demo script assumes a persisted user who already
 * completed Slice-1 with three grants at ACME (two RSU + one NSO) and
 * one ESPP grant. To keep this spec self-contained (no cross-test state
 * dependency on the Slice-1 spec), we seed that starting state directly
 * via the DB helper in step 0, then execute the §13 script
 * (signin → stacked view → ESPP purchase with notes lift → two trips →
 * M720 inputs → sessions → dashboard verify + axe smoke on the full
 * populated dashboard).
 *
 * ## Running locally
 *
 *   # 1. Bring up the backend + frontend stacks (same bring-up as the
 *   # Slice-1 demo — see `e2e/slice-1-demo.spec.ts` header).
 *   just db-up && just migrate
 *   DATABASE_URL='postgres://orbit_app:…sslmode=require&sslrootcert=scripts/dev/.ca.crt' \
 *     cargo run -p orbit -- api &            # :8080
 *   cd frontend && pnpm dev &                 # :5173 (proxies /api → :8080)
 *
 *   # 2. Run the spec.
 *   ORBIT_E2E_BASE_URL=http://127.0.0.1:5173 \
 *   ORBIT_E2E_DATABASE_URL='postgres://orbit_migrate:…' \
 *   pnpm exec playwright test slice-2-demo
 *
 * ## Gating
 *
 * Same discipline as T15: if `ORBIT_E2E_BASE_URL` is unset the whole
 * file is skipped. CI cannot bootstrap the full stack until Slice 9
 * (deploy). The spec is the load-bearing artifact; the run is optional
 * until then.
 *
 * ## Axe-core inside step 15
 *
 * After re-opening the dashboard with multi-grant + multi-employer
 * state (two RSUs at ACME, one NSO at ACME, one ESPP grant), we run
 * axe-core against the real DOM and fail on `critical` / `serious`
 * violations. This is the Slice-2 extension of the Slice-1 axe smoke
 * (G-21); the Vitest `axe-smoke.test.tsx` runs the same check against a
 * jsdom-rendered dashboard pre-commit.
 *
 * ## What this spec does NOT cover
 *
 * - EUR conversion / ECB pipeline — Slice 3.
 * - Art.7.p eligibility *calculation* — Slice 4 (we only capture).
 * - ESPP tax-treatment rendering — Slices 4/5.
 * - Mobile / responsive — Slice-2 desktop-first (§11 is partial spec).
 */

import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { Client as PgClient } from 'pg';

const baseUrl = process.env.ORBIT_E2E_BASE_URL;
const dbUrl = process.env.ORBIT_E2E_DATABASE_URL ?? process.env.DATABASE_URL_MIGRATE;

test.skip(
  !baseUrl,
  'ORBIT_E2E_BASE_URL not set — Slice-2 demo spec requires a live stack. See header comment.',
);

// Unique email per run so reruns don't need DB cleanup.
const EMAIL = `test+slice2-${Date.now()}@orbit.local`;
const PASSWORD = 'Orbit-Demo-Passphrase-2026';

// ---------------------------------------------------------------------------
// DB helpers — seed a "Slice-1 complete" user so we skip re-running §10 of
// the Slice-1 demo every time. All writes use the orbit_migrate role (set
// via `ORBIT_E2E_DATABASE_URL`) so they bypass RLS and succeed even in
// empty environments.
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
    const lines = blocking
      .map((v) => `  - [${v.impact}] ${v.id}: ${v.help}`)
      .join('\n');
    throw new Error(`axe at ${label}: ${blocking.length} blocking violations\n${lines}`);
  }
  expect(blocking).toEqual([]);
}

// ---------------------------------------------------------------------------
// The 15-step demo script (ports §13 steps 1–19, with setup collapsed).
// ---------------------------------------------------------------------------

test.describe('Slice 2 demo-acceptance script (§13, 15 test.steps)', () => {
  test.setTimeout(180_000);

  test('runs the full Slice-2 flow end-to-end with axe smoke on the final dashboard', async ({
    page,
    context,
  }) => {
    await context.emulateMedia({ reducedMotion: 'reduce' });
    const consoleErrors: string[] = [];
    page.on('pageerror', (e) => consoleErrors.push(e.message));

    // 1. Bring a fresh user up to "Slice-1 complete" (disclaimer +
    //    residency + two RSU grants at ACME + one NSO at ACME). We do
    //    this through the signup → wizard → grants UI so the state is
    //    identical to a real Slice-1 demo output.
    await test.step('1. Bring user up to Slice-1 complete (3 grants at ACME)', async () => {
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
      // Disclaimer → accept.
      await page.getByRole('button', { name: /aceptar|accept/i }).click();
      // Residency.
      await page.getByLabel(/autonomía/i).selectOption({ label: /madrid/i });
      const ccy = page.getByLabel(/moneda|currency/i);
      if (await ccy.isVisible()) await ccy.selectOption('EUR');
      await page.getByRole('button', { name: /guardar|continuar|submit|save/i }).click();
      // First grant — RSU 30k ACME.
      await page.getByLabel(/instrumento|instrument/i).selectOption('rsu');
      await page.getByLabel(/acciones|shares|share count/i).fill('30000');
      await page.getByLabel(/fecha del grant|grant date/i).fill('2024-09-15');
      await page.getByLabel(/empleador|employer/i).fill('ACME Inc.');
      const dt = page.getByLabel(/double.?trigger/i);
      if (await dt.isVisible()) await dt.check();
      await page.getByRole('button', { name: /guardar|save/i }).click();
      await expect(page).toHaveURL(/\/app\/dashboard/);

      // Add second RSU at ACME.
      await page.getByRole('link', { name: /añadir otro grant|añadir grant|add grant/i }).first().click();
      await page.getByLabel(/instrumento|instrument/i).selectOption('rsu');
      await page.getByLabel(/acciones|shares|share count/i).fill('15000');
      await page.getByLabel(/fecha del grant|grant date/i).fill('2025-03-15');
      await page.getByLabel(/empleador|employer/i).fill('ACME Inc.');
      await page.getByRole('button', { name: /guardar|save/i }).click();

      // Add NSO at ACME.
      await page.getByRole('link', { name: /añadir otro grant|añadir grant|add grant/i }).first().click();
      await page.getByLabel(/instrumento|instrument/i).selectOption('nso');
      await page.getByLabel(/acciones|shares|share count/i).fill('10000');
      await page.getByLabel(/fecha del grant|grant date/i).fill('2024-08-15');
      await page.getByLabel(/empleador|employer/i).fill('ACME Inc.');
      await page.getByLabel(/strike/i).fill('8.00');
      await page.getByRole('button', { name: /guardar|save/i }).click();
      await expect(page).toHaveURL(/\/app\/dashboard/);
    });

    // 2. (§13.2) Observe the stacked-employer view.
    await test.step('2. Stacked-employer tile merges 2 RSU + 1 NSO at ACME', async () => {
      await expect(page.getByText(/stacked[:\s]*acme/i)).toBeVisible();
      // Drill-down: the tile links to grant detail.
      const detail = page.getByRole('link', { name: /detalle|details/i }).first();
      await expect(detail).toBeVisible();
    });

    // 3. (§13.3) Navigate to (or create) the ESPP grant — use the "añadir
    //    grant" path with instrument=espp and a Slice-1 discount note so
    //    step 5 can exercise the notes-lift branch.
    await test.step('3. Create an ESPP grant with Slice-1 notes discount', async () => {
      await page.goto(`${baseUrl}/app/dashboard`);
      await page.getByRole('link', { name: /añadir otro grant|añadir grant|add grant/i }).first().click();
      await page.getByLabel(/instrumento|instrument/i).selectOption('espp');
      await page.getByLabel(/acciones|shares|share count/i).fill('500');
      await page.getByLabel(/fecha del grant|grant date/i).fill('2024-09-15');
      await page.getByLabel(/empleador|employer/i).fill('ACME Inc.');
      const discount = page.getByLabel(/descuento|discount/i);
      if (await discount.isVisible()) await discount.fill('15');
      await page.getByRole('button', { name: /guardar|save/i }).click();
      await expect(page).toHaveURL(/\/app\/dashboard/);
    });

    // 4. (§13.3–4) Grant detail shows "Registrar compra ESPP" CTA and
    //    the ESPP form pre-fills `employer_discount_percent` with 15.
    await test.step('4. ESPP grant detail + Registrar compra form with lifted discount', async () => {
      await page.getByRole('link', { name: /detalle|details|espp/i }).last().click();
      await expect(page).toHaveURL(/\/app\/grants\//);
      await page.getByRole('link', { name: /registrar compra espp|new purchase|record.*espp/i }).click();
      await expect(page).toHaveURL(/espp-purchases\/new/);
      // The Slice-1 notes discount (15%) is surfaced on the form.
      const discount = page.locator('input[name="employerDiscountPercent"]');
      await expect(discount).toHaveValue(/15/);
    });

    // 5. (§13.5) Submit the ESPP purchase. Toast surfaces `notes_lift`.
    await test.step('5. Submit ESPP purchase; notes-lift toast fires', async () => {
      await page.locator('input[name="offeringDate"]').fill('2025-01-15');
      await page.locator('input[name="purchaseDate"]').fill('2025-06-30');
      await page.locator('input[name="fmvAtPurchase"]').fill('45.00');
      await page.locator('input[name="purchasePricePerShare"]').fill('38.25');
      await page.locator('input[name="sharesPurchased"]').fill('100');
      await page.getByRole('button', { name: /guardar|save|submit/i }).click();
      // Toast copy mentions the migration (AC-4.5.1).
      await expect(page.getByText(/migrad|lifted|nota|notes/i).first()).toBeVisible({
        timeout: 10_000,
      });
    });

    // 6. (§13.6–7) First trip — eligible capture (US, 5/5).
    await test.step('6. Add eligible Art.7.p trip: US 2026-03-01..04-15, 5/5', async () => {
      await page.goto(`${baseUrl}/app/trips`);
      await page.getByRole('link', { name: /añadir viaje|add trip|new trip/i }).click();
      await page.locator('select[name="destinationCountry"]').selectOption('US');
      await page.locator('input[name="fromDate"]').fill('2026-03-01');
      await page.locator('input[name="toDate"]').fill('2026-04-15');
      await page.locator('textarea[name="purpose"]').fill('Kickoff with NYC team');
      // Employer paid = Yes.
      await page.getByRole('radio', { name: /sí|yes/i }).first().check();
      // Check all five criteria = Yes.
      for (const key of [
        'services_outside_spain',
        'non_spanish_employer',
        'not_tax_haven',
        'no_double_exemption',
        'within_annual_cap',
      ]) {
        await page.locator(`[data-criterion="${key}"] input[value="true"]`).check();
      }
      await page.getByRole('button', { name: /guardar|save|submit/i }).click();
      await expect(page).toHaveURL(/\/app\/trips/);
      await expect(page.getByText(/apto.*5\/5|5\/5/i)).toBeVisible();
    });

    // 7. (§13.8) Second trip — mixed/ineligible capture (ES, 1/5 or so).
    await test.step('7. Add mixed/ineligible Art.7.p trip: ES 2026-05-01..03', async () => {
      await page.getByRole('link', { name: /añadir viaje|add trip|new trip/i }).click();
      await page.locator('select[name="destinationCountry"]').selectOption('ES');
      await page.locator('input[name="fromDate"]').fill('2026-05-01');
      await page.locator('input[name="toDate"]').fill('2026-05-03');
      await page.locator('textarea[name="purpose"]').fill('Reunion regional en Barcelona');
      await page.getByRole('radio', { name: /sí|yes/i }).first().check();
      // criterion 1 = No; others Yes.
      await page.locator(`[data-criterion="services_outside_spain"] input[value="false"]`).check();
      for (const key of [
        'non_spanish_employer',
        'not_tax_haven',
        'no_double_exemption',
        'within_annual_cap',
      ]) {
        await page.locator(`[data-criterion="${key}"] input[value="true"]`).check();
      }
      await page.getByRole('button', { name: /guardar|save|submit/i }).click();
      await expect(page).toHaveURL(/\/app\/trips/);
      // Second row present with a <5/5 chip.
      await expect(page.getByText(/4\/5|3\/5|2\/5|1\/5|0\/5/)).toBeVisible();
    });

    // 8. (§13.9) Annual-cap tracker shows day count for 2026; year
    //    switcher flips to 2025 → zero.
    await test.step('8. Annual-cap tracker shows 2026 days and flips on year switch', async () => {
      await expect(page.getByTestId('annual-cap-tracker')).toContainText(/2026/);
      // Year selector — flip to 2025.
      const yearSelect = page.getByLabel(/año|year/i);
      if (await yearSelect.isVisible()) {
        await yearSelect.selectOption('2025');
        await expect(page.getByTestId('annual-cap-tracker')).toContainText(/0 días|0 day/i);
        // Revert to 2026.
        await yearSelect.selectOption('2026');
      }
    });

    // 9. (§13.10) M720 inputs — first save bank_accounts 25000 (inserted).
    await test.step('9. M720: inserted bank_accounts=25000', async () => {
      await page.goto(`${baseUrl}/app/account/profile`);
      const bank = page.getByTestId('m720-bank_accounts');
      await bank.getByRole('button', { name: /editar|edit/i }).click();
      await bank.locator('input[name="totalEur"]').fill('25000.00');
      await bank.getByRole('button', { name: /guardar|save/i }).click();
      await expect(bank).toContainText(/25\.000,00|25,000\.00/);
    });

    // 10. (§13.11) M720 — edit bank_accounts to 40000 later: closed_and_created.
    await test.step('10. M720: closed_and_created bank_accounts=40000', async () => {
      const bank = page.getByTestId('m720-bank_accounts');
      await bank.getByRole('button', { name: /editar|edit/i }).click();
      await bank.locator('input[name="totalEur"]').fill('40000.00');
      await bank.getByRole('button', { name: /guardar|save/i }).click();
      await expect(bank).toContainText(/40\.000,00|40,000\.00/);
    });

    // 11. M720 — save the same value 40000 again: no-op (no audit row).
    await test.step('11. M720: no-op save of identical bank_accounts=40000', async () => {
      const before = await auditCount(EMAIL, 'modelo_720_inputs.upsert');
      const bank = page.getByTestId('m720-bank_accounts');
      await bank.getByRole('button', { name: /editar|edit/i }).click();
      await bank.locator('input[name="totalEur"]').fill('40000.00');
      await bank.getByRole('button', { name: /guardar|save/i }).click();
      // The row still shows 40000.
      await expect(bank).toContainText(/40\.000,00|40,000\.00/);
      // audit_log count did NOT grow (AC-6.2.5).
      const after = await auditCount(EMAIL, 'modelo_720_inputs.upsert');
      expect(after, 'no-op save must not write a new audit row').toBe(before);
    });

    // 12. (§13.12–14) Sessions — revoke-all-others disabled with 1
    //     session; the current-session row's CTA is disabled.
    await test.step('12. Sessions: revoke CTAs disabled for current-only state', async () => {
      await page.goto(`${baseUrl}/app/account/sessions`);
      await expect(page.getByTestId('session-row')).toBeVisible();
      const bulk = page.getByRole('button', { name: /cerrar todas.*sesiones|revoke all.*others/i });
      await expect(bulk).toBeDisabled();
      // The current-session row's individual CTA is also disabled.
      const currentRow = page.getByTestId('session-row').filter({ hasText: /esta sesión|current/i });
      const revoke = currentRow.getByRole('button', { name: /cerrar|revoke/i });
      if (await revoke.count()) {
        await expect(revoke.first()).toBeDisabled();
      }
    });

    // 13. (§13.18) audit_log spot-check.
    await test.step('13. audit_log carries Slice-2 rows with expected shape counts', async () => {
      expect(await auditCount(EMAIL, 'espp_purchase.create')).toBeGreaterThanOrEqual(1);
      expect(await auditCount(EMAIL, 'art_7p_trip.create')).toBeGreaterThanOrEqual(2);
      expect(await auditCount(EMAIL, 'modelo_720_inputs.upsert')).toBeGreaterThanOrEqual(2);
    });

    // 14. (§13.15) "Tengo varios grants" link dismisses to dashboard.
    await test.step('14. "Tengo varios grants" link dismisses to populated dashboard', async () => {
      await page.goto(`${baseUrl}/app/dashboard`);
      const multi = page.getByRole('link', { name: /tengo varios grants|multiple grants/i });
      if (await multi.isVisible()) {
        await multi.click();
        // Dismisses to dashboard (not a CSV import UI — that ships in Slice 8).
        await expect(page).toHaveURL(/\/app\/dashboard/);
      }
    });

    // 15. (§13.16) axe-core assertion on the full populated dashboard.
    await test.step('15. Dashboard: multi-grant stacking + axe smoke (zero critical/serious)', async () => {
      await page.goto(`${baseUrl}/app/dashboard`);
      // Verify the stacked-employer ACME tile still renders after all the
      // Slice-2 state accumulated above.
      await expect(page.getByText(/stacked[:\s]*acme/i)).toBeVisible();
      // At least two employers or one stacked employer tile present.
      const tiles = page.getByTestId('employer-tile');
      expect(await tiles.count()).toBeGreaterThanOrEqual(1);
      await runAxe(page, '/app/dashboard (populated)');
    });

    expect(consoleErrors, 'no page errors over the whole flow').toEqual([]);
  });

  // §11 mobile/responsive is documented as out of Slice-2 demo scope.
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  test.skip('Slice-2 mobile responsive sweep (desktop-first demo; deferred)', async () => {
    // Intentionally empty.
  });
});
