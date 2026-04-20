import { Trans } from '@lingui/macro';
import { Outlet } from 'react-router-dom';
import { Footer } from './Footer';
import { Header } from './Header';
import { Sidebar } from './Sidebar';

interface Props {
  title?: React.ReactNode;
}

// Authenticated shell: sidebar + main + footer (G-1, G-17).
// Auth routes (/signin, /signup, /password-reset) use <AuthShell/> instead.
export function AppShell({ title }: Props): JSX.Element {
  return (
    <div className="app">
      <a className="skip" href="#main">
        <Trans>Saltar al contenido</Trans>
      </a>
      <Sidebar />
      <div className="main">
        <Header title={title} />
        <main id="main" className="content">
          <Outlet />
        </main>
        <Footer />
      </div>
    </div>
  );
}
