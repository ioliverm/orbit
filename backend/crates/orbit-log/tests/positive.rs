//! Smoke tests for the positive path of `event!`.
//!
//! Every call in this file MUST compile and succeed at runtime; the
//! compile-fail fixtures in `tests/compile_fail/` cover the inverse.

use orbit_log::{event, Level, SafeString};

#[test]
fn macro_accepts_allowlisted_field_types() {
    let tag: &'static str = "startup";
    let owned = SafeString::new("example".to_string());
    event!(Level::Info, "booted",
        request_id = 42u128,
        count = 7u64,
        ok = true,
        tag = tag,
        note = owned,
    );
}

#[test]
fn macro_accepts_no_fields() {
    event!(Level::Warn, "deprecated_call_observed");
}
