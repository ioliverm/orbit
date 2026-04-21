// Editable "Precios de vesting" section tests (Slice 3 T30).

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

  it('enters edit mode on Editar click and saves via PUT', async () => {
    const fetchSpy = mockFetch({
      '/vesting-events/e-1': () =>
        new Response(
          JSON.stringify({
            id: 'e-1',
            grantId: 'g-1',
            vestDate: '2025-10-15',
            sharesVestedThisEvent: '500',
            sharesVestedThisEventScaled: 5_000_000,
            cumulativeSharesVested: '500',
            fmvAtVest: '42.00',
            fmvCurrency: 'USD',
            isUserOverride: true,
            updatedAt: '2026-04-19T01:00:00Z',
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(
      <VestingEventsEditor grantId="g-1" events={[baseEvent()]} defaultCurrency="USD" />,
    );
    fireEvent.click(screen.getByTestId('vesting-row-edit'));
    const fmvInput = screen.getByTestId('vesting-row-fmv') as HTMLInputElement;
    fireEvent.change(fmvInput, { target: { value: '42.00' } });
    fireEvent.click(screen.getByTestId('vesting-row-save'));
    await waitFor(() => {
      expect(
        fetchSpy.mock.calls.some(
          (c) => String(c[0]).includes('/vesting-events/e-1') && (c[1] as RequestInit)?.method === 'PUT',
        ),
      ).toBe(true);
    });
  });

  it('cancels edit on Escape key', () => {
    renderWithProviders(
      <VestingEventsEditor grantId="g-1" events={[baseEvent()]} defaultCurrency="USD" />,
    );
    fireEvent.click(screen.getByTestId('vesting-row-edit'));
    const fmvInput = screen.getByTestId('vesting-row-fmv') as HTMLInputElement;
    fireEvent.keyDown(fmvInput, { key: 'Escape' });
    expect(screen.queryByTestId('vesting-row-save')).toBeNull();
  });

  it('saves on Enter key', async () => {
    const fetchSpy = mockFetch({
      '/vesting-events/e-1': () =>
        new Response(
          JSON.stringify({
            id: 'e-1',
            grantId: 'g-1',
            vestDate: '2025-10-15',
            sharesVestedThisEvent: '500',
            sharesVestedThisEventScaled: 5_000_000,
            cumulativeSharesVested: '500',
            fmvAtVest: '50.00',
            fmvCurrency: 'USD',
            isUserOverride: true,
            updatedAt: '2026-04-19T01:00:00Z',
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(
      <VestingEventsEditor grantId="g-1" events={[baseEvent()]} defaultCurrency="USD" />,
    );
    fireEvent.click(screen.getByTestId('vesting-row-edit'));
    const fmvInput = screen.getByTestId('vesting-row-fmv') as HTMLInputElement;
    fireEvent.change(fmvInput, { target: { value: '50.00' } });
    fireEvent.keyDown(fmvInput, { key: 'Enter' });
    await waitFor(() => {
      expect(
        fetchSpy.mock.calls.some(
          (c) => String(c[0]).includes('/vesting-events/e-1') && (c[1] as RequestInit)?.method === 'PUT',
        ),
      ).toBe(true);
    });
  });

  it('surfaces reload banner on 409 conflict', async () => {
    mockFetch({
      '/vesting-events/e-1': () =>
        new Response(
          JSON.stringify({
            error: {
              code: 'resource.stale_client_state',
              message: 'stale',
            },
          }),
          { status: 409 },
        ),
    });
    renderWithProviders(
      <VestingEventsEditor grantId="g-1" events={[baseEvent()]} defaultCurrency="USD" />,
    );
    fireEvent.click(screen.getByTestId('vesting-row-edit'));
    const fmvInput = screen.getByTestId('vesting-row-fmv') as HTMLInputElement;
    fireEvent.change(fmvInput, { target: { value: '40.00' } });
    fireEvent.click(screen.getByTestId('vesting-row-save'));
    await waitFor(() => {
      expect(screen.getByTestId('vesting-conflict-banner')).toBeInTheDocument();
    });
  });

  it('fractional shares round-trip through the editor without precision loss (T33 S2)', async () => {
    // Sanity-check the scaled ↔ decimal bridge: 1.2345 * 10_000 = 12345.
    expect(scaledToDecimalShares(12_345)).toBe('1.2345');

    // Start the row at whole shares (500) so typing "1.2345" is a
    // genuine change — otherwise the editor's same-value short-circuit
    // omits `sharesVested` from the PUT body.
    const initialScaled = 500 * 10_000;

    const fetchSpy = mockFetch({
      '/vesting-events/e-1': () =>
        new Response(
          JSON.stringify({
            id: 'e-1',
            grantId: 'g-1',
            vestDate: '2025-10-15',
            sharesVestedThisEvent: '1.2345',
            sharesVestedThisEventScaled: 12_345,
            cumulativeSharesVested: '1.2345',
            fmvAtVest: null,
            fmvCurrency: null,
            isUserOverride: true,
            updatedAt: '2026-04-19T01:00:00Z',
          }),
          { status: 200 },
        ),
    });
    renderWithProviders(
      <VestingEventsEditor
        grantId="g-1"
        events={[baseEvent({ sharesVestedThisEventScaled: initialScaled })]}
        defaultCurrency="USD"
      />,
    );
    fireEvent.click(screen.getByTestId('vesting-row-edit'));

    // Change the shares to 1.2345 explicitly, then save. Before the
    // fix this would be truncated to 1 because the input was backed by
    // Math.floor and sent via Number(shares).
    const inputs = screen.getAllByRole('textbox');
    const sharesInput = inputs.find(
      (el) => (el as HTMLInputElement).getAttribute('aria-label') === 'Acciones',
    ) as HTMLInputElement | undefined;
    // The editor currently renders shares as type="number"; fall back
    // to looking by aria-label across all inputs if the role filter
    // missed it.
    let targetInput = sharesInput;
    if (!targetInput) {
      const byAria = document.querySelector(
        'input[aria-label="Acciones"]',
      ) as HTMLInputElement | null;
      if (byAria) targetInput = byAria;
    }
    expect(targetInput).toBeTruthy();
    fireEvent.change(targetInput as HTMLInputElement, {
      target: { value: '1.2345' },
    });
    fireEvent.click(screen.getByTestId('vesting-row-save'));

    await waitFor(() => {
      const call = fetchSpy.mock.calls.find(
        (c) =>
          String(c[0]).includes('/vesting-events/e-1') &&
          (c[1] as RequestInit)?.method === 'PUT',
      );
      expect(call).toBeTruthy();
      const body = JSON.parse(String((call![1] as RequestInit).body));
      // The wire value must be the string "1.2345", not the number
      // 1.2345 or the floored integer 1.
      expect(body.sharesVested).toBe('1.2345');
    });
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
