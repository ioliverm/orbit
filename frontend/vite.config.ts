/// <reference types="vitest" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { lingui } from '@lingui/vite-plugin';

// Slice 0a security response headers (S0-13).
// Strict CSP — no 'unsafe-inline', no 'unsafe-eval' (SEC-180, ADR-009 §Strict CSP compatibility).
// HSTS is declared here for parity with the deploy-time Caddy config but is only
// observable over HTTPS; per ADR-015, HSTS is verified end-to-end at Slice 0b.
const securityHeaders: Record<string, string> = {
  'Content-Security-Policy': [
    "default-src 'self'",
    "script-src 'self'",
    "style-src 'self'",
    "img-src 'self' data:",
    "font-src 'self'",
    "connect-src 'self'",
    "object-src 'none'",
    "base-uri 'self'",
    "frame-ancestors 'none'",
    "form-action 'self'",
  ].join('; '),
  // HSTS only observable at 0b (real HTTPS); declared here for parity.
  'Strict-Transport-Security': 'max-age=63072000; includeSubDomains',
  'X-Content-Type-Options': 'nosniff',
  'X-Frame-Options': 'DENY',
  'Referrer-Policy': 'strict-origin-when-cross-origin',
  'Permissions-Policy': 'camera=(), microphone=(), geolocation=()',
  'Cross-Origin-Opener-Policy': 'same-origin',
};

export default defineConfig({
  plugins: [
    react({
      babel: {
        plugins: ['macros'],
      },
    }),
    lingui(),
  ],
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    headers: securityHeaders,
    proxy: {
      // orbit-api binds to 127.0.0.1:8080 (APP_BIND_ADDR default) per
      // backend/binaries/orbit/src/main.rs. Proxy keeps the SPA same-origin
      // (ADR-010 §2) so cookies + CSRF double-submit work without CORS in dev.
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: false,
      },
    },
  },
  preview: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    headers: securityHeaders,
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
    target: 'es2022',
    cssCodeSplit: true,
  },
  test: {
    environment: 'jsdom',
    globals: false,
    setupFiles: ['src/testing/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    exclude: ['e2e/**', 'node_modules/**', 'dist/**'],
  },
});
