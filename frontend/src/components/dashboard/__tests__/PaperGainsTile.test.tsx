// Unit tests for the paper-gains dashboard tile (Slice 3 T30).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { renderWithProviders } from '../../../testing/render';
import { PaperGainsTile } from '../PaperGainsTile';
import type { GrantDto } from '../../../api/grants';

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
    doubleTrigger: false,
    liquidityEventDate: null,
    doubleTriggerSatisfiedBy: null,
    employerName: 'ACME Inc.',
    ticker: 'ACME',
    notes: null,
    createdAt: '2026-04-19T00:00:00Z',
    updatedAt: '2026-04-19T00:00:00Z',
    ...overrides,
  };
}

function mockFetch(
  handlers: Record<string, (init: RequestInit) => Response>,
): ReturnType<typeof vi.fn> {
  const spy = vi.fn(async (url: string, init: RequestInit = {}) => {
    for (const [k, fn] of Object.entries(handlers)) {
      if (url.includes(k)) return fn(init);
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return spy;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('PaperGainsTile', () => {
  it('renders envelope range when combined band is present', async () => {
    mockFetch({
      '/api/v1/current-prices': () =>
        new Response(
          JSON.stringify({
            prices: [
              { ticker: 'ACME', price: '50.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' },
            ],
          }),
          { status: 200 },
        ),
      '/dashboard/paper-gains': () =>
        new Response(
          JSON.stringify({
            perGrant: [{ grantId: 'g-1', complete: true }],
            combinedEurBand: { low: '100.00', mid: '110.00', high: '120.00' },
            incompleteGrants: [],
            stalenessFx: 'fresh',
            fxDate: '2026-04-19',
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(<PaperGainsTile grants={[grantFixture()]} />);
    await waitFor(() => {
      expect(screen.getByTestId('paper-gains-mid').textContent).toContain('110.00');
    });
  });

  it('renders partial-data banner with incomplete grants linked', async () => {
    mockFetch({
      '/api/v1/current-prices': () =>
        new Response(JSON.stringify({ prices: [] }), { status: 200 }),
      '/dashboard/paper-gains': () =>
        new Response(
          JSON.stringify({
            perGrant: [],
            combinedEurBand: null,
            incompleteGrants: [
              { grantId: 'g-1', employer: 'ACME Inc.', instrument: 'rsu' },
            ],
            stalenessFx: 'fresh',
            fxDate: '2026-04-19',
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(<PaperGainsTile grants={[grantFixture()]} />);
    await waitFor(() => {
      expect(screen.getByTestId('paper-gains-partial-banner')).toBeTruthy();
    });
    const link = screen.getByTestId('paper-gains-incomplete-link') as HTMLAnchorElement;
    expect(link.getAttribute('href')).toContain('/app/grants/g-1');
  });

  it('renders a ticker row with the current price and supports clear', async () => {
    const fetchSpy = mockFetch({
      '/api/v1/current-prices/ACME': (init) => {
        if (init.method === 'DELETE') return new Response('', { status: 204 });
        return new Response(
          JSON.stringify({ ticker: 'ACME', price: '50.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' }),
          { status: 200 },
        );
      },
      '/api/v1/current-prices': () =>
        new Response(
          JSON.stringify({
            prices: [
              { ticker: 'ACME', price: '50.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' },
            ],
          }),
          { status: 200 },
        ),
      '/dashboard/paper-gains': () =>
        new Response(
          JSON.stringify({
            perGrant: [],
            combinedEurBand: null,
            incompleteGrants: [],
            stalenessFx: 'fresh',
            fxDate: '2026-04-19',
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(<PaperGainsTile grants={[grantFixture()]} />);
    const row = await screen.findByTestId('ticker-row');
    expect(row.getAttribute('data-ticker')).toBe('ACME');
    // Wait until the existing price has hydrated the row (value populated).
    await waitFor(() => {
      const input = row.querySelector('input') as HTMLInputElement;
      expect(input.value).toBe('50.00');
    });
    const clear = screen.getByTestId('ticker-row-clear');
    fireEvent.click(clear);
    await waitFor(
      () => {
        const calls = fetchSpy.mock.calls.map(
          (c) => String(c[0]) + '|' + (c[1] as RequestInit | undefined)?.method,
        );
        expect(
          calls.some((c) => c.includes('/current-prices/ACME') && c.includes('DELETE')),
        ).toBe(true);
      },
      { timeout: 2000 },
    );
  });

  it('shows "+ Añadir ticker" empty-state copy when portfolio has no tickers', async () => {
    mockFetch({
      '/api/v1/current-prices': () =>
        new Response(JSON.stringify({ prices: [] }), { status: 200 }),
      '/dashboard/paper-gains': () =>
        new Response(
          JSON.stringify({
            perGrant: [],
            combinedEurBand: null,
            incompleteGrants: [],
            stalenessFx: 'fresh',
            fxDate: '2026-04-19',
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(<PaperGainsTile grants={[grantFixture({ ticker: null })]} />);
    await waitFor(() => {
      expect(screen.getByText(/Introduce el precio actual por grant/i)).toBeInTheDocument();
    });
  });
});
