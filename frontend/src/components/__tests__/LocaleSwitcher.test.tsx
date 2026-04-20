import { describe, expect, it, beforeEach } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { act } from 'react';
import { LocaleSwitcher } from '../Layout/LocaleSwitcher';
import { renderWithProviders } from '../../testing/render';
import { useLocaleStore } from '../../store/locale';

function readCookie(name: string): string | null {
  const match = document.cookie
    .split(';')
    .map((p) => p.trim())
    .find((p) => p.startsWith(`${name}=`));
  return match ? decodeURIComponent(match.slice(name.length + 1)) : null;
}

describe('LocaleSwitcher', () => {
  beforeEach(() => {
    act(() => {
      useLocaleStore.getState().setLocale('es-ES');
    });
    document.cookie = 'orbit_locale=; Max-Age=0; Path=/';
  });

  it('shows the active locale and offers both options', () => {
    renderWithProviders(<LocaleSwitcher />);
    const select = screen.getByRole('combobox') as HTMLSelectElement;
    expect(select.value).toBe('es-ES');
    expect(screen.getByRole('option', { name: /español/i })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /english/i })).toBeInTheDocument();
  });

  it('updates the store and persists to orbit_locale cookie on change', () => {
    renderWithProviders(<LocaleSwitcher />);
    const select = screen.getByRole('combobox') as HTMLSelectElement;

    fireEvent.change(select, { target: { value: 'en' } });

    expect(useLocaleStore.getState().locale).toBe('en');
    expect(readCookie('orbit_locale')).toBe('en');
  });
});
