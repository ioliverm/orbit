// axe-core smoke on the Slice-1 critical-path screens (G-21).
//
// Scope chosen to keep Vitest affordable: the four user touchpoints the
// 17-step demo script walks through first (signup, signin, dashboard with a
// seeded grant, grant detail with the same grant). The T15 Playwright spec
// carries the full-stack a11y assertions (keyboard nav, reduced motion,
// focus ring) against a live server; this file is the pre-E2E smoke that
// fails fast during local unit-test cycles.
//
// We use axe-core directly (not vitest-axe) to avoid a transitive peer-dep
// mismatch with our Vitest 2.x. The call shape is:
//   axe.run(container) -> { violations: Result[] }
// and we fail on anything marked `critical` or `serious`.

import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import axe from 'axe-core';
import { screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { I18nProvider } from '@lingui/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render } from '@testing-library/react';
import { i18n, activateLocale } from '../../i18n';
import SignupPage from '../../routes/auth/signup';
import SigninPage from '../../routes/auth/signin';
import DashboardPage from '../../routes/app/dashboard';
import GrantDetailPage from '../../routes/app/grants/detail';
import EsppPurchaseNewPage from '../../routes/app/grants/espp-new';
import TripNewPage from '../../routes/app/trips/new';
import TripsIndexPage from '../../routes/app/trips';
import SessionsPage from '../../routes/account/sessions';
import ProfilePage from '../../routes/account/profile';
import { useAuthStore } from '../../store/auth';
import { useLocaleStore } from '../../store/locale';
import type { GrantDto } from '../../api/grants';

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

function renderPage(node: JSX.Element, path = '/'): HTMLElement {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
  });
  const { container } = render(
    <I18nProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={[path]}>{node}</MemoryRouter>
      </QueryClientProvider>
    </I18nProvider>,
  );
  return container;
}

function seedAuthed(): void {
  useAuthStore.setState({
    user: { id: 'u-1', email: 'maria@example.com', locale: 'es-ES', primaryCurrency: 'EUR' },
    residency: {
      jurisdiction: 'ES',
      subJurisdiction: 'ES-MD',
      regimeFlags: [],
      fromDate: '2026-04-19',
    },
    onboardingStage: 'complete',
    disclaimerAccepted: true,
    loading: false,
    initialized: true,
  });
  useLocaleStore.setState({ locale: 'es-ES' });
}

function grantFixture(overrides: Partial<GrantDto> = {}): GrantDto {
  return {
    id: 'g-1',
    instrument: 'rsu',
    grantDate: '2024-09-15',
    shareCount: '30000',
    shareCountScaled: 30_000 * 10_000,
    strikeAmount: null,
    strikeCurrency: null,
    vestingStart: '2024-09-15',
    vestingTotalMonths: 48,
    cliffMonths: 12,
    vestingCadence: 'monthly',
    doubleTrigger: true,
    liquidityEventDate: null,
    doubleTriggerSatisfiedBy: null,
    employerName: 'ACME Inc.',
    ticker: null,
    notes: null,
    createdAt: '2026-04-19T00:00:00Z',
    updatedAt: '2026-04-19T00:00:00Z',
    ...overrides,
  };
}

function mockGrantsList(grants: GrantDto[], extra?: Record<string, unknown>): void {
  const spy = vi.fn(async (url: string) => {
    if (url.endsWith('/api/v1/grants')) {
      return new Response(JSON.stringify({ grants }), { status: 200 });
    }
    if (extra) {
      for (const [suffix, body] of Object.entries(extra)) {
        if (url.includes(suffix)) {
          return new Response(JSON.stringify(body), { status: 200 });
        }
      }
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
}

/**
 * Run axe on the container and assert zero `critical` or `serious`
 * violations (G-21). `moderate` and `minor` are allowed — they correspond
 * to best-practice rules that we track outside the smoke budget.
 */
async function assertNoCriticalOrSeriousViolations(container: HTMLElement, label: string) {
  // axe-core runs against the real DOM node; jsdom is sufficient for static
  // structural rules (heading order, labels, landmarks). We disable the
  // `color-contrast` rule because jsdom cannot render real styles (no
  // canvas getContext), and the Playwright smoke in `frontend/e2e/slice-1-
  // demo.spec.ts` runs against a real browser and covers color contrast
  // properly.
  const results = await axe.run(container, {
    // Cap to WCAG 2.1 AA per UX — matches the gate the demo script asserts.
    runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] },
    resultTypes: ['violations'],
    rules: { 'color-contrast': { enabled: false } },
  });
  const blocking = results.violations.filter(
    (v) => v.impact === 'critical' || v.impact === 'serious',
  );
  if (blocking.length > 0) {
    const summary = blocking
      .map(
        (v) =>
          `  - [${v.impact}] ${v.id}: ${v.help}\n    nodes: ${v.nodes
            .map((n) => n.target.join(' > '))
            .join(' | ')}`,
      )
      .join('\n');
    throw new Error(`${label}: ${blocking.length} axe violations\n${summary}`);
  }
  expect(blocking).toEqual([]);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

beforeAll(() => {
  activateLocale('es-ES');
});

afterEach(() => {
  vi.unstubAllGlobals();
  useAuthStore.getState().clear();
});

describe('axe-core smoke (G-21) — critical-path screens', () => {
  it('/signup has no critical or serious violations', async () => {
    const container = renderPage(<SignupPage />, '/signup');
    await assertNoCriticalOrSeriousViolations(container, '/signup');
  });

  it('/signin has no critical or serious violations', async () => {
    const container = renderPage(<SigninPage />, '/signin');
    await assertNoCriticalOrSeriousViolations(container, '/signin');
  });

  it('/app/dashboard with one grant has no critical or serious violations', async () => {
    seedAuthed();
    mockGrantsList([grantFixture()]);
    const container = renderPage(<DashboardPage />, '/app/dashboard');
    await waitFor(() => {
      expect(screen.getByText('ACME Inc.')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(container, '/app/dashboard');
  });

  it('/app/dashboard empty state has no critical or serious violations', async () => {
    seedAuthed();
    mockGrantsList([]);
    const container = renderPage(<DashboardPage />, '/app/dashboard');
    await waitFor(() => {
      expect(screen.getByText(/tu cartera está vacía/i)).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(container, '/app/dashboard (empty)');
  });

  it('/app/grants/:id detail has no critical or serious violations', async () => {
    seedAuthed();
    const g = grantFixture();
    mockGrantsList([g], {
      [`/api/v1/grants/${g.id}/vesting`]: { vestingEvents: [] },
      [`/api/v1/grants/${g.id}`]: { grant: g },
    });
    const container = renderPage(
      <Routes>
        <Route path="/app/grants/:id" element={<GrantDetailPage />} />
      </Routes>,
      `/app/grants/${g.id}`,
    );
    await waitFor(() => {
      // Wait for either the grant detail heading, the employer name, or a
      // known fallback prose to appear.
      expect(screen.getByRole('heading', { level: 1 })).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(container, '/app/grants/:id');
  });

  it('/app/grants/:id/espp-purchases/new has no critical or serious violations', async () => {
    seedAuthed();
    const g = grantFixture({
      instrument: 'espp',
      notes: JSON.stringify({ estimated_discount_percent: 15 }),
    });
    mockGrantsList([g], {
      [`/api/v1/grants/${g.id}`]: { grant: g },
    });
    const container = renderPage(
      <Routes>
        <Route
          path="/app/grants/:grantId/espp-purchases/new"
          element={<EsppPurchaseNewPage />}
        />
      </Routes>,
      `/app/grants/${g.id}/espp-purchases/new`,
    );
    await waitFor(() => {
      expect(document.querySelector('input[name="offeringDate"]')).not.toBeNull();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/grants/:id/espp-purchases/new',
    );
  });

  it('/app/trips list has no critical or serious violations', async () => {
    seedAuthed();
    mockGrantsList([], {
      '/api/v1/trips': {
        trips: [],
        annualCapTracker: {
          year: 2026,
          tripCount: 0,
          dayCountDeclared: 0,
          employerPaidTripCount: 0,
          criteriaMetCountByKey: {
            services_outside_spain: 0,
            non_spanish_employer: 0,
            not_tax_haven: 0,
            no_double_exemption: 0,
            within_annual_cap: 0,
          },
        },
      },
    });
    const container = renderPage(<TripsIndexPage />, '/app/trips');
    await waitFor(() => {
      expect(screen.getByTestId('annual-cap-tracker')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(container, '/app/trips');
  });

  it('/app/trips/new has no critical or serious violations', async () => {
    seedAuthed();
    mockGrantsList([]);
    const container = renderPage(<TripNewPage />, '/app/trips/new');
    await waitFor(() => {
      expect(
        document.querySelector('select[name="destinationCountry"]'),
      ).not.toBeNull();
    });
    await assertNoCriticalOrSeriousViolations(container, '/app/trips/new');
  });

  it('/app/account/profile (M720 embedded) has no critical or serious violations', async () => {
    seedAuthed();
    mockGrantsList([], {
      '/modelo-720-inputs?category=bank_accounts': { history: [] },
      '/modelo-720-inputs?category=real_estate': { history: [] },
      '/user-tax-preferences/current': { current: null },
      '/user-tax-preferences': { preferences: [] },
    });
    const container = renderPage(<ProfilePage />, '/app/account/profile');
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: /perfil y residencia/i }),
      ).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/account/profile (M720)',
    );
  });

  // Slice 3b T39 — Profile with Preferencias fiscales seeded (ES + 45% + STC on).
  it('/app/account/profile with Preferencias fiscales seeded has no violations', async () => {
    seedAuthed();
    const current = {
      id: 'p-1',
      countryIso2: 'ES',
      rendimientoDelTrabajoPercent: '0.4500',
      sellToCoverEnabled: true,
      fromDate: '2026-04-19',
      toDate: null,
      createdAt: '2026-04-19T00:00:00Z',
      updatedAt: '2026-04-19T00:00:00Z',
    };
    mockGrantsList([], {
      '/modelo-720-inputs?category=bank_accounts': { history: [] },
      '/modelo-720-inputs?category=real_estate': { history: [] },
      '/user-tax-preferences/current': { current },
      '/user-tax-preferences': { preferences: [current] },
    });
    const container = renderPage(<ProfilePage />, '/app/account/profile');
    await waitFor(() => {
      expect(screen.getByTestId('tax-preferences-section')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/account/profile (preferencias fiscales seeded)',
    );
  });

  // Slice 3b T39 — grant-detail with the vesting-event dialog open
  // (derived-panel populated + pills + focus-trap structural landmarks).
  it('/app/grants/:id with VestingEventDialog open (populated) has no violations', async () => {
    seedAuthed();
    const g = grantFixture({ ticker: 'ACME' });
    mockGrantsList([g], {
      [`/api/v1/grants/${g.id}/vesting`]: {
        vestingEvents: [
          {
            id: 'e-1',
            vestDate: '2025-10-15',
            sharesVestedThisEvent: '100',
            sharesVestedThisEventScaled: 1_000_000,
            cumulativeSharesVested: '100',
            cumulativeSharesVestedScaled: 1_000_000,
            state: 'vested',
            fmvAtVest: '42.0000',
            fmvCurrency: 'USD',
            isUserOverride: true,
            updatedAt: '2026-04-19T00:00:00Z',
            taxWithholdingPercent: '0.4500',
            shareSellPrice: '42.2500',
            shareSellCurrency: 'USD',
            isSellToCoverOverride: true,
          },
        ],
        vestedToDate: '100',
        vestedToDateScaled: 1_000_000,
        awaitingLiquidity: '0',
        awaitingLiquidityScaled: 0,
      },
      [`/api/v1/grants/${g.id}`]: { grant: g, overridesWarning: true, overrideCount: 1 },
      '/current-price-override': { override: null },
      '/user-tax-preferences/current': { current: null },
      '/rule-set-chip': { fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' },
    });
    const container = renderPage(
      <Routes>
        <Route path="/app/grants/:grantId" element={<GrantDetailPage />} />
      </Routes>,
      `/app/grants/${g.id}`,
    );
    await waitFor(() => {
      expect(screen.getByTestId('vesting-editor')).toBeInTheDocument();
    });
    // Open the dialog for the first row.
    const editBtn = await screen.findByTestId('vesting-row-edit');
    editBtn.click();
    await waitFor(() => {
      expect(screen.getByTestId('vesting-dialog')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/grants/:id (dialog open + derived panel populated)',
    );
  });

  it('/app/grants/:id with VestingEventDialog open (empty derived panel) has no violations', async () => {
    seedAuthed();
    const g = grantFixture({ ticker: 'ACME' });
    mockGrantsList([g], {
      [`/api/v1/grants/${g.id}/vesting`]: {
        vestingEvents: [
          {
            id: 'e-1',
            vestDate: '2025-10-15',
            sharesVestedThisEvent: '100',
            sharesVestedThisEventScaled: 1_000_000,
            cumulativeSharesVested: '100',
            cumulativeSharesVestedScaled: 1_000_000,
            state: 'vested',
            fmvAtVest: null,
            fmvCurrency: null,
            isUserOverride: false,
            updatedAt: '2026-04-19T00:00:00Z',
          },
        ],
        vestedToDate: '0',
        vestedToDateScaled: 0,
        awaitingLiquidity: '0',
        awaitingLiquidityScaled: 0,
      },
      [`/api/v1/grants/${g.id}`]: { grant: g },
      '/current-price-override': { override: null },
      '/user-tax-preferences/current': { current: null },
      '/rule-set-chip': { fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' },
    });
    const container = renderPage(
      <Routes>
        <Route path="/app/grants/:grantId" element={<GrantDetailPage />} />
      </Routes>,
      `/app/grants/${g.id}`,
    );
    await waitFor(() => {
      expect(screen.getByTestId('vesting-editor')).toBeInTheDocument();
    });
    const editBtn = await screen.findByTestId('vesting-row-edit');
    editBtn.click();
    await waitFor(() => {
      expect(screen.getByTestId('vesting-dialog')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/grants/:id (dialog open + derived panel empty)',
    );
  });

  it('/app/account/sessions has no critical or serious violations', async () => {
    seedAuthed();
    mockGrantsList([], {
      '/api/v1/auth/sessions': {
        sessions: [
          {
            id: 's-1',
            userAgent: 'Firefox 128 · macOS',
            countryIso2: 'ES',
            createdAt: '2026-04-18T09:12:00Z',
            lastUsedAt: '2026-04-19T20:00:00Z',
            isCurrent: true,
          },
        ],
      },
    });
    const container = renderPage(<SessionsPage />, '/app/account/sessions');
    await waitFor(() => {
      expect(screen.getByTestId('session-row')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(container, '/app/account/sessions');
  });

  // T23 extension: a populated dashboard with two employers + three
  // grants (one mixed-instrument pair at ACME + one NSO at Bravo). This
  // exercises the per-employer panel, the drill-down rows, and the
  // multi-tile grid in one render — the Slice-1 axe test above covered
  // only the single-grant case.
  // T30: paper-gains dashboard seeded + partial-data banner present.
  it('/app/dashboard with paper-gains + partial-data banner has no violations', async () => {
    seedAuthed();
    const g1 = grantFixture({
      id: 'g-acme-rsu',
      instrument: 'rsu',
      employerName: 'ACME Inc.',
      ticker: 'ACME',
    });
    const paperGainsPayload = {
      perGrant: [{ grantId: g1.id, complete: false, missingReason: 'fmv_missing' }],
      combinedEurBand: null,
      incompleteGrants: [{ grantId: g1.id, employer: 'ACME Inc.', instrument: 'rsu' }],
      stalenessFx: 'fresh' as const,
      fxDate: '2026-04-17',
    };
    mockGrantsList([g1], {
      '/dashboard/stacked': { byEmployer: [], combined: [] },
      '/dashboard/paper-gains': paperGainsPayload,
      '/dashboard/modelo-720-threshold': {
        bankAccountsEur: '10000.00',
        realEstateEur: null,
        securitiesEur: '0.00',
        perCategoryBreach: false,
        aggregateBreach: false,
        thresholdEur: '50000.00',
        fxSensitivityBand: null,
        fxDate: '2026-04-17',
      },
      '/api/v1/current-prices': {
        prices: [{ ticker: 'ACME', price: '50.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' }],
      },
      '/rule-set-chip': { fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' },
    });
    const container = renderPage(<DashboardPage />, '/app/dashboard');
    await waitFor(() => {
      expect(screen.getByTestId('paper-gains-tile')).toBeInTheDocument();
      expect(screen.getByTestId('paper-gains-partial-banner')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(container, '/app/dashboard (paper-gains)');
  });

  it('/app/grants/:id with vesting editor + price override has no violations', async () => {
    seedAuthed();
    const g = grantFixture({ ticker: 'ACME' });
    mockGrantsList([g], {
      [`/api/v1/grants/${g.id}/vesting`]: {
        vestingEvents: [
          {
            id: 'e-1',
            vestDate: '2025-10-15',
            sharesVestedThisEvent: '500',
            sharesVestedThisEventScaled: 5_000_000,
            cumulativeSharesVested: '500',
            cumulativeSharesVestedScaled: 5_000_000,
            state: 'vested',
            fmvAtVest: '40.00',
            fmvCurrency: 'USD',
            isUserOverride: true,
            updatedAt: '2026-04-19T00:00:00Z',
          },
        ],
        vestedToDate: '500',
        vestedToDateScaled: 5_000_000,
        awaitingLiquidity: '0',
        awaitingLiquidityScaled: 0,
      },
      [`/api/v1/grants/${g.id}`]: { grant: g, overridesWarning: true, overrideCount: 1 },
      '/current-price-override': { override: null },
      '/rule-set-chip': { fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' },
    });
    const container = renderPage(
      <Routes>
        <Route path="/app/grants/:grantId" element={<GrantDetailPage />} />
      </Routes>,
      `/app/grants/${g.id}`,
    );
    await waitFor(() => {
      expect(screen.getByTestId('vesting-editor')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/grants/:id (vesting editor)',
    );
  });

  it('/app/dashboard with seeded multi-employer multi-instrument state has no violations', async () => {
    seedAuthed();
    const g1 = grantFixture({
      id: 'g-acme-rsu',
      instrument: 'rsu',
      shareCount: '30000',
      shareCountScaled: 30_000 * 10_000,
      employerName: 'ACME Inc.',
    });
    const g2 = grantFixture({
      id: 'g-acme-nso',
      instrument: 'nso',
      shareCount: '10000',
      shareCountScaled: 10_000 * 10_000,
      strikeAmount: '8.00',
      strikeCurrency: 'USD',
      employerName: 'ACME Inc.',
      grantDate: '2024-08-15',
      vestingStart: '2024-08-15',
    });
    const g3 = grantFixture({
      id: 'g-bravo-nso',
      instrument: 'nso',
      shareCount: '5000',
      shareCountScaled: 5_000 * 10_000,
      strikeAmount: '10.00',
      strikeCurrency: 'USD',
      employerName: 'Bravo Corp.',
      grantDate: '2025-01-15',
      vestingStart: '2025-01-15',
    });
    // Provide a populated /dashboard/stacked response so the
    // `EmployerPortfolioPanel` actually renders its rows. Shapes match
    // `WireEmployerStack` + `WireStackedPoint` (scaled numeric fields as
    // plain numbers — the wrapper coerces them with `toBigInt`).
    const stackedPayload = {
      byEmployer: [
        {
          employerName: 'ACME Inc.',
          employerKey: 'acme inc.',
          grantIds: [g1.id, g2.id],
          points: [
            {
              date: '2024-10-15',
              cumulativeSharesVested: 30_000 * 10_000,
              cumulativeTimeVestedAwaitingLiquidity: 0,
              perGrantBreakdown: [
                {
                  grantId: g1.id,
                  instrument: 'rsu',
                  sharesVestedThisEvent: 30_000 * 10_000,
                  cumulativeForThisGrant: 30_000 * 10_000,
                  state: 'vested' as const,
                },
              ],
            },
          ],
        },
        {
          employerName: 'Bravo Corp.',
          employerKey: 'bravo corp.',
          grantIds: [g3.id],
          points: [],
        },
      ],
      combined: [],
    };
    mockGrantsList([g1, g2, g3], {
      '/dashboard/stacked': stackedPayload,
    });
    const container = renderPage(<DashboardPage />, '/app/dashboard');
    await waitFor(() => {
      // `ACME Inc.` appears twice (employer-panel header + grant-tile
      // employer line), and Bravo Corp. appears twice too. `getAllByText`
      // + a length check is the stable shape.
      expect(screen.getAllByText('ACME Inc.').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Bravo Corp.').length).toBeGreaterThan(0);
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/dashboard (seeded multi-grant)',
    );
  });

  // T31 extension: dashboard with paper-gains state covering BOTH
  // complete and incomplete grants. Exercises the combined band
  // render + the per-grant incomplete row + the partial-data banner
  // link targets in one jsdom pass.
  it('/app/dashboard with mixed complete + incomplete paper-gains has no violations', async () => {
    seedAuthed();
    const gComplete = grantFixture({
      id: 'g-complete',
      instrument: 'rsu',
      employerName: 'ACME Inc.',
      ticker: 'ACME',
    });
    const gIncomplete = grantFixture({
      id: 'g-incomplete',
      instrument: 'rsu',
      employerName: 'Bravo Corp.',
      ticker: 'BETA',
    });
    const paperGainsPayload = {
      perGrant: [
        {
          grantId: gComplete.id,
          complete: true,
          gainNative: '1800.0000',
          gainEurBand: { low: '1572.30', mid: '1597.05', high: '1620.00' },
        },
        { grantId: gIncomplete.id, complete: false, missingReason: 'fmv_missing' },
      ],
      combinedEurBand: { low: '1572.30', mid: '1597.05', high: '1620.00' },
      incompleteGrants: [
        { grantId: gIncomplete.id, employer: 'Bravo Corp.', instrument: 'rsu' },
      ],
      stalenessFx: 'fresh' as const,
      fxDate: '2026-04-17',
    };
    mockGrantsList([gComplete, gIncomplete], {
      '/dashboard/stacked': { byEmployer: [], combined: [] },
      '/dashboard/paper-gains': paperGainsPayload,
      '/dashboard/modelo-720-threshold': {
        bankAccountsEur: '30000.00',
        realEstateEur: null,
        securitiesEur: '1800.00',
        perCategoryBreach: false,
        aggregateBreach: false,
        thresholdEur: '50000.00',
        fxSensitivityBand: null,
        fxDate: '2026-04-17',
      },
      '/api/v1/current-prices': {
        prices: [
          { ticker: 'ACME', price: '50.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' },
          { ticker: 'BETA', price: '75.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' },
        ],
      },
      '/rule-set-chip': { fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' },
    });
    const container = renderPage(<DashboardPage />, '/app/dashboard');
    await waitFor(() => {
      expect(screen.getByTestId('paper-gains-tile')).toBeInTheDocument();
      expect(screen.getByTestId('paper-gains-partial-banner')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/dashboard (paper-gains complete + incomplete)',
    );
  });

  // T31 extension: grant-detail with a fully-populated "Precios de
  // vesting" table — past vests carrying manual overrides AND
  // future vests still without FMV. Exercises the override-pill
  // render, the edit-in-place affordances, and the
  // cumulative-relaxed banner when any override changed shares.
  it('/app/grants/:id with populated vesting editor (past + future) has no violations', async () => {
    seedAuthed();
    const g = grantFixture({
      ticker: 'ACME',
      vestingTotalMonths: 12,
      cliffMonths: 0,
      shareCount: '12000',
      shareCountScaled: 12_000 * 10_000,
    });
    mockGrantsList([g], {
      [`/api/v1/grants/${g.id}/vesting`]: {
        vestingEvents: [
          {
            id: 'e-past-1',
            vestDate: '2025-02-15',
            sharesVestedThisEvent: '1000',
            sharesVestedThisEventScaled: 10_000_000,
            cumulativeSharesVested: '1000',
            cumulativeSharesVestedScaled: 10_000_000,
            state: 'vested',
            fmvAtVest: '40.00',
            fmvCurrency: 'USD',
            isUserOverride: true,
            updatedAt: '2026-04-19T00:00:00Z',
          },
          {
            id: 'e-past-2',
            vestDate: '2025-05-15',
            sharesVestedThisEvent: '900',
            sharesVestedThisEventScaled: 9_000_000,
            cumulativeSharesVested: '1900',
            cumulativeSharesVestedScaled: 19_000_000,
            state: 'vested',
            fmvAtVest: '42.00',
            fmvCurrency: 'USD',
            isUserOverride: true,
            updatedAt: '2026-04-19T00:00:00Z',
          },
          {
            id: 'e-future-1',
            vestDate: '2026-08-15',
            sharesVestedThisEvent: '1000',
            sharesVestedThisEventScaled: 10_000_000,
            cumulativeSharesVested: '2900',
            cumulativeSharesVestedScaled: 29_000_000,
            state: 'upcoming',
            fmvAtVest: null,
            fmvCurrency: null,
            isUserOverride: false,
            updatedAt: '2026-04-19T00:00:00Z',
          },
        ],
        vestedToDate: '1900',
        vestedToDateScaled: 19_000_000,
        awaitingLiquidity: '0',
        awaitingLiquidityScaled: 0,
      },
      [`/api/v1/grants/${g.id}`]: { grant: g, overridesWarning: true, overrideCount: 2 },
      '/current-price-override': { override: null },
      '/rule-set-chip': { fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' },
    });
    const container = renderPage(
      <Routes>
        <Route path="/app/grants/:grantId" element={<GrantDetailPage />} />
      </Routes>,
      `/app/grants/${g.id}`,
    );
    await waitFor(() => {
      expect(screen.getByTestId('vesting-editor')).toBeInTheDocument();
    });
    await assertNoCriticalOrSeriousViolations(
      container,
      '/app/grants/:id (vesting editor past+future populated)',
    );
  });
});
