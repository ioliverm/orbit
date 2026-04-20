// DisclaimerModal (G-8..G-10).

import { describe, expect, it, vi, afterEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { DisclaimerModal } from '../Disclaimer/DisclaimerModal';
import { renderWithProviders } from '../../testing/render';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('DisclaimerModal', () => {
  it('disables the submit button until the user checks the acknowledgement', () => {
    renderWithProviders(<DisclaimerModal />);
    const submit = screen.getByRole('button', { name: /aceptar y continuar/i });
    expect(submit).toBeDisabled();
  });

  it('POSTs to /api/v1/consent/disclaimer on accept', async () => {
    // JSDOM Response disallows bodies on 204; use 200 {} for the spy.
    const fetchSpy = vi.fn(async () => new Response('{}', { status: 200 }));
    vi.stubGlobal('fetch', fetchSpy);

    renderWithProviders(<DisclaimerModal />);
    fireEvent.click(screen.getByTestId('disclaimer-accept'));
    fireEvent.click(screen.getByRole('button', { name: /aceptar y continuar/i }));

    await waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(1));
    const [url, init] = fetchSpy.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/api/v1/consent/disclaimer');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({ version: 'v1-2026-04' });
  });

  it('keeps the modal open and surfaces an error on failure', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: { code: 'server_internal', message: 'boom' } }), {
            status: 500,
          }),
      ),
    );

    renderWithProviders(<DisclaimerModal />);
    fireEvent.click(screen.getByTestId('disclaimer-accept'));
    fireEvent.click(screen.getByRole('button', { name: /aceptar y continuar/i }));

    await waitFor(() => {
      expect(screen.getByText(/no se pudo guardar/i)).toBeInTheDocument();
    });
    expect(screen.getByTestId('disclaimer-backdrop')).toBeInTheDocument();
  });

  it('is announced as a dialog with aria-modal', () => {
    renderWithProviders(<DisclaimerModal />);
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAttribute('aria-labelledby');
  });
});
