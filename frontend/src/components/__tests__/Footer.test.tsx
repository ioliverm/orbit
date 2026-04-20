// Footer copy tests (G-1..G-7).
//   G-2: ES footer copy verbatim
//   G-3: EN footer copy verbatim
//   G-5: no rule-set chip in Slice 1.

import { describe, expect, it, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { act } from 'react';
import { Footer } from '../Layout/Footer';
import { renderWithProviders } from '../../testing/render';
import { useLocaleStore } from '../../store/locale';

describe('Footer', () => {
  beforeEach(() => {
    act(() => {
      useLocaleStore.getState().setLocale('es-ES');
    });
  });

  it('renders the exact ES copy from G-2', () => {
    renderWithProviders(<Footer />);
    expect(
      screen.getByText('Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja.'),
    ).toBeInTheDocument();
  });

  it('renders the exact EN copy from G-3 after switching locale', () => {
    act(() => {
      useLocaleStore.getState().setLocale('en');
    });
    renderWithProviders(<Footer />);
    expect(
      screen.getByText(
        "This is not tax or financial advice — Orbit calculates, it doesn't recommend.",
      ),
    ).toBeInTheDocument();
  });

  it('does not render a rule-set chip in Slice 1 (G-5)', () => {
    const { container } = renderWithProviders(<Footer />);
    expect(container.querySelector('.chip')).toBeNull();
    expect(screen.queryByText(/ver trazabilidad/i)).toBeNull();
  });

  it('is marked as a contentinfo landmark (G-17)', () => {
    renderWithProviders(<Footer />);
    expect(screen.getByRole('contentinfo')).toBeInTheDocument();
  });
});
