// Session list tests (Slice 2 T22, AC-7.1..AC-7.2).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import SessionsPage from '../sessions';
import { renderWithProviders } from '../../../testing/render';
import type { SessionRowDto } from '../../../api/sessions';

function fixture(rows: SessionRowDto[]): Response {
  return new Response(JSON.stringify({ sessions: rows }), { status: 200 });
}

function mockFetch(
  initialRows: SessionRowDto[],
  afterDelete?: SessionRowDto[],
): ReturnType<typeof vi.fn> {
  let listCalls = 0;
  const spy = vi.fn(async (url: string, init?: RequestInit) => {
    if (url.endsWith('/api/v1/auth/sessions') && (!init || init.method === 'GET' || init.method === undefined)) {
      listCalls += 1;
      const rows = afterDelete && listCalls > 1 ? afterDelete : initialRows;
      return fixture(rows);
    }
    if (url.includes('/api/v1/auth/sessions/') && init?.method === 'DELETE') {
      return new Response(null, { status: 204 });
    }
    if (url.endsWith('/revoke-all-others') && init?.method === 'POST') {
      return new Response(JSON.stringify({ revokedCount: 1 }), { status: 200 });
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return spy;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

const currentSession: SessionRowDto = {
  id: 's-current',
  userAgent: 'Firefox 128 · macOS',
  countryIso2: 'ES',
  createdAt: '2026-04-18T09:12:00Z',
  lastUsedAt: '2026-04-19T20:00:00Z',
  isCurrent: true,
};
const otherSession: SessionRowDto = {
  id: 's-other',
  userAgent: 'Safari iOS 17 · iPhone',
  countryIso2: 'ES',
  createdAt: '2026-04-17T22:04:00Z',
  lastUsedAt: '2026-04-19T09:00:00Z',
  isCurrent: false,
};

describe('SessionsPage', () => {
  it('renders a row per session with the "actual" marker on the current row', async () => {
    mockFetch([currentSession, otherSession]);
    renderWithProviders(<SessionsPage />);
    await waitFor(() => {
      expect(screen.getAllByTestId('session-row').length).toBe(2);
    });
    // The current-row button carries the exact "Esta sesión" label and is
    // disabled per AC-7.2.5. We scope to the current row to avoid matching
    // the sibling "Cerrar esta sesión" button on the other row.
    const rows = screen.getAllByTestId('session-row');
    const currentRow = rows.find((r) =>
      r.classList.contains('session-row--current'),
    )!;
    const currentBtn = currentRow.querySelector('button');
    expect(currentBtn).not.toBeNull();
    expect(currentBtn).toBeDisabled();
  });

  it('revokes a non-current session when its row CTA is clicked', async () => {
    const spy = mockFetch([currentSession, otherSession], [currentSession]);
    renderWithProviders(<SessionsPage />);
    await waitFor(() => {
      expect(screen.getAllByTestId('session-row').length).toBe(2);
    });
    fireEvent.click(screen.getByRole('button', { name: /cerrar esta sesión/i }));
    await waitFor(() => {
      expect(screen.getAllByTestId('session-row').length).toBe(1);
    });
    const calls = spy.mock.calls.map((c) => [c[0], (c[1] as RequestInit | undefined)?.method]);
    expect(
      calls.some(
        ([url, method]) =>
          typeof url === 'string' &&
          url.includes('/api/v1/auth/sessions/s-other') &&
          method === 'DELETE',
      ),
    ).toBe(true);
  });

  it('requires two-step confirm for "Cerrar las demás sesiones"', async () => {
    mockFetch([currentSession, otherSession]);
    renderWithProviders(<SessionsPage />);
    await waitFor(() => {
      expect(screen.getAllByTestId('session-row').length).toBe(2);
    });
    const bulkBtn = screen.getByRole('button', {
      name: /cerrar las demás sesiones/i,
    });
    fireEvent.click(bulkBtn);
    await waitFor(() => {
      expect(screen.getByTestId('sessions-bulk-confirm')).toBeInTheDocument();
    });
    // Step 1 → Continuar.
    fireEvent.click(screen.getByRole('button', { name: /continuar/i }));
    await waitFor(() => {
      expect(
        screen.getByText(/¿confirmas cerrar todas las demás sesiones/i),
      ).toBeInTheDocument();
    });
  });

  it('disables the bulk CTA when the user has no other sessions', async () => {
    mockFetch([currentSession]);
    renderWithProviders(<SessionsPage />);
    await waitFor(() => {
      expect(screen.getAllByTestId('session-row').length).toBe(1);
    });
    const bulkBtn = screen.getByRole('button', {
      name: /cerrar las demás sesiones/i,
    });
    expect(bulkBtn).toBeDisabled();
  });
});
