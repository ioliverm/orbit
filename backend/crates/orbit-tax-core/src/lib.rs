//! Crate scaffold. Slice 0a.
//!
//! Forbidden: `std::collections::HashMap`. Use `BTreeMap` or `IndexMap`. (SEC-085)
//!
//! Sensitive-type stubs live here. None of them implements
//! `orbit_log::SafeToLog`, so `event!(field = grant)` is a compile error. Each
//! type also carries a manual redacted `Debug` impl so `format!("{g:?}")`,
//! `tracing::event!(?g)`, `panic!("{g:?}")`, and `dbg!(g)` cannot leak the
//! type's contents via the ambient `Debug` formatter either. (SEC-050)

use std::fmt;

/// Equity grant. Real fields land in Slice 1.
#[derive(Clone)]
pub struct Grant;

/// Completed tax calculation. Real fields land in Slice 2.
#[derive(Clone)]
pub struct Calculation;

/// Input payload for the sell-now preview. Real fields land in Slice 2.
#[derive(Clone)]
pub struct SellNowInput;

/// User-authored what-if scenario. Real fields land in Slice 2.
#[derive(Clone)]
pub struct Scenario;

/// Generated export artefact (PDF / CSV). Real fields land in Slice 4.
#[derive(Clone)]
pub struct Export;

macro_rules! redacted_debug {
    ($t:ty, $label:literal) => {
        impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!($label, "(<redacted>)"))
            }
        }
    };
}

redacted_debug!(Grant, "Grant");
redacted_debug!(Calculation, "Calculation");
redacted_debug!(SellNowInput, "SellNowInput");
redacted_debug!(Scenario, "Scenario");
redacted_debug!(Export, "Export");
