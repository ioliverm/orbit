// /signup/verify-sent — STATE 2 of the signup wizard.
// "Check your email" screen. Shown after POST /auth/signup succeeds,
// regardless of whether the email was new or an existing account (SEC-003).

import { Trans } from '@lingui/macro';
import { Link, useLocation } from 'react-router-dom';

interface LocationState {
  email?: string;
}

export default function SignupVerifySentPage(): JSX.Element {
  const loc = useLocation();
  const state = (loc.state ?? {}) as LocationState;
  const email = state.email ?? '';

  return (
    <section className="auth-card auth-card--wide" aria-labelledby="verify-sent-title">
      <h1 id="verify-sent-title" className="auth-card__title">
        <Trans>Revisa tu correo</Trans>
      </h1>
      <p className="auth-card__sub">
        {email ? (
          <Trans>Hemos enviado un enlace a {email}. El enlace caduca en 24 horas.</Trans>
        ) : (
          <Trans>Hemos enviado un enlace a tu correo. El enlace caduca en 24 horas.</Trans>
        )}
      </p>
      <div className="auth-card__footer">
        <Trans>
          Si no recibiste el correo, comprueba la carpeta de spam. No puedes entrar en la app hasta
          verificar tu email.
        </Trans>
      </div>
      <div className="row row--between">
        <Link className="back-link" to="/signin">
          ← <Trans>Volver a iniciar sesión</Trans>
        </Link>
      </div>
    </section>
  );
}
