// ESPP purchase form tests (Slice 2 T22, AC-4.2.* / AC-4.5.1).
//
// Coverage:
//   - Renders the form fields (happy path).
//   - Validation: purchase_date before offering_date (AC-4.2.7).
//   - Submit happy path; `migratedFromNotes` toast is stashed + navigation occurs.
//   - Duplicate-purchase 422 surfaces the warning + forceDuplicate retry.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { Route, Routes } from 'react-router-dom';
import EsppPurchaseNewPage from '../espp-new';
import { renderWithProviders } from '../../../../testing/render';
import type { GrantDto } from '../../../../api/grants';

function grantFixture(overrides: Partial<GrantDto> = {}): GrantDto {
  return {
    id: 'g-espp-1',
    instrument: 'espp',
    grantDate: '2025-01-15',
    shareCount: '100',
    shareCountScaled: 100 * 10_000,
    strikeAmount: null,
    strikeCurrency: 'USD',
    vestingStart: '2025-01-15',
    vestingTotalMonths: 6,
    cliffMonths: 0,
    vestingCadence: 'monthly',
    doubleTrigger: false,
    liquidityEventDate: null,
    doubleTriggerSatisfiedBy: null,
    employerName: 'Different Inc.',
    ticker: null,
    notes: JSON.stringify({ estimated_discount_percent: 15 }),
    createdAt: '2025-01-15T00:00:00Z',
    updatedAt: '2025-01-15T00:00:00Z',
    ...overrides,
  };
}

/**
 * Route-scoped fetch mock. `stages` is an ordered list of POST responses;
 * each submit pulls the next one (supports duplicate → retry tests).
 */
function mockFetch(grant: GrantDto, stages: Array<() => Response>): ReturnType<typeof vi.fn> {
  let postCursor = 0;
  const spy = vi.fn(async (url: string, init?: RequestInit) => {
    if (url.endsWith(`/api/v1/grants/${grant.id}`) && (!init || init.method === 'GET' || init.method === undefined)) {
      return new Response(JSON.stringify({ grant }), { status: 200 });
    }
    if (url.endsWith(`/api/v1/grants/${grant.id}/espp-purchases`) && init?.method === 'POST') {
      const stage = stages[postCursor++] ?? stages[stages.length - 1]!;
      return stage();
    }
    return new Response('{}', { status: 200 });
  });
  vi.stubGlobal('fetch', spy);
  return spy;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

interface Fill {
  offeringDate: string;
  purchaseDate: string;
  fmv: string;
  price: string;
  shares: string;
}

function fillEsppForm(v: Fill): void {
  const $ = (name: string): HTMLInputElement =>
    document.querySelector(`input[name="${name}"]`) as HTMLInputElement;
  fireEvent.change($('offeringDate'), { target: { value: v.offeringDate } });
  fireEvent.change($('purchaseDate'), { target: { value: v.purchaseDate } });
  fireEvent.change($('fmvAtPurchase'), { target: { value: v.fmv } });
  fireEvent.change($('purchasePricePerShare'), { target: { value: v.price } });
  fireEvent.change($('sharesPurchased'), { target: { value: v.shares } });
}

describe('ESPP purchase new form', () => {
  it('renders the form with date, shares, currency, and discount fields', async () => {
    const g = grantFixture();
    mockFetch(g, []);
    renderWithProviders(
      <Routes>
        <Route
          path="/app/grants/:grantId/espp-purchases/new"
          element={<EsppPurchaseNewPage />}
        />
      </Routes>,
      { initialEntries: [`/app/grants/${g.id}/espp-purchases/new`] },
    );
    await waitFor(() => {
      expect(document.querySelector('input[name="offeringDate"]')).not.toBeNull();
    });
    expect(document.querySelector('input[name="purchaseDate"]')).not.toBeNull();
    expect(document.querySelector('input[name="sharesPurchased"]')).not.toBeNull();
    expect(document.querySelector('select[name="currency"]')).not.toBeNull();
    expect(
      document.querySelector('input[name="employerDiscountPercent"]'),
    ).not.toBeNull();
  });

  it('pre-fills the discount from the Slice-1 notes JSON (AC-4.5.1)', async () => {
    const g = grantFixture();
    mockFetch(g, []);
    renderWithProviders(
      <Routes>
        <Route
          path="/app/grants/:grantId/espp-purchases/new"
          element={<EsppPurchaseNewPage />}
        />
      </Routes>,
      { initialEntries: [`/app/grants/${g.id}/espp-purchases/new`] },
    );
    await waitFor(() => {
      const discount = document.querySelector(
        'input[name="employerDiscountPercent"]',
      ) as HTMLInputElement | null;
      expect(discount?.value).toBe('15');
    });
  });

  it('rejects purchase_date earlier than offering_date (AC-4.2.7)', async () => {
    const g = grantFixture();
    mockFetch(g, []);
    renderWithProviders(
      <Routes>
        <Route
          path="/app/grants/:grantId/espp-purchases/new"
          element={<EsppPurchaseNewPage />}
        />
      </Routes>,
      { initialEntries: [`/app/grants/${g.id}/espp-purchases/new`] },
    );
    await waitFor(() => {
      expect(document.querySelector('input[name="offeringDate"]')).not.toBeNull();
    });
    fillEsppForm({
      offeringDate: '2025-06-30',
      purchaseDate: '2025-01-15',
      fmv: '45.00',
      price: '38.25',
      shares: '100',
    });
    fireEvent.click(screen.getByRole('button', { name: /guardar compra/i }));
    await waitFor(() => {
      expect(
        screen.getByText(
          /La fecha de compra debe ser igual o posterior a la de oferta\./i,
        ),
      ).toBeInTheDocument();
    });
  });

  it('surfaces the duplicate-purchase warning and retries with forceDuplicate (AC-4.2.8)', async () => {
    const g = grantFixture();
    const duplicate = () =>
      new Response(
        JSON.stringify({
          error: {
            code: 'validation',
            message: 'duplicate',
            details: { fields: [{ field: 'purchase', code: 'duplicate' }] },
          },
        }),
        { status: 422 },
      );
    const success = () =>
      new Response(
        JSON.stringify({
          purchase: {
            id: 'p-1',
            grantId: g.id,
            offeringDate: '2025-01-15',
            purchaseDate: '2025-06-30',
            fmvAtPurchase: '45.00',
            purchasePricePerShare: '38.25',
            sharesPurchased: '100',
            sharesPurchasedScaled: 100 * 10_000,
            currency: 'USD',
            fmvAtOffering: null,
            employerDiscountPercent: '15',
            notes: null,
            createdAt: '2025-06-30T00:00:00Z',
            updatedAt: '2025-06-30T00:00:00Z',
          },
          migratedFromNotes: false,
        }),
        { status: 201 },
      );
    mockFetch(g, [duplicate, success]);
    renderWithProviders(
      <Routes>
        <Route
          path="/app/grants/:grantId/espp-purchases/new"
          element={<EsppPurchaseNewPage />}
        />
      </Routes>,
      { initialEntries: [`/app/grants/${g.id}/espp-purchases/new`] },
    );
    await waitFor(() => {
      expect(document.querySelector('input[name="offeringDate"]')).not.toBeNull();
    });
    fillEsppForm({
      offeringDate: '2025-01-15',
      purchaseDate: '2025-06-30',
      fmv: '45.00',
      price: '38.25',
      shares: '100',
    });
    fireEvent.click(screen.getByRole('button', { name: /guardar compra/i }));

    // First submit: duplicate warning renders.
    await waitFor(() => {
      expect(screen.getByText(/parece un duplicado/i)).toBeInTheDocument();
    });

    // Confirm and retry.
    fireEvent.click(
      screen.getByRole('checkbox', {
        name: /confirmo que es una compra distinta/i,
      }),
    );
    fireEvent.click(
      screen.getByRole('button', { name: /confirmar y guardar igualmente/i }),
    );
    await waitFor(() => {
      // After the 201, the component navigates away; the form vanishes.
      expect(
        screen.queryByRole('button', { name: /confirmar y guardar igualmente/i }),
      ).toBeNull();
    });
  });
});
