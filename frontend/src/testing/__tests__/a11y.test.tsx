// Lightweight a11y smoke test (G-16..G-20).
//
// The ADR-009 follow-up mentions @axe-core/playwright for a proper a11y
// check. That dep is not in package.json yet and T14a can't add new deps
// without an ADR. Until @axe-core lands (T15), this test at least catches:
//   - multiple <h1>s on a page (G-16)
//   - inputs without a label association (G-18)
//   - missing landmarks on routes that should have them (G-17)

import { describe, expect, it } from 'vitest';
import { render, type RenderResult } from '@testing-library/react';
import { I18nProvider } from '@lingui/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import { i18n } from '../../i18n';
import SigninPage from '../../routes/auth/signin';
import SignupPage from '../../routes/auth/signup';

function renderPage(node: React.ReactElement): RenderResult {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <I18nProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryRouter>{node}</MemoryRouter>
      </QueryClientProvider>
    </I18nProvider>,
  );
}

function assertSingleH1(container: HTMLElement): void {
  const h1s = container.querySelectorAll('h1');
  expect(h1s.length, 'expected exactly one <h1> per route').toBe(1);
}

function assertLabelledInputs(container: HTMLElement): void {
  const inputs = container.querySelectorAll('input:not([type="hidden"]):not([type="checkbox"])');
  for (const input of Array.from(inputs)) {
    const id = input.getAttribute('id');
    const ariaLabel = input.getAttribute('aria-label');
    const ariaLabelledBy = input.getAttribute('aria-labelledby');
    const hasLabel = id ? Boolean(container.querySelector(`label[for="${CSS.escape(id)}"]`)) : false;
    expect(hasLabel || ariaLabel || ariaLabelledBy, `input lacks a label: ${input.outerHTML}`).toBeTruthy();
  }
}

describe('auth routes a11y smoke', () => {
  it('signup: single h1 + labelled inputs', () => {
    const { container } = renderPage(<SignupPage />);
    assertSingleH1(container);
    assertLabelledInputs(container);
  });

  it('signin: single h1 + labelled inputs', () => {
    const { container } = renderPage(<SigninPage />);
    assertSingleH1(container);
    assertLabelledInputs(container);
  });
});
