// Locale store. The user's choice persists in `localStorage` per ADR-009
// (avoids the AEPD cookie-banner concern at Slice 0). We also set a
// short-lived `orbit_locale` cookie so the backend can honour the locale
// on outbound email (G-12 follow-up: backend reads on signup if present).

import { create } from 'zustand';
import type { Locale } from '../i18n';
import { activateLocale, DEFAULT_LOCALE, SUPPORTED_LOCALES, i18n } from '../i18n';

const STORAGE_KEY = 'orbit.locale';
const COOKIE_NAME = 'orbit_locale';
const COOKIE_MAX_AGE_SECS = 60 * 60 * 24 * 365; // 1 year

interface LocaleState {
  locale: Locale;
  setLocale: (locale: Locale) => void;
}

function persist(locale: Locale): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    /* storage unavailable — tolerate */
  }
  if (typeof document !== 'undefined') {
    document.cookie = `${COOKIE_NAME}=${encodeURIComponent(locale)}; Path=/; Max-Age=${COOKIE_MAX_AGE_SECS}; SameSite=Lax`;
  }
}

function initialLocale(): Locale {
  // i18n.ts already resolved one on module load (localStorage + navigator).
  // Mirror whatever is active — this keeps store + i18n in lock-step.
  const active = i18n.locale as Locale | undefined;
  if (active && (SUPPORTED_LOCALES as readonly string[]).includes(active)) {
    return active;
  }
  return DEFAULT_LOCALE;
}

export const useLocaleStore = create<LocaleState>((set) => ({
  locale: initialLocale(),
  setLocale: (locale) => {
    activateLocale(locale);
    persist(locale);
    if (typeof document !== 'undefined') {
      document.documentElement.lang = locale === 'es-ES' ? 'es' : 'en';
    }
    set({ locale });
  },
}));
