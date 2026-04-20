// Onboarding-gate hook (ADR-014 §3, AC G-8).
//
// Reads the current auth snapshot and redirects the user to the step that
// matches their `onboardingStage`. Call this from any /app/* route; the
// auth store is hydrated by <RouterBootstrap/> at the root.

import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import type { OnboardingStage } from '../api/auth';
import { useAuthStore } from '../store/auth';

const STAGE_TO_PATH: Record<OnboardingStage, string> = {
  disclaimer: '/app/disclaimer',
  residency: '/app/onboarding/residency',
  first_grant: '/app/onboarding/first-grant',
  complete: '/app/dashboard',
};

export function stagePath(stage: OnboardingStage): string {
  return STAGE_TO_PATH[stage];
}

/**
 * Redirects to the correct step if the user isn't already there. Pass the
 * "expected" stage for the current route. If the user is ahead of the
 * expected stage, send them to wherever they belong.
 */
export function useOnboardingGate(expected: OnboardingStage | 'signed_in'): void {
  const navigate = useNavigate();
  const { initialized, user, onboardingStage } = useAuthStore();

  useEffect(() => {
    if (!initialized) return;
    if (!user) {
      navigate('/signin', { replace: true });
      return;
    }
    if (expected === 'signed_in') return;
    if (onboardingStage && onboardingStage !== expected) {
      navigate(STAGE_TO_PATH[onboardingStage], { replace: true });
    }
  }, [initialized, user, onboardingStage, expected, navigate]);
}
