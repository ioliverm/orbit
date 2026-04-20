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
});
