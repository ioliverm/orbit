// Grant detail tests (AC-6.1.*, AC-6.2.*).
//
// Focus:
//   - Summary + timeline render from a fetched grant.
//   - Double-trigger + no liquidity event shows the awaiting-liquidity alert +
//     "Ingresos imponibles hasta la fecha: 0 acciones" summary line (AC-6.1.4).
//   - Edit button swaps to the shared form, pre-populated with saved fields.
//   - Delete two-step confirm flow reaches the final DELETE call (AC-6.2.4).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { Route, Routes } from 'react-router-dom';
import GrantDetailPage from '../detail';
import { renderWithProviders } from '../../../../testing/render';
import { useAuthStore } from '../../../../store/auth';
import { useLocaleStore } from '../../../../store/locale';
import type { GrantDto } from '../../../../api/grants';

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

function mockServer(opts: {
  grant: GrantDto | null;
  notFound?: boolean;
}): { spy: ReturnType<typeof vi.fn>; calls: Array<{ url: string; init: RequestInit }> } {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  const spy = vi.fn(async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    if (opts.notFound) {
      return new Response(
        JSON.stringify({ error: { code: 'not_found', message: 'not found' } }),
        { status: 404 },
      );
    }
    if ((init?.method ?? 'GET') === 'GET' && url.includes('/grants/')) {
      return new Response(JSON.stringify({ grant: opts.grant }), { status: 200 });
    }
    if (init?.method === 'PUT' && url.includes('/grants/')) {
      return new Response(
        JSON.stringify({ grant: opts.grant, vestingEvents: [] }),
        { status: 200 },
      );
    }
    if (init?.method === 'DELETE' && url.includes('/grants/')) {
      return new Response(null, { status: 204 });
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return { spy, calls };
}

function renderAt(path: string): ReturnType<typeof renderWithProviders> {
  return renderWithProviders(
    <Routes>
      <Route path="/app/grants/:grantId" element={<GrantDetailPage />} />
      <Route path="/app/dashboard" element={<div>DASHBOARD</div>} />
    </Routes>,
    { initialEntries: [path] },
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  useAuthStore.getState().clear();
});

describe('GrantDetailPage', () => {
  it('renders summary + timeline for a loaded grant', async () => {
    seedAuthed();
    mockServer({ grant: grantFixture() });
    renderAt('/app/grants/g-1');

    await waitFor(() => {
      expect(screen.getByRole('heading', { level: 1, name: /acme inc\./i })).toBeInTheDocument();
    });
    // Summary tile: "Acciones totales" with ES number.
    expect(screen.getByText(/acciones totales/i)).toBeInTheDocument();
    expect(screen.getAllByText(/30\.000/).length).toBeGreaterThan(0);
    // Timeline card present.
    expect(screen.getByTestId('timeline-card')).toBeInTheDocument();
  });

  it('surfaces the double-trigger awaiting-liquidity alert + "0 acciones" line (AC-6.1.4)', async () => {
    seedAuthed();
    mockServer({
      grant: grantFixture({ doubleTrigger: true, liquidityEventDate: null }),
    });
    renderAt('/app/grants/g-1');

    await waitFor(() => {
      expect(screen.getByText(/rsu double-trigger/i)).toBeInTheDocument();
    });
    expect(
      screen.getByText(/ingresos imponibles hasta la fecha: 0 acciones/i),
    ).toBeInTheDocument();
  });

  it('renders a 404 when the grant is not found (AC-7.3)', async () => {
    seedAuthed();
    mockServer({ grant: null, notFound: true });
    renderAt('/app/grants/missing');

    await waitFor(() => {
      expect(screen.getByText(/grant no encontrado/i)).toBeInTheDocument();
    });
  });

  it('Edit swaps to the form with saved values pre-populated (AC-6.2.1)', async () => {
    seedAuthed();
    mockServer({ grant: grantFixture({ employerName: 'ACME Inc.' }) });
    renderAt('/app/grants/g-1');

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /editar/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: /editar/i }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { level: 1, name: /editar grant/i })).toBeInTheDocument();
    });
    // Employer pre-populated.
    const employer = screen.getByLabelText(/empleador/i) as HTMLInputElement;
    expect(employer.value).toBe('ACME Inc.');
  });

  it('Delete requires a two-step confirm and fires a DELETE (AC-6.2.4)', async () => {
    seedAuthed();
    const { calls } = mockServer({ grant: grantFixture() });
    renderAt('/app/grants/g-1');

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /eliminar/i })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: /^eliminar$/i }));
    // Step 1 modal.
    expect(screen.getByTestId('delete-confirm')).toBeInTheDocument();
    expect(screen.getByTestId('delete-step1')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('delete-step1'));
    // Step 2 confirmation.
    expect(screen.getByTestId('delete-step2')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('delete-step2'));

    await waitFor(() => {
      const del = calls.find((c) => c.init?.method === 'DELETE' && c.url.includes('/grants/g-1'));
      expect(del).toBeDefined();
    });
  });
});
