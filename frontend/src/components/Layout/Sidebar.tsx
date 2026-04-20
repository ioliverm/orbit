import { Trans } from '@lingui/macro';
import { NavLink } from 'react-router-dom';

// Sidebar IA mirrors dashboard-slice-1.html (UX §3.1). Entries that are
// deferred to later slices render as `próx.` pills routed to
// /app/proximamente — no [paid] badges (v1.2 PoC dropped the paid tier),
// no blurred previews (D-9 scrapped).
//
// The Portfolio > Dashboard entry is the only live destination in T14a;
// Grants detail + first-grant are wired in T14b.

interface SoonLinkProps {
  to: string;
  label: React.ReactNode;
}
function SoonLink({ to, label }: SoonLinkProps): JSX.Element {
  return (
    <NavLink className="sidebar__link" to={to}>
      <span>{label}</span>
      <span className="sidebar__pill">
        <Trans>próx.</Trans>
      </span>
    </NavLink>
  );
}

function liveNavClass({ isActive }: { isActive: boolean }): string {
  return isActive ? 'sidebar__link sidebar__link--active' : 'sidebar__link';
}

export function Sidebar(): JSX.Element {
  return (
    <aside className="sidebar" aria-label="Navegación principal">
      <div className="sidebar__brand">
        ORB<span>IT</span>
      </div>

      <nav className="sidebar__group" aria-label="Cartera">
        <div className="sidebar__heading">
          <Trans>Cartera</Trans>
        </div>
        <NavLink className={liveNavClass} to="/app/dashboard">
          <Trans>Dashboard</Trans>
        </NavLink>
        <NavLink className={liveNavClass} to="/app/grants">
          <Trans>Grants</Trans>
        </NavLink>
      </nav>

      <nav className="sidebar__group" aria-label="Decisiones">
        <div className="sidebar__heading">
          <Trans>Decisiones</Trans>
        </div>
        <SoonLink to="/app/proximamente?feature=sell-now" label={<Trans>Sell-now</Trans>} />
        <SoonLink to="/app/proximamente?feature=scenarios" label={<Trans>Escenarios</Trans>} />
      </nav>

      <nav className="sidebar__group" aria-label="Cumplimiento">
        <div className="sidebar__heading">
          <Trans>Cumplimiento</Trans>
        </div>
        <SoonLink to="/app/proximamente?feature=modelo-720" label={<Trans>Modelo 720</Trans>} />
        <SoonLink to="/app/proximamente?feature=exports" label={<Trans>Exports</Trans>} />
      </nav>

      <nav className="sidebar__group mt-auto" aria-label="Cuenta">
        <div className="sidebar__heading">
          <Trans>Cuenta</Trans>
        </div>
        <NavLink className={liveNavClass} to="/app/account/profile">
          <Trans>Perfil y residencia</Trans>
        </NavLink>
        <SoonLink to="/app/proximamente?feature=privacy" label={<Trans>Datos y privacidad</Trans>} />
      </nav>
    </aside>
  );
}
