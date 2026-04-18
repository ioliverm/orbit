//! `Money` stub for Slice 0a.
//!
//! Forbidden in any log, event payload, crash report, or error response body.
//! Does NOT implement `orbit_log::SafeToLog`; attempting `event!(m = money)` is
//! a compile error. (SEC-050)

/// Monetary amount. Real fields land in Slice 0b.
///
/// `Debug` is permitted for internal dev / panics; it is NOT a log-safe type
/// per SEC-050 and there is no path to emit a `Money` through `orbit_log`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money;
