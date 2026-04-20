// Vitest bootstrap: matchers + cleanup between tests.
// Loaded via vite.config.ts > test.setupFiles.
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, beforeEach } from 'vitest';
import { activateLocale } from '../i18n';

beforeEach(() => {
  // Force ES for string-matching stability; per-test may switch explicitly.
  activateLocale('es-ES');
});

afterEach(() => {
  cleanup();
});
