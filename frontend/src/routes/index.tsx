// Central route table. Keeps main.tsx short and lets tests mount the same
// set under a MemoryRouter for integration-style vitest coverage.

import type { RouteObject } from 'react-router-dom';
import { AppShell } from '../components/Layout/AppShell';
import { AuthShell } from '../components/Layout/AuthShell';
import IndexRedirect from './index-redirect';
import { RootLayout } from './root';

import SignupPage from './auth/signup';
import SignupVerifySentPage from './auth/signup-verify-sent';
import VerifyEmailPage from './auth/verify-email';
import SigninPage from './auth/signin';
import PasswordResetRequestPage from './auth/password-reset-request';
import PasswordResetLandingPage from './auth/password-reset-landing';

import DisclaimerRoute from './onboarding/disclaimer';
import ResidencyStepPage from './onboarding/residency';
import FirstGrantStepPage from './onboarding/first-grant';

import DashboardPage from './app/dashboard';
import GrantsIndexPage from './app/grants';
import GrantDetailPage from './app/grants/detail';
import AddGrantPage from './app/grants/new';
import ProximamenteRoute from './app/proximamente';
import ProfilePage from './account/profile';

export function buildRoutes(): RouteObject[] {
  return [
    {
      path: '/',
      element: <RootLayout />,
      children: [
        { index: true, element: <IndexRedirect /> },

        // Auth branch — uses the no-sidebar shell.
        {
          element: <AuthShell secondaryCta={{ to: '/signin', label: 'Iniciar sesión' }} />,
          children: [
            { path: 'signup', element: <SignupPage /> },
            { path: 'signup/verify-sent', element: <SignupVerifySentPage /> },
            { path: 'verify-email', element: <VerifyEmailPage /> },
            { path: 'password-reset/request', element: <PasswordResetRequestPage /> },
            { path: 'password-reset', element: <PasswordResetLandingPage /> },
          ],
        },
        {
          element: <AuthShell secondaryCta={{ to: '/signup', label: 'Crear cuenta' }} />,
          children: [{ path: 'signin', element: <SigninPage /> }],
        },

        // Onboarding branch (pre-dashboard) — no sidebar yet.
        {
          element: <AuthShell />,
          children: [
            { path: 'app/disclaimer', element: <DisclaimerRoute /> },
            { path: 'app/onboarding/residency', element: <ResidencyStepPage /> },
            { path: 'app/onboarding/first-grant', element: <FirstGrantStepPage /> },
          ],
        },

        // Authenticated shell — full sidebar + header + footer.
        {
          path: 'app',
          element: <AppShell />,
          children: [
            { path: 'dashboard', element: <DashboardPage /> },
            { path: 'grants', element: <GrantsIndexPage /> },
            { path: 'grants/new', element: <AddGrantPage /> },
            { path: 'grants/:grantId', element: <GrantDetailPage /> },
            { path: 'proximamente', element: <ProximamenteRoute /> },
            { path: 'account/profile', element: <ProfilePage /> },
          ],
        },
      ],
    },
  ];
}
