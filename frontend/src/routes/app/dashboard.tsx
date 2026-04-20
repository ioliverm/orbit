// /app/dashboard — T14b will fill this in. For T14a we render a minimal
// Slice-1 empty-state placeholder (AC-5.1.1 copy) so the shell has a
// sensible landing page post-onboarding.

import { Trans } from '@lingui/macro';
import { useOnboardingGate } from '../../hooks/useOnboardingGate';

export default function DashboardPage(): JSX.Element {
  useOnboardingGate('complete');
  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Tu cartera</Trans>
          </h1>
          <p className="page-title__meta">
            <Trans>Todavía no has añadido ningún grant.</Trans>
          </p>
        </div>
      </div>
      <section className="card" aria-labelledby="empty-state">
        <h2 id="empty-state" className="auth-card__title">
          <Trans>Empieza añadiendo tu primer grant</Trans>
        </h2>
        <p className="muted text-sm">
          <Trans>
            Un grant es la concesión de acciones de una empresa (RSU, NSO, ISO, ESPP). Registrarlo
            en Orbit te permite visualizar tu vesting.
          </Trans>
        </p>
      </section>
    </>
  );
}
