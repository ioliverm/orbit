import { describe, expect, it, vi, afterEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import SignupPage from '../signup';
import { renderWithProviders } from '../../../testing/render';

afterEach(() => {
  vi.unstubAllGlobals();
});

function typeInto(el: HTMLElement, value: string): void {
  fireEvent.change(el, { target: { value } });
}

describe('SignupPage', () => {
  it('rejects an empty form inline without hitting the network', async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);

    renderWithProviders(<SignupPage />);
    fireEvent.click(screen.getByRole('button', { name: /continuar/i }));

    await waitFor(() => {
      expect(screen.getByText(/introduce un correo válido/i)).toBeInTheDocument();
    });
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('rejects a password shorter than 12 characters inline', async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);

    renderWithProviders(<SignupPage />);
    typeInto(screen.getByLabelText(/correo electrónico/i), 'maria.o@example.com');
    typeInto(screen.getByLabelText(/contraseña/i), 'short');
    fireEvent.click(screen.getByRole('button', { name: /continuar/i }));

    await waitFor(() => {
      expect(screen.getByText(/mínimo 12 caracteres/i)).toBeInTheDocument();
    });
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it('submits to /api/v1/auth/signup with the form values and navigates on success', async () => {
    const fetchSpy = vi.fn(async () => new Response('', { status: 201 }));
    vi.stubGlobal('fetch', fetchSpy);

    renderWithProviders(<SignupPage />);
    typeInto(screen.getByLabelText(/correo electrónico/i), 'maria.o@example.com');
    typeInto(screen.getByLabelText(/contraseña/i), 'correcthorse12!');
    fireEvent.click(screen.getByRole('button', { name: /continuar/i }));

    await waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(1));
    const [url, init] = fetchSpy.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/api/v1/auth/signup');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toMatchObject({
      email: 'maria.o@example.com',
      password: 'correcthorse12!',
    });
  });

  it('surfaces HIBP breach as a field-level error', async () => {
    const fetchSpy = vi.fn(
      async () =>
        new Response(
          JSON.stringify({
            error: {
              code: 'validation',
              message: 'validation',
              details: { fields: [{ field: 'password', code: 'breached' }] },
            },
          }),
          { status: 422 },
        ),
    );
    vi.stubGlobal('fetch', fetchSpy);

    renderWithProviders(<SignupPage />);
    typeInto(screen.getByLabelText(/correo electrónico/i), 'maria.o@example.com');
    typeInto(screen.getByLabelText(/contraseña/i), 'correcthorse12!');
    fireEvent.click(screen.getByRole('button', { name: /continuar/i }));

    await waitFor(() => {
      expect(screen.getByText(/listas públicas de filtraciones/i)).toBeInTheDocument();
    });
  });
});
