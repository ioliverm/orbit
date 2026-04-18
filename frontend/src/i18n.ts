import { i18n } from '@lingui/core';
import { messages as esMessages } from './locales/es-ES/messages.po';
import { messages as enMessages } from './locales/en/messages.po';

// ES-primary, EN-fallback per ADR-009 §Locale handling.
export const SUPPORTED_LOCALES = ['es-ES', 'en'] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];
export const DEFAULT_LOCALE: Locale = 'es-ES';

i18n.load({
  'es-ES': esMessages,
  en: enMessages,
});

function resolveInitialLocale(): Locale {
  // localStorage key per ADR-009 (not a cookie — avoids AEPD cookie-banner issue at Slice 0).
  const stored =
    typeof localStorage !== 'undefined'
      ? localStorage.getItem('orbit.locale')
      : null;
  if (stored && (SUPPORTED_LOCALES as readonly string[]).includes(stored)) {
    return stored as Locale;
  }
  // Honor Accept-Language on first load: if the browser prefers English, use it.
  if (typeof navigator !== 'undefined') {
    const pref = navigator.language?.toLowerCase() ?? '';
    if (pref.startsWith('en')) return 'en';
  }
  return DEFAULT_LOCALE;
}

export function activateLocale(locale: Locale): void {
  i18n.activate(locale);
}

activateLocale(resolveInitialLocale());

export { i18n };
