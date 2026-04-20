//! Residency-domain support modules. Slice 1 T13b.
//!
//! The canonical autonomía list (ADR-014 Alternatives item "Hardcoding the
//! autonomía list client-side" — server-authoritative) lives in
//! [`autonomias`]. The HTTP handler that serves it + the residency POST/GET
//! live in `crate::handlers::residency`.

pub mod autonomias;
