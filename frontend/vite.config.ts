/// <reference types="vitest" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { lingui } from '@lingui/vite-plugin';

// Slice 0a security response headers (S0-13).
//
// The *production* policy is strict — no 'unsafe-inline', no 'unsafe-eval'
// (SEC-180, ADR-009 §Strict CSP compatibility). Emitted on `vite preview`
// and in the Caddy config that ships in Slice 8.
//
// Vite's **dev** transformer injects an inline React-Refresh preamble plus
// eval'd HMR glue, which a strict CSP blocks. The dev-only policy relaxes
// `script-src` to allow inline + eval and widens `connect-src` to cover
// the HMR websocket. Nothing from the dev policy ships to production —
// `command === 'serve'` gates the relaxation.
//
// HSTS is declared for parity with the deploy-time Caddy config but is
// only observable over HTTPS; per ADR-015, HSTS is verified end-to-end
// at Slice 8.
const baseSecurityHeaders = {
  'Strict-Transport-Security': 'max-age=63072000; includeSubDomains',
  'X-Content-Type-Options': 'nosniff',
  'X-Frame-Options': 'DENY',
  'Referrer-Policy': 'strict-origin-when-cross-origin',
  'Permissions-Policy': 'camera=(), microphone=(), geolocation=()',
  'Cross-Origin-Opener-Policy': 'same-origin',
} satisfies Record<string, string>;

const prodCsp = [
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
].join('; ');

const devCsp = [
  "default-src 'self'",
  // Vite dev injects inline scripts (React-Refresh preamble + HMR client).
  "script-src 'self' 'unsafe-inline' 'unsafe-eval'",
  // Vite dev injects inline <style> tags for HMR CSS updates.
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "font-src 'self'",
  // Vite HMR runs over a websocket on the same host:port as the dev server.
  "connect-src 'self' ws://127.0.0.1:5173 ws://localhost:5173",
  "object-src 'none'",
  "base-uri 'self'",
  "frame-ancestors 'none'",
  "form-action 'self'",
].join('; ');

const prodSecurityHeaders: Record<string, string> = {
  ...baseSecurityHeaders,
  'Content-Security-Policy': prodCsp,
};

const devSecurityHeaders: Record<string, string> = {
  ...baseSecurityHeaders,
  'Content-Security-Policy': devCsp,
};

export default defineConfig(({ command }) => ({
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
    // `command === 'serve'` only; preview uses prod headers.
    headers: command === 'serve' ? devSecurityHeaders : prodSecurityHeaders,
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
    headers: prodSecurityHeaders,
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
}));
