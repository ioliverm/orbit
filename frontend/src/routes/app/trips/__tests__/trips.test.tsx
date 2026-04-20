// Art. 7.p trip list + new form tests (Slice 2 T22, AC-5.*).
//
// Coverage:
//   - New form: renders trip facts + the five-criterion checklist.
//   - Criterion checklist interaction (click Sí / No on each row).
//   - Submit rejects when any criterion is left blank (AC-5.2.3).
//   - List view renders annual cap tracker + the criteria-met chip.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { Route, Routes } from 'react-router-dom';
import TripNewPage from '../new';
import TripsIndexPage from '..';
import { renderWithProviders } from '../../../../testing/render';
import { ART_7P_CRITERION_KEYS, type TripDto } from '../../../../api/trips';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('Art. 7.p new trip form', () => {
  it('renders trip facts + all five criterion rows (AC-5.1.*)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 200 })),
    );
    renderWithProviders(<TripNewPage />, { initialEntries: ['/app/trips/new'] });

    await waitFor(() => {
      expect(document.querySelector('select[name="destinationCountry"]')).not.toBeNull();
    });
    expect(document.querySelector('input[name="fromDate"]')).not.toBeNull();
    expect(document.querySelector('input[name="toDate"]')).not.toBeNull();
    // 5 criterion rows + 1 employer-paid group → 6 role=radiogroup.
    const radiogroups = screen.getAllByRole('radiogroup');
    expect(radiogroups.length).toBeGreaterThanOrEqual(6);
  });

  it('rejects submit when criteria are incomplete (AC-5.2.3)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 200 })),
    );
    renderWithProviders(<TripNewPage />, { initialEntries: ['/app/trips/new'] });

    await waitFor(() => {
      expect(document.querySelector('select[name="destinationCountry"]')).not.toBeNull();
    });
    fireEvent.change(
      document.querySelector('select[name="destinationCountry"]') as HTMLSelectElement,
      { target: { value: 'US' } },
    );
    fireEvent.change(
      document.querySelector('input[name="fromDate"]') as HTMLInputElement,
      { target: { value: '2026-03-01' } },
    );
    fireEvent.change(
      document.querySelector('input[name="toDate"]') as HTMLInputElement,
      { target: { value: '2026-04-15' } },
    );
    // Only answer 2 of 5 criteria.
    const yes1 = document.querySelectorAll(
      'input[name="criteria.services_outside_spain"]',
    )[0] as HTMLInputElement;
    fireEvent.click(yes1);
    const yes2 = document.querySelectorAll(
      'input[name="criteria.non_spanish_employer"]',
    )[0] as HTMLInputElement;
    fireEvent.click(yes2);

    fireEvent.click(screen.getByRole('button', { name: /guardar desplazamiento/i }));
    await waitFor(() => {
      const alert = document.getElementById('criteria-err');
      expect(alert).not.toBeNull();
      expect(alert?.textContent).toMatch(
        /responde a los cinco criterios antes de guardar/i,
      );
    });
  });

  it('accepts submit when all five criteria are answered (happy path)', async () => {
    const spy = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.endsWith('/api/v1/trips') && init?.method === 'POST') {
        return new Response(
          JSON.stringify({
            trip: {
              id: 't-1',
              destinationCountry: 'US',
              fromDate: '2026-03-01',
              toDate: '2026-04-15',
              employerPaid: true,
              purpose: null,
              eligibilityCriteria: {
                services_outside_spain: true,
                non_spanish_employer: true,
                not_tax_haven: true,
                no_double_exemption: true,
                within_annual_cap: true,
              },
              createdAt: '2026-03-01T00:00:00Z',
              updatedAt: '2026-03-01T00:00:00Z',
            },
          }),
          { status: 201 },
        );
      }
      return new Response('{}', { status: 200 });
    });
    vi.stubGlobal('fetch', spy);

    renderWithProviders(
      <Routes>
        <Route path="/app/trips/new" element={<TripNewPage />} />
        <Route path="/app/trips" element={<div>trips-list-stub</div>} />
      </Routes>,
      { initialEntries: ['/app/trips/new'] },
    );

    await waitFor(() => {
      expect(document.querySelector('select[name="destinationCountry"]')).not.toBeNull();
    });
    fireEvent.change(
      document.querySelector('select[name="destinationCountry"]') as HTMLSelectElement,
      { target: { value: 'US' } },
    );
    fireEvent.change(
      document.querySelector('input[name="fromDate"]') as HTMLInputElement,
      { target: { value: '2026-03-01' } },
    );
    fireEvent.change(
      document.querySelector('input[name="toDate"]') as HTMLInputElement,
      { target: { value: '2026-04-15' } },
    );
    for (const key of ART_7P_CRITERION_KEYS) {
      const yes = document.querySelectorAll(
        `input[name="criteria.${key}"]`,
      )[0] as HTMLInputElement;
      fireEvent.click(yes);
    }
    fireEvent.click(screen.getByRole('button', { name: /guardar desplazamiento/i }));

    await waitFor(() => {
      expect(screen.getByText(/trips-list-stub/i)).toBeInTheDocument();
    });
    // POST was issued.
    const posts = spy.mock.calls.filter(
      (c) => (c[1] as RequestInit | undefined)?.method === 'POST',
    );
    expect(posts.length).toBe(1);
  });
});

describe('Art. 7.p trip list', () => {
  it('renders the annual cap tracker + a criteria-met chip per row', async () => {
    const trip: TripDto = {
      id: 't-9',
      destinationCountry: 'US',
      fromDate: '2026-03-01',
      toDate: '2026-04-15',
      employerPaid: true,
      purpose: 'Kickoff',
      eligibilityCriteria: {
        services_outside_spain: true,
        non_spanish_employer: true,
        not_tax_haven: true,
        no_double_exemption: true,
        within_annual_cap: true,
      },
      createdAt: '2026-03-01T00:00:00Z',
      updatedAt: '2026-03-01T00:00:00Z',
    };
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (url.includes('/api/v1/trips')) {
          return new Response(
            JSON.stringify({
              trips: [trip],
              annualCapTracker: {
                year: 2026,
                tripCount: 1,
                dayCountDeclared: 46,
                employerPaidTripCount: 1,
                criteriaMetCountByKey: {
                  services_outside_spain: 1,
                  non_spanish_employer: 1,
                  not_tax_haven: 1,
                  no_double_exemption: 1,
                  within_annual_cap: 1,
                },
              },
            }),
            { status: 200 },
          );
        }
        return new Response('{}', { status: 200 });
      }),
    );
    renderWithProviders(<TripsIndexPage />, { initialEntries: ['/app/trips'] });

    await waitFor(() => {
      expect(screen.getByTestId('annual-cap-tracker')).toBeInTheDocument();
    });
    // Wait for the trip row to be rendered (initial render shows Cargando…).
    await waitFor(() => {
      expect(screen.getByTestId('trip-row')).toBeInTheDocument();
    });
    // Chip text is composed across spans; assert the containing row does
    // carry the "5/5" signal plus the Apto label.
    const row = screen.getByTestId('trip-row');
    expect(row.textContent).toMatch(/5\/5/);
    expect(row.textContent?.toLowerCase()).toContain('apto');
    // Day count declared from the tracker surfaces.
    expect(screen.getByText(/46/)).toBeInTheDocument();
  });
});
