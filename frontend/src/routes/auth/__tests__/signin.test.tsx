import { describe, expect, it, vi, afterEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import SigninPage from '../signin';
import { renderWithProviders } from '../../../testing/render';

afterEach(() => {
  vi.unstubAllGlobals();
});

function typeInto(el: HTMLElement, value: string): void {
  fireEvent.change(el, { target: { value } });
}

function mockFetch(status: number, body: unknown, headers?: Record<string, string>): ReturnType<typeof vi.fn> {
  const spy = vi.fn(
    async () =>
      new Response(typeof body === 'string' ? body : JSON.stringify(body), {
        status,
        headers: new Headers(headers ?? {}),
      }),
  );
  vi.stubGlobal('fetch', spy);
  return spy;
}

describe('SigninPage', () => {
  it('renders the initial state with email + password fields', () => {
    renderWithProviders(<SigninPage />);
    expect(screen.getByRole('heading', { level: 1, name: /inicia sesión/i })).toBeInTheDocument();
    expect(screen.getByLabelText(/correo electrónico/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/contraseña/i)).toBeInTheDocument();
  });

  it('shows the generic SEC-003/004 error on 401 auth', async () => {
    mockFetch(401, { error: { code: 'auth', message: 'bad creds' } });

    renderWithProviders(<SigninPage />);
    typeInto(screen.getByLabelText(/correo electrónico/i), 'maria.o@example.com');
    typeInto(screen.getByLabelText(/contraseña/i), 'whatever');
    fireEvent.click(screen.getByRole('button', { name: /^iniciar sesión$/i }));

    await waitFor(() => {
      expect(
        screen.getByText(/credenciales inválidas\. comprueba tu correo y contraseña/i),
      ).toBeInTheDocument();
    });
  });

  it('switches to captcha state when the server returns captcha_required', async () => {
    mockFetch(401, { error: { code: 'captcha_required', message: 'captcha' } });

    renderWithProviders(<SigninPage />);
    typeInto(screen.getByLabelText(/correo electrónico/i), 'maria.o@example.com');
    typeInto(screen.getByLabelText(/contraseña/i), 'whatever1');
    fireEvent.click(screen.getByRole('button', { name: /^iniciar sesión$/i }));

    await waitFor(() => {
      expect(screen.getByText(/comprobación adicional/i)).toBeInTheDocument();
    });
  });

  it('switches to rate-limited view on 429 and honours Retry-After', async () => {
    mockFetch(429, { error: { code: 'rate_limited', message: 'slow' } }, { 'Retry-After': '480' });

    renderWithProviders(<SigninPage />);
    typeInto(screen.getByLabelText(/correo electrónico/i), 'maria.o@example.com');
    typeInto(screen.getByLabelText(/contraseña/i), 'whatever1');
    fireEvent.click(screen.getByRole('button', { name: /^iniciar sesión$/i }));

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: /demasiados intentos/i })).toBeInTheDocument();
    });
    expect(screen.getByText(/~8 min/i)).toBeInTheDocument();
  });
});
