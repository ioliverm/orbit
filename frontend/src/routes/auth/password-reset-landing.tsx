// /password-reset?token=... — stub for the token-landing form.
// Backend endpoint POST /auth/reset is not implemented in T13a/b.
// TODO(backend T1x): wire POST /api/v1/auth/reset (ADR-011 §Flows).

import { Trans } from '@lingui/macro';
import { Link } from 'react-router-dom';
import { ErrorBanner } from '../../components/feedback/ErrorBanner';

export default function PasswordResetLandingPage(): JSX.Element {
  return (
    <section className="auth-card" aria-labelledby="pr-land-title">
      <h1 id="pr-land-title" className="auth-card__title">
        <Trans>Elige una contraseña nueva</Trans>
      </h1>
      <ErrorBanner variant="info" title={<Trans>Aún no disponible</Trans>}>
        <Trans>
          El restablecimiento de contraseña se habilitará en una iteración posterior. Si tu enlace
          caduca mientras tanto, pide uno nuevo cuando la función esté disponible.
        </Trans>
      </ErrorBanner>
      <Link className="btn btn--secondary" to="/signin">
        <Trans>Volver a iniciar sesión</Trans>
      </Link>
    </section>
  );
}
