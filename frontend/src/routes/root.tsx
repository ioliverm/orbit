import { Trans } from '@lingui/macro';
import type { RouteObject } from 'react-router-dom';

// Slice 0a placeholder route. Proves the i18n pipeline and the CSP-clean shell.
// Feature routes (signup, signin, /app/*) land in later slices per ADR-009 §Routing map.
function RootPage(): JSX.Element {
  return (
    <main className="auth-shell">
      <div className="auth-shell__topbar">
        <span className="auth-shell__brand">
          ORBIT<span>.</span>
        </span>
      </div>
      <div className="auth-shell__main">
        <div className="auth-card">
          <h1 className="auth-card__title">
            <Trans>Orbit — Slice 0a scaffold</Trans>
          </h1>
          <p className="auth-card__sub">
            <Trans>Security envelope first. Features follow.</Trans>
          </p>
        </div>
      </div>
    </main>
  );
}

export const rootRoute: RouteObject = {
  path: '/',
  element: <RootPage />,
};
