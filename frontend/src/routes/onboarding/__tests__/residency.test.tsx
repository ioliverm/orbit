// Residency step tests (AC-4.1.1..6).
//
// Focus:
//   - Autonomía dropdown renders the full list with foral suffix.
//   - Happy-path submit posts to /api/v1/residency with the correct body.
//   - Empty autonomía blocks submit inline (AC-4.1.6).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { QueryClient } from '@tanstack/react-query';
import ResidencyStepPage from '../residency';
import { renderWithProviders } from '../../../testing/render';
import { useAuthStore } from '../../../store/auth';
import { useLocaleStore } from '../../../store/locale';

interface MockCall {
  url: string;
  init: RequestInit;
}

function seedAuthed(): void {
  useAuthStore.setState({
    user: { id: 'u-1', email: 'maria@example.com', locale: 'es-ES', primaryCurrency: 'EUR' },
    residency: null,
    onboardingStage: 'residency',
    disclaimerAccepted: true,
    loading: false,
    initialized: true,
  });
  useLocaleStore.setState({ locale: 'es-ES' });
}

const autonomiasPayload = {
  autonomias: [
    { code: 'ES-MD', nameEs: 'Comunidad de Madrid', nameEn: 'Madrid', foral: false },
    { code: 'ES-CT', nameEs: 'Cataluña', nameEn: 'Catalonia', foral: false },
    { code: 'ES-NA', nameEs: 'Navarra', nameEn: 'Navarre', foral: true },
    { code: 'ES-PV', nameEs: 'País Vasco', nameEn: 'Basque Country', foral: true },
  ],
};

function mockAutonomiasOnly(): ReturnType<typeof vi.fn> {
  const spy = vi.fn(async (url: string) => {
    if (url.endsWith('/api/v1/residency/autonomias')) {
      return new Response(JSON.stringify(autonomiasPayload), { status: 200 });
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return spy;
}

function mockSubmit(): { spy: ReturnType<typeof vi.fn>; calls: MockCall[] } {
  const calls: MockCall[] = [];
  const spy = vi.fn(async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    if (url.endsWith('/api/v1/residency/autonomias')) {
      return new Response(JSON.stringify(autonomiasPayload), { status: 200 });
    }
    if (url.endsWith('/api/v1/residency')) {
      return new Response(
        JSON.stringify({
          residency: {
            id: 'r-1',
            jurisdiction: 'ES',
            subJurisdiction: 'ES-MD',
            fromDate: '2026-04-19',
            toDate: null,
            regimeFlags: [],
          },
          primaryCurrency: 'EUR',
        }),
        { status: 201 },
      );
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return { spy, calls };
}

afterEach(() => {
  vi.unstubAllGlobals();
  useAuthStore.getState().clear();
});

describe('ResidencyStepPage', () => {
  it('renders autonomía options including foral entries with the "no soportado en v1" suffix', async () => {
    seedAuthed();
    mockAutonomiasOnly();

    renderWithProviders(<ResidencyStepPage />);

    await waitFor(() => {
      expect(screen.getByRole('option', { name: /comunidad de madrid/i })).toBeInTheDocument();
    });
    // Foral entries must be present and suffixed.
    expect(
      screen.getByRole('option', { name: /navarra \(no soportado en v1\)/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('option', { name: /país vasco \(no soportado en v1\)/i }),
    ).toBeInTheDocument();
  });

  it('submits the POST body with jurisdiction=ES + subJurisdiction + primaryCurrency', async () => {
    seedAuthed();
    const { calls } = mockSubmit();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    renderWithProviders(<ResidencyStepPage />, { client });

    // Wait for autonomias to hydrate the dropdown.
    await waitFor(() =>
      expect(screen.getByRole('option', { name: /comunidad de madrid/i })).toBeInTheDocument(),
    );

    // Default selection is ES-MD. Submit as-is.
    fireEvent.click(screen.getByRole('button', { name: /guardar residencia y continuar/i }));

    await waitFor(() => {
      const submit = calls.find((c) => c.url.endsWith('/api/v1/residency') && c.init.method === 'POST');
      expect(submit).toBeDefined();
    });
    const submit = calls.find((c) => c.url.endsWith('/api/v1/residency') && c.init.method === 'POST')!;
    expect(JSON.parse(submit.init.body as string)).toMatchObject({
      jurisdiction: 'ES',
      subJurisdiction: 'ES-MD',
      primaryCurrency: 'EUR',
      regimeFlags: [],
    });
  });

  it('adds the foral_navarra flag when País Vasco or Navarra is selected', async () => {
    seedAuthed();
    const { calls } = mockSubmit();

    renderWithProviders(<ResidencyStepPage />);
    await waitFor(() =>
      expect(screen.getByRole('option', { name: /navarra \(no soportado/i })).toBeInTheDocument(),
    );

    const select = screen.getByLabelText(/autonomía/i) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: 'ES-NA' } });
    fireEvent.click(screen.getByRole('button', { name: /guardar residencia y continuar/i }));

    await waitFor(() => {
      const submit = calls.find(
        (c) => c.url.endsWith('/api/v1/residency') && c.init.method === 'POST',
      );
      expect(submit).toBeDefined();
    });
    const submit = calls.find((c) => c.url.endsWith('/api/v1/residency') && c.init.method === 'POST')!;
    const body = JSON.parse(submit.init.body as string);
    expect(body.subJurisdiction).toBe('ES-NA');
    expect(body.regimeFlags).toContain('foral_navarra');
  });

  it('blocks submit with inline error if the autonomía is cleared (AC-4.1.6)', async () => {
    seedAuthed();
    const spy = mockAutonomiasOnly();

    renderWithProviders(<ResidencyStepPage />);
    await waitFor(() =>
      expect(screen.getByRole('option', { name: /comunidad de madrid/i })).toBeInTheDocument(),
    );

    const select = screen.getByLabelText(/autonomía/i) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: '' } });
    fireEvent.click(screen.getByRole('button', { name: /guardar residencia y continuar/i }));

    await waitFor(() => {
      expect(screen.getByText(/selecciona una autonomía/i)).toBeInTheDocument();
    });
    // The POST must never have fired.
    const postCalls = spy.mock.calls.filter(
      (c) =>
        (c[1] as RequestInit | undefined)?.method === 'POST' &&
        (c[0] as string).endsWith('/api/v1/residency'),
    );
    expect(postCalls).toHaveLength(0);
  });
});
