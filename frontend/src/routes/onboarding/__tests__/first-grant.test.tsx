// First-grant form tests (AC-4.2.*).
//
// Focus:
//   - Conditional fields react to instrument selection (AC-4.2.2).
//   - Live preview renders once vesting inputs are valid (AC-4.2.5).
//   - Cliff > vest validation blocks submit (AC-4.2.6).
//   - Zero shares blocked (AC-4.2.7).
//   - Strike required for NSO/ISO (AC-4.2.8).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import FirstGrantStepPage from '../first-grant';
import { renderWithProviders } from '../../../testing/render';
import { useAuthStore } from '../../../store/auth';
import { useLocaleStore } from '../../../store/locale';

function makeFetchSpy() {
  return vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(
    async () => new Response('{}', { status: 200 }),
  );
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
    onboardingStage: 'first_grant',
    disclaimerAccepted: true,
    loading: false,
    initialized: true,
  });
  useLocaleStore.setState({ locale: 'es-ES' });
}

afterEach(() => {
  vi.unstubAllGlobals();
  useAuthStore.getState().clear();
  vi.useRealTimers();
});

describe('FirstGrantStepPage', () => {
  it('does not show the strike field for RSU by default; shows it after selecting NSO', async () => {
    seedAuthed();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 200 })));
    renderWithProviders(<FirstGrantStepPage />);

    expect(screen.queryByLabelText(/^strike$/i)).toBeNull();

    // Select NSO (ISO's description also mentions "NSO", disambiguate by value).
    const nso = document.querySelector('input[type="radio"][value="nso"]') as HTMLElement;
    fireEvent.click(nso);

    await waitFor(() => {
      expect(screen.getByLabelText(/^strike$/i)).toBeInTheDocument();
    });
  });

  it('renders the ESPP estimated-discount field only when ESPP is selected', async () => {
    seedAuthed();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 200 })));
    renderWithProviders(<FirstGrantStepPage />);

    expect(screen.queryByLabelText(/descuento estimado/i)).toBeNull();
    fireEvent.click(
      document.querySelector('input[type="radio"][value="espp"]') as HTMLElement,
    );
    await waitFor(() => {
      expect(screen.getByLabelText(/descuento estimado/i)).toBeInTheDocument();
    });
  });

  it('renders the live preview after the user fills in the share count (AC-4.2.5)', async () => {
    seedAuthed();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 200 })));
    renderWithProviders(<FirstGrantStepPage />);

    // Type share count.
    fireEvent.change(screen.getByLabelText(/número de acciones/i), { target: { value: '30000' } });

    // The preview is debounced 300 ms; waitFor default (1000 ms) is enough.
    await waitFor(
      () => {
        expect(screen.getByText(/acciones totales/i)).toBeInTheDocument();
      },
      { timeout: 2000 },
    );
  });

  it('blocks submit when cliff > vesting_total_months on the custom template (AC-4.2.6)', async () => {
    seedAuthed();
    const spy = makeFetchSpy();
    vi.stubGlobal('fetch', spy);
    renderWithProviders(<FirstGrantStepPage />);

    fireEvent.change(screen.getByLabelText(/empleador/i), { target: { value: 'ACME' } });
    fireEvent.change(screen.getByLabelText(/número de acciones/i), { target: { value: '100' } });
    fireEvent.change(screen.getByLabelText(/calendario de vesting/i), {
      target: { value: 'custom' },
    });
    await waitFor(() => {
      expect(screen.getByLabelText(/meses totales/i)).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText(/meses totales/i), { target: { value: '12' } });
    fireEvent.change(screen.getByLabelText(/cliff \(meses\)/i), { target: { value: '24' } });

    fireEvent.click(screen.getByRole('button', { name: /guardar grant/i }));

    await waitFor(() => {
      expect(screen.getByText(/el cliff no puede superar el periodo total/i)).toBeInTheDocument();
    });
    const posts = spy.mock.calls.filter(
      (c) => (c[1] as RequestInit | undefined)?.method === 'POST',
    );
    expect(posts).toHaveLength(0);
  });

  it('blocks submit when share_count ≤ 0 (AC-4.2.7)', async () => {
    seedAuthed();
    const spy = makeFetchSpy();
    vi.stubGlobal('fetch', spy);
    renderWithProviders(<FirstGrantStepPage />);

    fireEvent.change(screen.getByLabelText(/empleador/i), { target: { value: 'ACME' } });
    fireEvent.change(screen.getByLabelText(/número de acciones/i), { target: { value: '0' } });
    fireEvent.click(screen.getByRole('button', { name: /guardar grant/i }));

    await waitFor(() => {
      expect(
        screen.getByText(/introduce un número de acciones mayor que 0/i),
      ).toBeInTheDocument();
    });
    const posts = spy.mock.calls.filter(
      (c) => (c[1] as RequestInit | undefined)?.method === 'POST',
    );
    expect(posts).toHaveLength(0);
  });

  it('blocks submit when strike is missing for NSO (AC-4.2.8)', async () => {
    seedAuthed();
    const spy = makeFetchSpy();
    vi.stubGlobal('fetch', spy);
    renderWithProviders(<FirstGrantStepPage />);

    // Switch to NSO.
    // The ISO radio's description mentions "NSO", so disambiguate by value.
    fireEvent.click(
      Array.from(document.querySelectorAll('input[type="radio"][value="nso"]'))[0] as HTMLElement,
    );
    await waitFor(() => {
      expect(screen.getByLabelText(/^strike$/i)).toBeInTheDocument();
    });
    fireEvent.change(screen.getByLabelText(/empleador/i), { target: { value: 'ACME' } });
    fireEvent.change(screen.getByLabelText(/número de acciones/i), { target: { value: '100' } });
    // Leave strike empty.
    fireEvent.click(screen.getByRole('button', { name: /guardar grant/i }));

    await waitFor(() => {
      expect(screen.getByText(/el strike es obligatorio/i)).toBeInTheDocument();
    });
    const posts = spy.mock.calls.filter(
      (c) => (c[1] as RequestInit | undefined)?.method === 'POST',
    );
    expect(posts).toHaveLength(0);
  });
});
