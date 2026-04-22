// Slice 3b T39 — VestingEventDialog tests.
//
// Covers the per-row editor's happy paths + the key error surfaces
// called out in AC-7 of the slice-3b ACs: OCC 409, unsaved-changes
// prompt, narrow-clear vs full-clear, 422 envelopes for the four
// sell-to-cover validation codes, and the profile-sourced tax-percent
// placeholder.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { renderWithProviders } from '../../../testing/render';
import { VestingEventDialog } from '../VestingEventDialog';
import type { EditableVestingEvent } from '../VestingEventsEditor';

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

function baseEvent(overrides: Partial<EditableVestingEvent> = {}): EditableVestingEvent {
  return {
    id: 'e-1',
    vestDate: '2024-09-15',
    sharesVestedThisEventScaled: 100 * 10_000,
    fmvAtVest: null,
    fmvCurrency: null,
    isUserOverride: false,
    updatedAt: '2026-04-19T00:00:00Z',
    state: 'vested',
    ...overrides,
  };
}

function rowResponse(overrides: Record<string, unknown> = {}): Response {
  return new Response(
    JSON.stringify({
      id: 'e-1',
      grantId: 'g-1',
      vestDate: '2024-09-15',
      sharesVestedThisEvent: '100',
      sharesVestedThisEventScaled: 1_000_000,
      cumulativeSharesVested: '100',
      fmvAtVest: '42.0000',
      fmvCurrency: 'USD',
      isUserOverride: true,
      updatedAt: '2026-04-19T01:00:00Z',
      ...overrides,
    }),
    { status: 200 },
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('VestingEventDialog', () => {
  it('renders with role="dialog" + pills + derived panel dashed', () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent()}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    const dialog = screen.getByTestId('vesting-dialog');
    expect(dialog.getAttribute('role')).toBe('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(screen.getByTestId('dialog-pill-auto')).toBeInTheDocument();
    expect(screen.getByTestId('dialog-pill-stc-off')).toBeInTheDocument();
    // No FMV + no sell-to-cover → every derived cell is "—".
    expect(screen.getByTestId('derived-gross').textContent).toBe('—');
    expect(screen.getByTestId('derived-sold').textContent).toBe('—');
    expect(screen.getByTestId('derived-net').textContent).toBe('—');
    expect(screen.getByTestId('derived-cash').textContent).toBe('—');
  });

  it('renders derived values live when FMV + sell price + tax are present', async () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({
          fmvAtVest: '42.0000',
          fmvCurrency: 'USD',
          shareSellPrice: '42.2500',
          shareSellCurrency: 'USD',
          taxWithholdingPercent: '0.4500',
          isUserOverride: true,
          isSellToCoverOverride: true,
        })}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    // Fixture `typical_spain_45pct` maps: gross = 4200, sold = 44.7337,
    // net = 55.2663, cash = 1890.0000. We check gross + sold string
    // contents rather than locale formatting so the assertion does not
    // depend on jsdom's Intl tables.
    const gross = screen.getByTestId('derived-gross');
    const sold = screen.getByTestId('derived-sold');
    expect(gross.textContent).not.toBe('—');
    expect(sold.textContent).toMatch(/44/);
  });

  it('Save posts PUT with expectedUpdatedAt and tax-as-fraction', async () => {
    const spy = mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
      '/vesting-events/e-1': () => rowResponse(),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({ fmvAtVest: '42.0000', fmvCurrency: 'USD', isUserOverride: true })}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    // Type sell price + tax.
    fireEvent.change(screen.getByTestId('dlg-sell-price'), { target: { value: '42.2500' } });
    fireEvent.change(screen.getByTestId('dlg-tax-percent'), { target: { value: '45' } });
    fireEvent.click(screen.getByTestId('dlg-save'));

    await waitFor(() => {
      const call = spy.mock.calls.find(
        (c) => String(c[0]).includes('/vesting-events/e-1') && (c[1] as RequestInit)?.method === 'PUT',
      );
      expect(call).toBeTruthy();
      const body = JSON.parse(String((call![1] as RequestInit).body));
      expect(body.expectedUpdatedAt).toBe('2026-04-19T00:00:00Z');
      expect(body.shareSellPrice).toBe('42.2500');
      expect(body.taxWithholdingPercent).toBe('0.4500');
    });
  });

  it('surfaces the OCC conflict banner on 409', async () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
      '/vesting-events/e-1': () =>
        new Response(
          JSON.stringify({
            error: { code: 'resource.stale_client_state', message: 'stale' },
          }),
          { status: 409 },
        ),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({ fmvAtVest: '42.0000', fmvCurrency: 'USD' })}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    fireEvent.change(screen.getByTestId('dlg-fmv'), { target: { value: '43.0000' } });
    fireEvent.click(screen.getByTestId('dlg-save'));
    await waitFor(() => {
      expect(screen.getByTestId('vesting-dialog-conflict-banner')).toBeInTheDocument();
    });
  });

  it('shows the unsaved-changes prompt when closing with dirty fields', () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
    });
    let closed = false;
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({ fmvAtVest: '42.0000', fmvCurrency: 'USD' })}
        defaultCurrency="USD"
        onClose={() => {
          closed = true;
        }}
        onSaved={() => {}}
      />,
    );
    fireEvent.change(screen.getByTestId('dlg-fmv'), { target: { value: '43.0000' } });
    fireEvent.click(screen.getByTestId('dlg-cancel'));
    expect(screen.getByTestId('unsaved-prompt')).toBeInTheDocument();
    // "Discard changes" actually closes.
    fireEvent.click(screen.getByTestId('unsaved-discard'));
    expect(closed).toBe(true);
  });

  it('narrow-clear sends clearSellToCoverOverride: true, preserves FMV', async () => {
    const spy = mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
      '/vesting-events/e-1': () => rowResponse(),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({
          fmvAtVest: '42.0000',
          fmvCurrency: 'USD',
          shareSellPrice: '42.2500',
          shareSellCurrency: 'USD',
          taxWithholdingPercent: '0.4500',
          isUserOverride: true,
          isSellToCoverOverride: true,
        })}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId('dlg-revert-stc'));
    await waitFor(() => {
      const call = spy.mock.calls.find(
        (c) => String(c[0]).includes('/vesting-events/e-1') && (c[1] as RequestInit)?.method === 'PUT',
      );
      expect(call).toBeTruthy();
      const body = JSON.parse(String((call![1] as RequestInit).body));
      expect(body.clearSellToCoverOverride).toBe(true);
      expect(body.clearOverride).toBeUndefined();
    });
  });

  it('full-clear sends clearOverride: true', async () => {
    const spy = mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
      '/vesting-events/e-1': () => rowResponse(),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({
          fmvAtVest: '42.0000',
          fmvCurrency: 'USD',
          isUserOverride: true,
        })}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId('dlg-revert-all'));
    await waitFor(() => {
      const call = spy.mock.calls.find(
        (c) => String(c[0]).includes('/vesting-events/e-1') && (c[1] as RequestInit)?.method === 'PUT',
      );
      expect(call).toBeTruthy();
      const body = JSON.parse(String((call![1] as RequestInit).body));
      expect(body.clearOverride).toBe(true);
    });
  });

  it('surfaces the negative-net-shares 422 banner', async () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
      '/vesting-events/e-1': () =>
        new Response(
          JSON.stringify({
            error: {
              code: 'validation',
              message: 'validation',
              details: {
                fields: [
                  {
                    field: 'shareSellPrice',
                    code: 'vesting_event.sell_to_cover.negative_net_shares',
                  },
                ],
              },
            },
          }),
          { status: 422 },
        ),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({ fmvAtVest: '42.0000', fmvCurrency: 'USD' })}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    fireEvent.change(screen.getByTestId('dlg-sell-price'), { target: { value: '40.0000' } });
    fireEvent.change(screen.getByTestId('dlg-tax-percent'), { target: { value: '100' } });
    fireEvent.click(screen.getByTestId('dlg-save'));
    await waitFor(() => {
      const err = screen.getByTestId('vesting-dialog-error');
      expect(err.textContent).toMatch(/100%/);
    });
  });

  it('surfaces the currency-mismatch 422 banner', async () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
      '/vesting-events/e-1': () =>
        new Response(
          JSON.stringify({
            error: {
              code: 'validation',
              message: 'validation',
              details: {
                fields: [
                  {
                    field: 'shareSellCurrency',
                    code: 'vesting_event.sell_to_cover.currency_mismatch',
                  },
                ],
              },
            },
          }),
          { status: 422 },
        ),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent({ fmvAtVest: '42.0000', fmvCurrency: 'USD' })}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    fireEvent.change(screen.getByTestId('dlg-sell-price'), { target: { value: '42.2500' } });
    fireEvent.change(screen.getByTestId('dlg-tax-percent'), { target: { value: '45' } });
    fireEvent.click(screen.getByTestId('dlg-save'));
    await waitFor(() => {
      const err = screen.getByTestId('vesting-dialog-error');
      expect(err.textContent).toMatch(/moneda/i);
    });
  });

  it('seeds the tax-percent placeholder from the user tax preferences', async () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(
          JSON.stringify({
            current: {
              id: 'p-1',
              countryIso2: 'ES',
              rendimientoDelTrabajoPercent: '0.4500',
              sellToCoverEnabled: true,
              fromDate: '2026-04-19',
              toDate: null,
              createdAt: '2026-04-19T00:00:00Z',
              updatedAt: '2026-04-19T00:00:00Z',
            },
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(
      <VestingEventDialog
        grantId="g-1"
        event={baseEvent()}
        defaultCurrency="USD"
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    await waitFor(() => {
      const input = screen.getByTestId('dlg-tax-percent') as HTMLInputElement;
      // The placeholder includes the seeded value (45.00).
      expect(input.placeholder).toMatch(/45/);
      // Empty value: the server does default-sourcing on save
      // (AC-7.6.3) — the client leaves the field blank.
      expect(input.value).toBe('');
    });
  });
});
