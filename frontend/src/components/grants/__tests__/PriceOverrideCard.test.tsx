// Per-grant current-price override card tests (Slice 3 T30).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { renderWithProviders } from '../../../testing/render';
import { PriceOverrideCard } from '../PriceOverrideCard';

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

describe('PriceOverrideCard', () => {
  it('shows "Sin override" pill when no override exists', async () => {
    mockFetch({
      '/current-price-override': () =>
        new Response(JSON.stringify({ override: null }), { status: 200 }),
    });
    renderWithProviders(<PriceOverrideCard grantId="g-1" defaultCurrency="USD" />);
    await waitFor(() => {
      expect(screen.getByTestId('price-override-empty')).toBeInTheDocument();
    });
  });

  it('shows existing override value', async () => {
    mockFetch({
      '/current-price-override': () =>
        new Response(
          JSON.stringify({
            override: { price: '55.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' },
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(<PriceOverrideCard grantId="g-1" defaultCurrency="USD" />);
    const val = await screen.findByTestId('price-override-value');
    expect(val.textContent).toMatch(/55\.00 USD/);
  });

  it('saves a new override via PUT', async () => {
    const fetchSpy = mockFetch({
      '/current-price-override': (init) => {
        if (init.method === 'PUT') {
          return new Response(
            JSON.stringify({
              override: { price: '45.00', currency: 'USD', enteredAt: '2026-04-19T00:00:00Z' },
            }),
            { status: 200 },
          );
        }
        return new Response(JSON.stringify({ override: null }), { status: 200 });
      },
    });
    renderWithProviders(<PriceOverrideCard grantId="g-1" defaultCurrency="USD" />);
    await screen.findByTestId('price-override-empty');
    fireEvent.click(screen.getByTestId('price-override-edit'));
    fireEvent.change(screen.getByTestId('price-override-input') as HTMLInputElement, {
      target: { value: '45.00' },
    });
    fireEvent.click(screen.getByTestId('price-override-save'));
    await waitFor(() => {
      expect(
        fetchSpy.mock.calls.some(
          (c) =>
            String(c[0]).includes('/current-price-override') &&
            (c[1] as RequestInit)?.method === 'PUT',
        ),
      ).toBe(true);
    });
  });
});
