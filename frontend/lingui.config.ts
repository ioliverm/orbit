import type { LinguiConfig } from '@lingui/conf';

// ES-primary, EN-fallback per ADR-009 §Locale handling.
// PO format, catalogs colocated with source under src/locales/<locale>/messages.po.
const config: LinguiConfig = {
  locales: ['es-ES', 'en'],
  sourceLocale: 'es-ES',
  fallbackLocales: {
    default: 'es-ES',
  },
  catalogs: [
    {
      path: '<rootDir>/src/locales/{locale}/messages',
      include: ['src'],
    },
  ],
  format: 'po',
  compileNamespace: 'es',
};

export default config;
