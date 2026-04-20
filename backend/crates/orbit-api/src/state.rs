//! Application state shared across every handler.
//!
//! `AppState` holds the Postgres pool, the HMAC key used to hash IPs in
//! audit rows (SEC-054), the cookie-security toggle (dev `false`, Slice 8
//! `true`), and the `reqwest` client used for the HIBP outbound call
//! (ADR-011, SEC-002). Everything is cheap to clone — the pool and client
//! are internally reference-counted.

use std::sync::Arc;

use sqlx::PgPool;

/// Configuration and long-lived dependencies injected into every handler.
#[derive(Clone)]
pub struct AppState {
    /// Postgres pool. Handlers never use this directly — always via
    /// `orbit_db::Tx::for_user`.
    pub pool: PgPool,
    /// HMAC-SHA256 key for `ip_hash` (SEC-054). 32 bytes, from
    /// `APP_IP_HASH_KEY_HEX`.
    pub ip_hash_key: Arc<[u8; 32]>,
    /// Set on `Set-Cookie`: `true` in prod (Slice 8+), `false` in dev so
    /// cookies flow over `http://localhost:*`. See `APP_COOKIE_SECURE`.
    pub cookie_secure: bool,
    /// Same-origin frontend origin for CORS (`APP_CORS_ORIGIN`). In dev:
    /// `http://localhost:5173`.
    pub cors_origin: String,
    /// Shared reqwest client for outbound calls (HIBP k-anonymity).
    /// 500 ms total timeout per SEC-003 fail-closed posture.
    pub http: reqwest::Client,
}
