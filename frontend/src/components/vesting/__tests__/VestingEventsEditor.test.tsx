// Editable "Precios de vesting" section tests (Slice 3 T30 + Slice 3b
// T39). The Slice-3 inline row-edit pattern was replaced by a per-row
// dialog (`VestingEventDialog`); these tests cover the display-only
// editor shell (relaxed-invariant banner + bulk-fill + dialog-opening
// wiring). Dialog-centric assertions live in `VestingEventDialog.test.tsx`.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { renderWithProviders } from '../../../testing/render';
import {
  VestingEventsEditor,
  scaledToDecimalShares,
  type EditableVestingEvent,
} from '../VestingEventsEditor';

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

function baseEvent(overrides: Partial<EditableVestingEvent> = {}): EditableVestingEvent {
  return {
    id: 'e-1',
    vestDate: '2025-10-15',
    sharesVestedThisEventScaled: 500 * 10_000,
    fmvAtVest: null,
    fmvCurrency: null,
    isUserOverride: false,
    updatedAt: '2026-04-19T00:00:00Z',
    state: 'vested',
    ...overrides,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('VestingEventsEditor', () => {
  it('shows the relaxed-invariant banner when any row has isUserOverride', () => {
    renderWithProviders(
      <VestingEventsEditor
        grantId="g-1"
        events={[baseEvent({ isUserOverride: true, fmvAtVest: '42.00', fmvCurrency: 'USD' })]}
        defaultCurrency="USD"
      />,
    );
    expect(screen.getByTestId('relaxed-invariant-banner')).toBeInTheDocument();
  });

  it('opens the dialog when Editar is clicked', () => {
    mockFetch({
      '/user-tax-preferences/current': () =>
        new Response(JSON.stringify({ current: null }), { status: 200 }),
    });
    renderWithProviders(
      <VestingEventsEditor grantId="g-1" events={[baseEvent()]} defaultCurrency="USD" />,
    );
    fireEvent.click(screen.getByTestId('vesting-row-edit'));
    expect(screen.getByTestId('vesting-dialog')).toBeInTheDocument();
  });

  it('scaledToDecimalShares preserves 4-dp precision', () => {
    expect(scaledToDecimalShares(12_345)).toBe('1.2345');
    expect(scaledToDecimalShares(500 * 10_000)).toBe('500');
  });

  it('bulk-fill toast reports skip count', async () => {
    mockFetch({
      '/vesting-events/bulk-fmv': () =>
        new Response(
          JSON.stringify({ appliedCount: 9, skippedCount: 3 }),
          { status: 200 },
        ),
    });
    renderWithProviders(
      <VestingEventsEditor grantId="g-1" events={[baseEvent()]} defaultCurrency="USD" />,
    );
    fireEvent.click(screen.getByTestId('bulk-fmv-open'));
    const fmv = screen.getByTestId('bulk-fmv-input') as HTMLInputElement;
    fireEvent.change(fmv, { target: { value: '40.00' } });
    fireEvent.click(screen.getByTestId('bulk-fmv-submit'));
    await waitFor(() => {
      const toast = screen.getByTestId('bulk-fmv-toast');
      expect(toast.textContent).toMatch(/9/);
      expect(toast.textContent).toMatch(/3/);
    });
  });
});
