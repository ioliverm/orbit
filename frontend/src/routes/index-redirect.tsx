// `/` — decides where to send the user based on the auth snapshot.
//   - not initialized → render a spinner; <RootLayout/> is loading /auth/me
//   - no user → /signin
//   - user + stage → stage path (disclaimer / residency / first-grant / dashboard)

import { Navigate } from 'react-router-dom';
import { Spinner } from '../components/Spinner';
import { stagePath } from '../hooks/useOnboardingGate';
import { useAuthStore } from '../store/auth';

export default function IndexRedirect(): JSX.Element {
  const { initialized, user, onboardingStage } = useAuthStore();
  if (!initialized) {
    return (
      <div className="auth-shell">
        <main className="auth-shell__main">
          <Spinner />
        </main>
      </div>
    );
  }
  if (!user) return <Navigate to="/signin" replace />;
  if (onboardingStage) return <Navigate to={stagePath(onboardingStage)} replace />;
  return <Navigate to="/app/dashboard" replace />;
}
