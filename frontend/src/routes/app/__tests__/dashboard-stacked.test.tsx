// Multi-grant dashboard refresh tests (Slice 2 T22, AC-8.2).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import DashboardPage from '../dashboard';
import { renderWithProviders } from '../../../testing/render';
import { useAuthStore } from '../../../store/auth';
import { useLocaleStore } from '../../../store/locale';
import type { GrantDto } from '../../../api/grants';
import type {
  StackedDashboardResponse,
  WireEmployerStack,
} from '../../../api/dashboard';

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
    doubleTrigger: false,
    liquidityEventDate: null,
    doubleTriggerSatisfiedBy: null,
    employerName: 'ACME Inc.',
    ticker: null,
    notes: null,
    createdAt: '2024-09-15T00:00:00Z',
    updatedAt: '2024-09-15T00:00:00Z',
    ...overrides,
  };
}

function stackFor(employer: string, grantIds: string[]): WireEmployerStack {
  return {
    employerName: employer,
    employerKey: employer.toLowerCase().trim(),
    grantIds,
    points: [
      {
        date: '2025-01-01',
        cumulativeSharesVested: 0,
        cumulativeTimeVestedAwaitingLiquidity: 0,
        perGrantBreakdown: [],
      },
      {
        date: '2026-01-01',
        cumulativeSharesVested: 300_000_000,
        cumulativeTimeVestedAwaitingLiquidity: 0,
        perGrantBreakdown: grantIds.map((gid) => ({
          grantId: gid,
          instrument: 'rsu',
          sharesVestedThisEvent: 100_000_000,
          cumulativeForThisGrant: 100_000_000,
          state: 'vested' as const,
        })),
      },
    ],
  };
}

function mockFetch(
  grants: GrantDto[],
  stacked: StackedDashboardResponse,
): ReturnType<typeof vi.fn> {
  const spy = vi.fn(async (url: string) => {
    if (url.endsWith('/api/v1/grants')) {
      return new Response(JSON.stringify({ grants }), { status: 200 });
    }
    if (url.endsWith('/api/v1/dashboard/stacked')) {
      return new Response(JSON.stringify(stacked), { status: 200 });
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return spy;
}

afterEach(() => {
  vi.unstubAllGlobals();
  useAuthStore.getState().clear();
});

describe('Multi-grant dashboard refresh', () => {
  it('degenerates to a single-row per-employer panel when only one grant exists (AC-8.2.7)', async () => {
    seedAuthed();
    const g = grantFixture();
    mockFetch([g], {
      byEmployer: [stackFor('ACME Inc.', [g.id])],
      combined: stackFor('ACME Inc.', [g.id]).points,
    });
    renderWithProviders(<DashboardPage />, { initialEntries: ['/app/dashboard'] });

    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: /cartera por empleador/i }),
      ).toBeInTheDocument();
    });
    // Exactly one portfolio row rendered.
    const rows = document.querySelectorAll('.portfolio-row');
    expect(rows.length).toBe(1);
    // The single-grant tile grid is still rendered below (ACME appears
    // both in the portfolio row and in its own tile).
    const acmeMatches = Array.from(
      document.querySelectorAll('*'),
    ).filter((n) => n.textContent?.trim() === 'ACME Inc.');
    expect(acmeMatches.length).toBeGreaterThanOrEqual(2);
  });

  it('renders two employer rows when two disjoint employers exist', async () => {
    seedAuthed();
    const a = grantFixture({ id: 'g-a', employerName: 'Alpha' });
    const b = grantFixture({
      id: 'g-b',
      employerName: 'Bravo',
      instrument: 'nso',
      strikeAmount: '8.00',
      strikeCurrency: 'USD',
    });
    mockFetch([a, b], {
      byEmployer: [stackFor('Alpha', ['g-a']), stackFor('Bravo', ['g-b'])],
      combined: stackFor('combined', ['g-a', 'g-b']).points,
    });
    renderWithProviders(<DashboardPage />, { initialEntries: ['/app/dashboard'] });

    await waitFor(() => {
      expect(document.querySelectorAll('.portfolio-row').length).toBe(2);
    });
    const rows = document.querySelectorAll('.portfolio-row__name');
    const names = Array.from(rows).map((n) => n.textContent?.trim());
    expect(names).toEqual(['Alpha', 'Bravo']);
  });

  it('drills down into per-instrument groups when a single employer has mixed instruments', async () => {
    seedAuthed();
    const rsu = grantFixture({
      id: 'g-rsu',
      employerName: 'Acme',
      instrument: 'rsu',
    });
    const nso = grantFixture({
      id: 'g-nso',
      employerName: 'Acme',
      instrument: 'nso',
      strikeAmount: '8.00',
      strikeCurrency: 'USD',
    });
    const mixed = stackFor('Acme', ['g-rsu', 'g-nso']);
    // Mark one breakdown row as nso for the legend split.
    mixed.points[1]!.perGrantBreakdown[1] = {
      grantId: 'g-nso',
      instrument: 'nso',
      sharesVestedThisEvent: 100_000_000,
      cumulativeForThisGrant: 100_000_000,
      state: 'vested',
    };
    mockFetch([rsu, nso], {
      byEmployer: [mixed],
      combined: mixed.points,
    });
    renderWithProviders(<DashboardPage />, { initialEntries: ['/app/dashboard'] });

    await waitFor(() => {
      expect(document.querySelectorAll('.portfolio-row').length).toBe(1);
    });
    // Toggle shows expanded (auto-expanded when grant count > 1).
    const toggle = document.querySelector('.portfolio-row__toggle') as HTMLButtonElement;
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    // Instrument group labels should both render once the panel is expanded.
    const subPanel = document.querySelector('.portfolio-sub');
    expect(subPanel?.textContent?.toUpperCase()).toContain('RSU');
    expect(subPanel?.textContent?.toUpperCase()).toContain('NSO');
    // Collapse via the toggle.
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
  });
});
