import { Trans } from '@lingui/macro';
import { Link, Outlet } from 'react-router-dom';
import { Footer } from './Footer';
import { LocaleSwitcher } from './LocaleSwitcher';

interface Props {
  /** Secondary CTA in the topbar (e.g. "Iniciar sesión" on /signup). */
  secondaryCta?: { to: string; label: React.ReactNode };
}

// Auth shell for /signup, /signin, /password-reset. No sidebar.
// Matches signup.html + signin.html structure exactly.
export function AuthShell({ secondaryCta }: Props): JSX.Element {
  return (
    <div className="auth-shell">
      <a className="skip" href="#main">
        <Trans>Saltar al contenido</Trans>
      </a>
      <header className="auth-shell__topbar">
        <div className="auth-shell__brand">
          ORB<span>IT</span>
        </div>
        <div className="row gap-2">
          <LocaleSwitcher />
          {secondaryCta ? (
            <Link className="btn btn--secondary btn--sm" to={secondaryCta.to}>
              {secondaryCta.label}
            </Link>
          ) : null}
        </div>
      </header>
      <main id="main" className="auth-shell__main">
        <div className="auth-stack">
          <Outlet />
        </div>
      </main>
      <Footer />
    </div>
  );
}
