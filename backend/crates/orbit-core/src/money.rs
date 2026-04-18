//! `Money` stub for Slice 0a.
//!
//! Forbidden in any log, event payload, crash report, or error response body.
//! Does NOT implement `orbit_log::SafeToLog`; attempting `event!(m = money)` is
//! a compile error. (SEC-050)

use std::fmt;

/// Monetary amount. Real fields land in Slice 1+.
///
/// `Debug` is a manual redacted impl so `format!("{m:?}")`, `tracing::event!(?m)`,
/// `panic!("{m:?}")`, and `dbg!(m)` cannot leak contents. `orbit_log::event!` is
/// separately blocked at compile time via the sealed `SafeToLog` trait. (SEC-050)
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Money;

impl fmt::Debug for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Money(<redacted>)")
    }
}
