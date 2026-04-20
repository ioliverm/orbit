// Test-render helpers. A single <AppProviders> component wraps I18n + Query
// + Router so individual test files don't duplicate the boilerplate.

import { I18nProvider } from '@lingui/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, type RenderOptions, type RenderResult } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { activateLocale, i18n } from '../i18n';

// Force ES locale on every test render so string-matching assertions in
// other files don't drift when a sibling test switches locale.
activateLocale('es-ES');

export interface WrapperOptions {
  initialEntries?: string[];
  client?: QueryClient;
  locale?: 'es-ES' | 'en';
}

export function renderWithProviders(
  ui: React.ReactElement,
  opts: WrapperOptions & Omit<RenderOptions, 'wrapper'> = {},
): RenderResult {
  activateLocale(opts.locale ?? 'es-ES');
  const client =
    opts.client ??
    new QueryClient({
      defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
    });
  const entries = opts.initialEntries ?? ['/'];

  function Wrapper({ children }: { children: React.ReactNode }): JSX.Element {
    return (
      <I18nProvider i18n={i18n}>
        <QueryClientProvider client={client}>
          <MemoryRouter initialEntries={entries}>{children}</MemoryRouter>
        </QueryClientProvider>
      </I18nProvider>
    );
  }
  return render(ui, { wrapper: Wrapper, ...opts });
}

export { i18n };
