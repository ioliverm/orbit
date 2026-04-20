// /app/onboarding/first-grant — T14b stub.

import { Trans } from '@lingui/macro';
import { useOnboardingGate } from '../../hooks/useOnboardingGate';

export default function FirstGrantStepPage(): JSX.Element {
  useOnboardingGate('first_grant');
  return (
    <section className="auth-card auth-card--wide" aria-labelledby="fg-title">
      <h1 id="fg-title" className="auth-card__title">
        <Trans>Paso 5 — Añade tu primer grant</Trans>
      </h1>
      <p className="auth-card__sub">
        <Trans>El formulario del primer grant llega en la siguiente iteración (T14b).</Trans>
      </p>
    </section>
  );
}
