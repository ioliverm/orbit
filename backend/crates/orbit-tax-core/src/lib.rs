//! Crate scaffold. Slice 0a.
//!
//! Forbidden: `std::collections::HashMap`. Use `BTreeMap` or `IndexMap`. (SEC-085)
//!
//! Sensitive-type stubs live here and do **not** implement
//! `orbit_log::SafeToLog`; attempting `event!(field = grant)` is a compile
//! error. (SEC-050)

/// Equity grant. Real fields land in Slice 1.
#[derive(Debug, Clone)]
pub struct Grant;

/// Completed tax calculation. Real fields land in Slice 2.
#[derive(Debug, Clone)]
pub struct Calculation;

/// Input payload for the sell-now preview. Real fields land in Slice 2.
#[derive(Debug, Clone)]
pub struct SellNowInput;

/// User-authored what-if scenario. Real fields land in Slice 2.
#[derive(Debug, Clone)]
pub struct Scenario;

/// Generated export artefact (PDF / CSV). Real fields land in Slice 4.
#[derive(Debug, Clone)]
pub struct Export;
