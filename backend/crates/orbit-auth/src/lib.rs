//! Auth primitives for Orbit. Slice 0a (T7).
//!
//! This crate ships **only** the cryptographic / cookie-shape primitives
//! needed for S0-21 and S0-22. It does **not** wire HTTP handlers, touch
//! Postgres, or know anything about `axum`; that integration lives in
//! `orbit-api` in Slice 1.
//!
//! Modules:
//!
//!   * [`password`] — argon2id hash / verify pinned to OWASP-2024 params
//!     (SEC-001).
//!   * [`session`] — opaque session + CSRF token factories, the
//!     `HttpOnly; Secure; SameSite=Lax` cookie factory, and the CSRF
//!     double-submit verifier (SEC-006, SEC-188, SEC-189).
//!
//! MFA / TOTP is intentionally absent: ADR-011 defers it to Slice 7, and
//! Slice 1 returns 501 on the MFA endpoints.

pub mod password;
pub mod session;
