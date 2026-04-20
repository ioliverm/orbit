// /app/account/profile — T14b/T15 stub.
// Residency edit (AC-4.1.7) + Data & privacy placeholders land in later
// iterations. For T14a we render a skeleton so the sidebar entry works.

import { Trans } from '@lingui/macro';

export default function ProfilePage(): JSX.Element {
  return (
    <>
      <div className="page-title">
        <div>
          <h1>
            <Trans>Perfil y residencia</Trans>
          </h1>
          <p className="page-title__meta">
            <Trans>Gestión de datos disponible a partir de la siguiente iteración.</Trans>
          </p>
        </div>
      </div>
      <section className="account-panel" aria-label="Perfil">
        <p className="muted">
          <Trans>El formulario de residencia y la sección de datos y privacidad llegan en T14b.</Trans>
        </p>
      </section>
    </>
  );
}
