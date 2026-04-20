// Modelo 720 section tests (Slice 2 T22, AC-6.*).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { Modelo720Section } from '../Modelo720Section';
import { renderWithProviders } from '../../../testing/render';
import type { Modelo720InputDto, UpsertOutcome } from '../../../api/modelo720';

function historyFixture(
  category: 'bank_accounts' | 'real_estate',
  rows: Modelo720InputDto[],
): Response {
  return new Response(JSON.stringify({ history: rows }), { status: 200 });
}

function upsertResponse(outcome: UpsertOutcome): Response {
  const dto: Modelo720InputDto = {
    id: 'm-1',
    category: 'bank_accounts',
    amountEur: '25000.00',
    referenceDate: '2026-12-31',
    fromDate: '2026-12-31',
    toDate: null,
    createdAt: '2026-12-31T00:00:00Z',
  };
  const body: Record<string, unknown> = { current: dto, outcome };
  if (outcome === 'no_op') body.unchanged = true;
  return new Response(JSON.stringify(body), {
    status: outcome === 'inserted' || outcome === 'closed_and_created' ? 201 : 200,
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('Modelo720Section', () => {
  it('renders the three category rows with securities stubbed as próx.', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url.includes('category=bank_accounts')) {
          return historyFixture('bank_accounts', []);
        }
        if (url.includes('category=real_estate')) {
          return historyFixture('real_estate', []);
        }
        return new Response('{}', { status: 200 });
      }),
    );
    renderWithProviders(<Modelo720Section />);
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: /modelo 720/i }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByText(/valores \/ participaciones en el extranjero/i),
    ).toBeInTheDocument();
    // Securities stub shows "próx." badge.
    expect(
      screen.getByText(/se calculará cuando actives seguimiento fiscal/i),
    ).toBeInTheDocument();
  });

  it('shows the "guardado" toast on inserted outcome', async () => {
    const spy = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes('/modelo-720-inputs') && init?.method === 'POST') {
        return upsertResponse('inserted');
      }
      if (url.includes('category=')) {
        return historyFixture('bank_accounts', []);
      }
      return new Response('{}', { status: 200 });
    });
    vi.stubGlobal('fetch', spy);
    renderWithProviders(<Modelo720Section />);
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: /modelo 720/i }),
      ).toBeInTheDocument();
    });
    fireEvent.change(
      document.querySelector('input[inputmode="decimal"]') as HTMLInputElement,
      { target: { value: '25000.00' } },
    );
    fireEvent.click(screen.getByRole('button', { name: /guardar modelo 720/i }));
    await waitFor(() => {
      const toast = screen.getByTestId('m720-toast');
      expect(toast.textContent?.toLowerCase()).toContain('guardado');
    });
  });

  it('shows the "sin cambios" toast on no-op outcome', async () => {
    const spy = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.includes('/modelo-720-inputs') && init?.method === 'POST') {
        return upsertResponse('no_op');
      }
      if (url.includes('category=')) {
        return historyFixture('bank_accounts', []);
      }
      return new Response('{}', { status: 200 });
    });
    vi.stubGlobal('fetch', spy);
    renderWithProviders(<Modelo720Section />);
    await waitFor(() => {
      expect(
        screen.getByRole('heading', { name: /modelo 720/i }),
      ).toBeInTheDocument();
    });
    fireEvent.change(
      document.querySelector('input[inputmode="decimal"]') as HTMLInputElement,
      { target: { value: '25000.00' } },
    );
    fireEvent.click(screen.getByRole('button', { name: /guardar modelo 720/i }));
    await waitFor(() => {
      const toast = screen.getByTestId('m720-toast');
      expect(toast.textContent?.toLowerCase()).toContain('sin cambios');
    });
  });

  it('renders the prior-periods history table', async () => {
    const rows: Modelo720InputDto[] = [
      {
        id: 'm-prev',
        category: 'bank_accounts',
        amountEur: '10000.00',
        referenceDate: '2024-12-31',
        fromDate: '2024-12-31',
        toDate: '2025-12-30',
        createdAt: '2024-12-31T00:00:00Z',
      },
      {
        id: 'm-cur',
        category: 'bank_accounts',
        amountEur: '25000.00',
        referenceDate: '2025-12-31',
        fromDate: '2025-12-31',
        toDate: null,
        createdAt: '2025-12-31T00:00:00Z',
      },
    ];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url.includes('category=bank_accounts')) {
          return historyFixture('bank_accounts', rows);
        }
        if (url.includes('category=real_estate')) {
          return historyFixture('real_estate', []);
        }
        return new Response('{}', { status: 200 });
      }),
    );
    renderWithProviders(<Modelo720Section />);
    await waitFor(() => {
      expect(screen.getAllByTestId('m720-history-row').length).toBe(2);
    });
    // The currently-open row carries the `.pill--full` badge.
    const pills = document.querySelectorAll('.pill--full');
    expect(pills.length).toBeGreaterThanOrEqual(1);
  });
});
