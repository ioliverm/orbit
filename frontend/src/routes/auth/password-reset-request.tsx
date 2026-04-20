// /password-reset/request — stub.
//
// The backend does not implement POST /auth/forgot in T13a/b (see
// backend/crates/orbit-api/src/router.rs). We render the form per
// password-reset.html State A but disable submit with a visible notice so
// the UI ships truthfully.
// TODO(backend T1x): wire POST /api/v1/auth/forgot (ADR-010 §9 + ADR-011).

import { Trans } from '@lingui/macro';
import { Link } from 'react-router-dom';
import { ErrorBanner } from '../../components/feedback/ErrorBanner';
import { FormField } from '../../components/forms/FormField';

export default function PasswordResetRequestPage(): JSX.Element {
  return (
    <section className="auth-card" aria-labelledby="pr-req-title">
      <h1 id="pr-req-title" className="auth-card__title">
        <Trans>Restablecer contraseña</Trans>
      </h1>
      <p className="auth-card__sub">
        <Trans>
          Introduce el correo con el que te registraste. Si la dirección existe en Orbit,
          recibirás un enlace de un solo uso válido durante 60 minutos.
        </Trans>
      </p>

      <ErrorBanner variant="info" title={<Trans>Aún no disponible</Trans>}>
        <Trans>
          El restablecimiento de contraseña se habilitará en una iteración posterior. Mientras
          tanto, contacta con soporte si necesitas recuperar el acceso.
        </Trans>
      </ErrorBanner>

      <form className="stack gap-4" noValidate>
        <FormField label={<Trans>Correo electrónico</Trans>}>
          {({ inputId }) => (
            <input id={inputId} type="email" autoComplete="email" className="input" disabled />
          )}
        </FormField>
        <div className="row row--between">
          <Link className="back-link" to="/signin">
            ← <Trans>Volver</Trans>
          </Link>
          <button className="btn btn--primary" type="button" disabled>
            <Trans>Enviar enlace</Trans>
          </button>
        </div>
      </form>
    </section>
  );
}
