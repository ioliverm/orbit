//! HTTP layer for Orbit. Slice 1 T13a — scaffold + auth handlers.
//!
//! The crate entry point is [`router`], which consumes an [`AppState`] and
//! returns a fully-wired axum `Router`. The single binary (`orbit api`)
//! serves it on `APP_BIND_ADDR`.
//!
//! Module layout:
//!
//!   * [`router`] — router assembly + middleware stack.
//!   * [`state`]  — `AppState` struct.
//!   * [`error`]  — unified error envelope (ADR-010 §7, SEC-051).
//!   * [`middleware`] — request-id, security headers, session, CSRF,
//!     rate limit.
//!   * [`handlers`] — per-endpoint handler functions (`auth`, `health`).
//!   * [`audit`]  — typed audit_log writer (SEC-100..SEC-103).
//!   * [`hibp`]   — HIBP k-anonymity breached-password check (SEC-002).

pub mod audit;
pub mod error;
pub mod handlers;
pub mod hibp;
pub mod middleware;
pub mod router;
pub mod state;

pub use router::router;
pub use state::AppState;
