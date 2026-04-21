// M720 threshold banner unit tests (Slice 3 T30).

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor, fireEvent } from '@testing-library/react';
import { renderWithProviders } from '../../../testing/render';
import { M720ThresholdBanner } from '../M720ThresholdBanner';

function mockThreshold(body: Record<string, unknown>): void {
  const spy = vi.fn(async (url: string) => {
    if (String(url).includes('/dashboard/modelo-720-threshold')) {
      return new Response(JSON.stringify(body), { status: 200 });
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
}

beforeEach(() => {
  sessionStorage.clear();
});

afterEach(() => {
  vi.unstubAllGlobals();
  sessionStorage.clear();
});

describe('M720ThresholdBanner', () => {
  it('renders nothing below threshold', async () => {
    mockThreshold({
      bankAccountsEur: '10000.00',
      realEstateEur: null,
      securitiesEur: '0.00',
      perCategoryBreach: false,
      aggregateBreach: false,
      thresholdEur: '50000.00',
      fxSensitivityBand: null,
      fxDate: '2026-04-19',
    });
    renderWithProviders(<M720ThresholdBanner />);
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('m720-threshold-banner')).toBeNull();
  });

  it('renders aggregate variant on aggregate breach', async () => {
    mockThreshold({
      bankAccountsEur: '30000.00',
      realEstateEur: '25000.00',
      securitiesEur: '0.00',
      perCategoryBreach: false,
      aggregateBreach: true,
      thresholdEur: '50000.00',
      fxSensitivityBand: null,
      fxDate: '2026-04-19',
    });
    renderWithProviders(<M720ThresholdBanner />);
    const banner = await screen.findByTestId('m720-threshold-banner');
    expect(banner.getAttribute('data-variant')).toBe('aggregate');
  });

  it('renders per-category variant on single-category breach', async () => {
    mockThreshold({
      bankAccountsEur: '75000.00',
      realEstateEur: null,
      securitiesEur: null,
      perCategoryBreach: true,
      aggregateBreach: true,
      thresholdEur: '50000.00',
      fxSensitivityBand: null,
      fxDate: '2026-04-19',
    });
    renderWithProviders(<M720ThresholdBanner />);
    const banner = await screen.findByTestId('m720-threshold-banner');
    expect(banner.getAttribute('data-variant')).toBe('per-category');
  });

  it('dismissal persists for the session', async () => {
    mockThreshold({
      bankAccountsEur: '75000.00',
      realEstateEur: null,
      securitiesEur: null,
      perCategoryBreach: true,
      aggregateBreach: true,
      thresholdEur: '50000.00',
      fxSensitivityBand: null,
      fxDate: '2026-04-19',
    });
    const { unmount } = renderWithProviders(<M720ThresholdBanner />);
    await screen.findByTestId('m720-threshold-banner');
    fireEvent.click(screen.getByTestId('m720-threshold-dismiss'));
    await waitFor(() => {
      expect(screen.queryByTestId('m720-threshold-banner')).toBeNull();
    });
    // Unmount, re-render: session flag still holds.
    unmount();
    renderWithProviders(<M720ThresholdBanner />);
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByTestId('m720-threshold-banner')).toBeNull();
  });

  it('renders incomplete-values footnote when securitiesEur is null', async () => {
    mockThreshold({
      bankAccountsEur: '60000.00',
      realEstateEur: null,
      securitiesEur: null,
      perCategoryBreach: true,
      aggregateBreach: true,
      thresholdEur: '50000.00',
      fxSensitivityBand: null,
      fxDate: '2026-04-19',
    });
    renderWithProviders(<M720ThresholdBanner />);
    await screen.findByTestId('m720-threshold-banner');
    expect(screen.getByText(/valores incompletos/i)).toBeInTheDocument();
  });
});
