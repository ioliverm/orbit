// Dashboard tests (AC-5.1..5.3).
//
// Focus:
//   - Empty state (zero grants) renders Tu cartera + prose + Añadir grant CTA (AC-5.1.1).
//   - Single-grant tile renders with all required cells (AC-5.2.1).
//   - Multi-grant tiles render; no Modelo 720 banner, no rule-set chip (AC-5.3).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import DashboardPage from '../dashboard';
import { renderWithProviders } from '../../../testing/render';
import { useAuthStore } from '../../../store/auth';
import { useLocaleStore } from '../../../store/locale';
import type { GrantDto } from '../../../api/grants';

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

function mockGrantsList(grants: GrantDto[]): ReturnType<typeof vi.fn> {
  const spy = vi.fn(async (url: string) => {
    if (url.endsWith('/api/v1/grants')) {
      return new Response(JSON.stringify({ grants }), { status: 200 });
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

describe('DashboardPage', () => {
  it('renders the empty state when the user has no grants (AC-5.1.1)', async () => {
    seedAuthed();
    mockGrantsList([]);
    renderWithProviders(<DashboardPage />);

    await waitFor(() => {
      expect(screen.getByText(/tu cartera está vacía/i)).toBeInTheDocument();
    });
    expect(screen.getByRole('heading', { level: 1, name: /tu cartera/i })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /añadir grant/i })).toHaveAttribute(
      'href',
      '/app/grants/new',
    );
  });

  it('does not render a Modelo 720 banner or rule-set chip in the empty state (AC-5.3)', async () => {
    seedAuthed();
    mockGrantsList([]);
    renderWithProviders(<DashboardPage />);

    await waitFor(() => {
      expect(screen.getByText(/tu cartera está vacía/i)).toBeInTheDocument();
    });
    expect(screen.queryByText(/modelo 720/i)).toBeNull();
    expect(screen.queryByText(/rule.?set/i)).toBeNull();
    expect(screen.queryByText(/valor papel/i)).toBeNull();
    expect(screen.queryByText(/impuesto estimado/i)).toBeNull();
  });

  it('renders a single grant tile with employer, share count, and a sparkline (AC-5.2.1)', async () => {
    seedAuthed();
    mockGrantsList([grantFixture()]);
    renderWithProviders(<DashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('ACME Inc.')).toBeInTheDocument();
    });
    // Share count formatted per ES locale (30.000).
    expect(screen.getByText(/30\.000/)).toBeInTheDocument();
    // Sparkline has role="img".
    const imgs = screen.getAllByRole('img');
    expect(imgs.length).toBeGreaterThan(0);
  });

  it('renders one tile per grant in multi-grant mode (AC-5.2.1)', async () => {
    seedAuthed();
    mockGrantsList([
      grantFixture({ id: 'g-1', employerName: 'ACME Inc.' }),
      grantFixture({
        id: 'g-2',
        employerName: 'Contoso Ltd.',
        instrument: 'nso',
        strikeAmount: '8.00',
        strikeCurrency: 'USD',
        doubleTrigger: false,
      }),
    ]);
    renderWithProviders(<DashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('ACME Inc.')).toBeInTheDocument();
      expect(screen.getByText('Contoso Ltd.')).toBeInTheDocument();
    });

    // Strike shows with explicit USD suffix (AC-5.2.2).
    expect(screen.getByText(/\$8\.00 USD/)).toBeInTheDocument();
  });

  it('does not render tax tiles, rule-set chips, or EUR conversions with grants (AC-5.3)', async () => {
    seedAuthed();
    mockGrantsList([grantFixture()]);
    renderWithProviders(<DashboardPage />);

    await waitFor(() => {
      expect(screen.getByText('ACME Inc.')).toBeInTheDocument();
    });
    expect(screen.queryByText(/modelo 720/i)).toBeNull();
    expect(screen.queryByText(/impuesto estimado/i)).toBeNull();
    expect(screen.queryByText(/€/)).toBeNull();
    expect(screen.queryByText(/valor papel/i)).toBeNull();
  });
});
