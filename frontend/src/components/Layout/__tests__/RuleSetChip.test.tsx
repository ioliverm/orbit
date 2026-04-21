// Rule-set chip unit tests (Slice 3 T30).

import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { renderWithProviders } from '../../../testing/render';
import { RuleSetChip } from '../RuleSetChip';

function mockChip(body: unknown, status = 200): void {
  const spy = vi.fn(async (url: string) => {
    if (String(url).includes('/rule-set-chip')) {
      return new Response(
        typeof body === 'string' ? body : JSON.stringify(body),
        { status },
      );
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('RuleSetChip', () => {
  it('renders fresh chip with ECB date and engine version', async () => {
    mockChip({ fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' });
    renderWithProviders(<RuleSetChip />);
    const chip = await screen.findByTestId('rule-set-chip');
    expect(chip.getAttribute('data-staleness')).toBe('fresh');
    expect(chip.textContent).toContain('motor v0.3.0');
  });

  it('renders walkback variant for 1–2 day staleness', async () => {
    mockChip({ fxDate: '2026-04-17', stalenessDays: 2, engineVersion: '0.3.0' });
    renderWithProviders(<RuleSetChip />);
    const chip = await screen.findByTestId('rule-set-chip');
    expect(chip.getAttribute('data-staleness')).toBe('walkback');
    expect(chip.textContent).toMatch(/stale 2 día/);
  });

  it('renders stale variant for 3–7 day staleness', async () => {
    mockChip({ fxDate: '2026-04-12', stalenessDays: 5, engineVersion: '0.3.0' });
    renderWithProviders(<RuleSetChip />);
    const chip = await screen.findByTestId('rule-set-chip');
    expect(chip.getAttribute('data-staleness')).toBe('stale');
  });

  it('hides when fxDate is null (unavailable)', async () => {
    mockChip({ fxDate: null, stalenessDays: null, engineVersion: '0.3.0' });
    renderWithProviders(<RuleSetChip />);
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('rule-set-chip')).toBeNull();
  });

  it('hides on 401 (unauthenticated routes)', async () => {
    mockChip({ error: { code: 'unauthenticated' } }, 401);
    renderWithProviders(<RuleSetChip />);
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('rule-set-chip')).toBeNull();
  });

  it('opens and closes the explainer popover on click', async () => {
    mockChip({ fxDate: '2026-04-17', stalenessDays: 0, engineVersion: '0.3.0' });
    renderWithProviders(<RuleSetChip />);
    const chip = await screen.findByTestId('rule-set-chip');
    fireEvent.click(chip);
    await waitFor(() => {
      expect(screen.getByTestId('chip-popover')).toBeInTheDocument();
    });
    fireEvent.click(chip);
    await waitFor(() => {
      expect(screen.queryByTestId('chip-popover')).toBeNull();
    });
  });
});
