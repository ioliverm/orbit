// /app/onboarding/residency — T14b stub.
// The full AC-4.1.* form ships in T14b. We render a placeholder so the
// route exists and router tests pass.

import { Trans } from '@lingui/macro';
import { useOnboardingGate } from '../../hooks/useOnboardingGate';

export default function ResidencyStepPage(): JSX.Element {
  useOnboardingGate('residency');
  return (
    <section className="auth-card auth-card--wide" aria-labelledby="residency-title">
      <h1 id="residency-title" className="auth-card__title">
        <Trans>Paso 4 — Tu residencia fiscal</Trans>
      </h1>
      <p className="auth-card__sub">
        <Trans>El formulario de residencia llega en la siguiente iteración (T14b).</Trans>
      </p>
    </section>
  );
}
