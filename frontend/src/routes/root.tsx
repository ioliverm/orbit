// Root layout — bootstraps auth, wires the error-sinks on the API client,
// and renders the child route via <Outlet/>. Every route in the SPA hangs
// off this one.

import { useCallback, useEffect, useRef } from 'react';
import { Outlet, useLocation, useNavigate, type RouteObject } from 'react-router-dom';
import { setOnboardingRequiredSink, setUnauthenticatedSink } from '../api/client';
import { useAuthBootstrap } from '../hooks/useAuth';
import { stagePath } from '../hooks/useOnboardingGate';
import type { OnboardingStage } from '../api/auth';
import { useAuthStore } from '../store/auth';

// Routes where a 401 "unauthenticated" should NOT bounce the user to
// /signin (they're either on the auth pages already, or the /auth/me
// bootstrap probe is expected to 401 on cold boot).
const AUTH_PATHS = ['/signin', '/signup', '/verify-email', '/password-reset'];

function shouldRedirectOn401(pathname: string): boolean {
  return !AUTH_PATHS.some((p) => pathname === p || pathname.startsWith(`${p}/`));
}

function RootLayout(): JSX.Element {
  const navigate = useNavigate();
  const location = useLocation();
  const locationRef = useRef(location);
  locationRef.current = location;

  // Bootstrap /auth/me. Fires once on app boot + any time the query is
  // invalidated (e.g. after signin, after disclaimer acceptance).
  useAuthBootstrap();

  const clear = useAuthStore((s) => s.clear);
  const setOnboardingStage = useAuthStore((s) => s.setOnboardingStage);

  const onUnauthenticated = useCallback(() => {
    clear();
    if (shouldRedirectOn401(locationRef.current.pathname)) {
      navigate('/signin', { replace: true });
    }
  }, [clear, navigate]);

  const onOnboardingRequired = useCallback(
    (stage: string) => {
      const allowed: OnboardingStage[] = ['disclaimer', 'residency', 'first_grant', 'complete'];
      if (allowed.includes(stage as OnboardingStage)) {
        const s = stage as OnboardingStage;
        setOnboardingStage(s);
        navigate(stagePath(s), { replace: true });
      }
    },
    [navigate, setOnboardingStage],
  );

  useEffect(() => {
    setUnauthenticatedSink(onUnauthenticated);
    setOnboardingRequiredSink(onOnboardingRequired);
    return () => {
      setUnauthenticatedSink(null);
      setOnboardingRequiredSink(null);
    };
  }, [onUnauthenticated, onOnboardingRequired]);

  return <Outlet />;
}

// Routes are assembled in main.tsx via `buildRoutes()` so tests can mount
// the app with a MemoryRouter without duplicating the route table.
export { RootLayout };

// Kept for backwards-compat with the Slice 0a shape. main.tsx no longer
// uses this export directly; see `buildRoutes` in ./index.ts.
export const rootRoute: RouteObject = {
  path: '/',
  element: <RootLayout />,
};
