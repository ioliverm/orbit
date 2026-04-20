//! HTTP middleware for orbit-api.
//!
//! The middleware stack, from outer to inner (request ingress order):
//!
//! 1. `TraceLayer` — request/response spans.
//! 2. `SetResponseHeader` (security headers per SEC-180..SEC-186).
//! 3. `RequestIdLayer` — injects `X-Request-Id` on the response.
//! 4. `TimeoutLayer` — 30 s wall-clock cap.
//! 5. `RequestBodyLimit` — 128 KiB default.
//! 6. `CorsLayer` — explicit origin + credentials per SEC-187.
//! 7. Session lookup — per-route in the handler module, not a global layer
//!    (`/healthz`, `/readyz`, `/auth/signup`, `/auth/signin`,
//!    `/auth/verify-email` skip it).
//! 8. CSRF double-submit — likewise per-route.
//! 9. Rate limit — per-route.

pub mod csrf;
pub mod onboarding;
pub mod rate_limit;
pub mod request_id;
pub mod security_headers;
pub mod session;

pub use session::SessionAuth;
