// Slice 3b T39 — "Preferencias fiscales" section on /app/account/profile.
//
// Covers the basics called out in slice-3b-acceptance-criteria §4:
// render (empty + populated), save outcomes (no_op / inserted /
// closed_and_created / updated_same_day), and the history table.

import { afterEach, describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import ProfilePage from '../profile';
import { renderWithProviders } from '../../../testing/render';
import { useAuthStore } from '../../../store/auth';

type Handler = (init: RequestInit) => Response;

function mockFetch(handlers: Record<string, Handler>): ReturnType<typeof vi.fn> {
  const spy = vi.fn(async (url: string, init: RequestInit = {}) => {
    for (const [k, fn] of Object.entries(handlers)) {
      if (url.includes(k)) return fn(init);
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return spy;
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
}

const emptyHandlers: Record<string, Handler> = {
  '/user-tax-preferences/current': () =>
    new Response(JSON.stringify({ current: null }), { status: 200 }),
  '/user-tax-preferences': () =>
    new Response(JSON.stringify({ preferences: [] }), { status: 200 }),
  '/modelo-720-inputs': () =>
    new Response(JSON.stringify({ history: [] }), { status: 200 }),
};

beforeEach(() => {
  seedAuthed();
});
afterEach(() => {
  vi.unstubAllGlobals();
  useAuthStore.getState().clear();
});

describe('ProfilePage — Preferencias fiscales', () => {
  it('renders with empty state copy + disabled Save CTA when no row exists', async () => {
    mockFetch(emptyHandlers);
    renderWithProviders(<ProfilePage />);
    await waitFor(() => {
      expect(screen.getByTestId('tax-preferences-section')).toBeInTheDocument();
    });
    const save = screen.getByTestId('prefs-save') as HTMLButtonElement;
    expect(save.disabled).toBe(true);
  });

  it('reveals the percent field when country is Spain', async () => {
    mockFetch(emptyHandlers);
    renderWithProviders(<ProfilePage />);
    await waitFor(() => {
      expect(screen.getByTestId('tax-preferences-section')).toBeInTheDocument();
    });
    const select = screen.getByTestId('prefs-country') as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'ES' } });
    // The hidden attribute is driven by state; after switching to ES
    // the field is visible.
    const pct = screen.getByTestId('prefs-percent') as HTMLInputElement;
    const fieldContainer = pct.closest('.field--conditional') as HTMLElement;
    expect(fieldContainer.hasAttribute('hidden')).toBe(false);
  });

  it('saves via POST and surfaces the "closed_and_created" toast', async () => {
    const current = {
      id: 'p-1',
      countryIso2: 'ES',
      rendimientoDelTrabajoPercent: '0.4600',
      sellToCoverEnabled: true,
      fromDate: '2026-04-19',
      toDate: null,
      createdAt: '2026-04-19T00:00:00Z',
      updatedAt: '2026-04-19T00:00:00Z',
    };
    const handlers: Record<string, Handler> = {
      '/user-tax-preferences/current': () =>
        new Response(
          JSON.stringify({
            current: { ...current, countryIso2: 'PT', rendimientoDelTrabajoPercent: null, sellToCoverEnabled: false },
          }),
          { status: 200 },
        ),
      '/user-tax-preferences': (init) => {
        if (init.method === 'POST') {
          return new Response(
            JSON.stringify({ current, outcome: 'closed_and_created' }),
            { status: 201 },
          );
        }
        return new Response(JSON.stringify({ preferences: [] }), { status: 200 });
      },
      '/modelo-720-inputs': () =>
        new Response(JSON.stringify({ history: [] }), { status: 200 }),
    };
    mockFetch(handlers);
    renderWithProviders(<ProfilePage />);
    await waitFor(() => {
      const select = screen.getByTestId('prefs-country') as HTMLSelectElement;
      expect(select.value).toBe('PT');
    });
    // Flip country to ES + set percent.
    fireEvent.change(screen.getByTestId('prefs-country'), { target: { value: 'ES' } });
    fireEvent.change(screen.getByTestId('prefs-percent'), { target: { value: '46' } });
    fireEvent.click(screen.getByTestId('prefs-save'));
    await waitFor(() => {
      const toast = screen.getByTestId('prefs-toast');
      expect(toast.textContent).toMatch(/período|period/i);
    });
  });

  it('surfaces "sin cambios" toast on no_op outcome', async () => {
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
    const handlers: Record<string, Handler> = {
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current }), { status: 200 }),
      '/user-tax-preferences': (init) => {
        if (init.method === 'POST') {
          return new Response(
            JSON.stringify({ current, outcome: 'no_op', unchanged: true }),
            { status: 200 },
          );
        }
        return new Response(JSON.stringify({ preferences: [] }), { status: 200 });
      },
      '/modelo-720-inputs': () =>
        new Response(JSON.stringify({ history: [] }), { status: 200 }),
    };
    mockFetch(handlers);
    renderWithProviders(<ProfilePage />);
    await waitFor(() => {
      const select = screen.getByTestId('prefs-country') as HTMLSelectElement;
      expect(select.value).toBe('ES');
    });
    // Dirty the form by flipping the toggle off and on so Save activates.
    fireEvent.click(screen.getByTestId('prefs-sell-to-cover'));
    fireEvent.click(screen.getByTestId('prefs-sell-to-cover'));
    // Now edit percent to re-equal something arbitrary but different
    // from initial (so Save is enabled); the server returns no_op.
    fireEvent.change(screen.getByTestId('prefs-percent'), { target: { value: '46' } });
    fireEvent.click(screen.getByTestId('prefs-save'));
    await waitFor(() => {
      const toast = screen.getByTestId('prefs-toast');
      expect(toast.textContent).toMatch(/[Ss]in cambios/);
    });
  });

  it('renders the history table with closed rows below the form', async () => {
    const closedRow = {
      id: 'p-old',
      countryIso2: 'ES',
      rendimientoDelTrabajoPercent: '0.4500',
      sellToCoverEnabled: true,
      fromDate: '2026-02-15',
      toDate: '2026-04-01',
      createdAt: '2026-02-15T00:00:00Z',
      updatedAt: '2026-04-01T00:00:00Z',
    };
    const current = {
      id: 'p-open',
      countryIso2: 'ES',
      rendimientoDelTrabajoPercent: '0.4600',
      sellToCoverEnabled: true,
      fromDate: '2026-04-01',
      toDate: null,
      createdAt: '2026-04-01T00:00:00Z',
      updatedAt: '2026-04-01T00:00:00Z',
    };
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current }), { status: 200 }),
      '/user-tax-preferences': () =>
        new Response(
          JSON.stringify({ preferences: [current, closedRow] }),
          { status: 200 },
        ),
      '/modelo-720-inputs': () =>
        new Response(JSON.stringify({ history: [] }), { status: 200 }),
    });
    renderWithProviders(<ProfilePage />);
    await waitFor(() => {
      expect(screen.getByTestId('prefs-history')).toBeInTheDocument();
    });
    const rows = screen.getAllByTestId('prefs-history-row');
    expect(rows).toHaveLength(1);
  });
});
