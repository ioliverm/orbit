// Auth store (ADR-009 §State-management taxonomy).
//
// This is a *thin* cache over /auth/me. TanStack Query is the owner for
// real server state; we mirror just enough into Zustand so UI surfaces
// outside the auth route tree (sidebar greeting, onboarding redirects,
// disclaimer modal gate) can read without drilling props.

import { create } from 'zustand';
import type {
  MeResidencySummary,
  MeResponse,
  MeUser,
  OnboardingStage,
} from '../api/auth';

export interface AuthSnapshot {
  user: MeUser | null;
  residency: MeResidencySummary | null;
  onboardingStage: OnboardingStage | null;
  disclaimerAccepted: boolean;
  /** True while the initial /auth/me load is in flight. */
  loading: boolean;
  /** Set once the first /auth/me load has either resolved or rejected. */
  initialized: boolean;
}

interface AuthActions {
  setFromMe: (me: MeResponse) => void;
  setLoading: (loading: boolean) => void;
  setInitialized: (initialized: boolean) => void;
  setOnboardingStage: (stage: OnboardingStage) => void;
  setDisclaimerAccepted: (accepted: boolean) => void;
  clear: () => void;
}

export type AuthStore = AuthSnapshot & AuthActions;

const EMPTY: AuthSnapshot = {
  user: null,
  residency: null,
  onboardingStage: null,
  disclaimerAccepted: false,
  loading: false,
  initialized: false,
};

export const useAuthStore = create<AuthStore>((set) => ({
  ...EMPTY,
  setFromMe: (me) =>
    set({
      user: me.user,
      residency: me.residency,
      onboardingStage: me.onboardingStage,
      disclaimerAccepted: me.disclaimerAccepted,
      loading: false,
      initialized: true,
    }),
  setLoading: (loading) => set({ loading }),
  setInitialized: (initialized) => set({ initialized }),
  setOnboardingStage: (onboardingStage) => set({ onboardingStage }),
  setDisclaimerAccepted: (disclaimerAccepted) => set({ disclaimerAccepted }),
  clear: () => set({ ...EMPTY, initialized: true }),
}));
