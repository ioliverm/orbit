//! `orbit-core` — domain primitives shared across backend crates.
//!
//! Slice 0a shipped the `Money` stub + log-safety redaction. Slice 1 adds
//! the [`vesting`] module: the pure vesting-derivation algorithm from
//! ADR-014 §3, re-used by `orbit-api` on grant write and by the frontend
//! for the live preview (AC-4.2.5). Re-implementations must produce
//! identical event lists for the same grant input (AC-4.3.5).

pub mod money;
pub mod paper_gains;
pub mod stacked_grants;
pub mod vesting;

pub use money::Money;
pub use paper_gains::{
    compute as compute_paper_gains, EsppPurchaseForPaperGains, EurBand, GrantForPaperGains,
    GrantPriceOverrideForPaperGains, MissingReason, PaperGainsInput, PaperGainsResult,
    PerGrantGains, TickerPriceForPaperGains, VestingEventForPaperGains,
};
pub use stacked_grants::{
    normalize_employer, stack_cumulative_for_employer, stack_dashboard, vested_to_date_at,
    EmployerStack, GrantMeta, PerGrantDelta, StackedDashboard, StackedPoint,
};
pub use vesting::{
    derive_vesting_events, vested_to_date, whole_shares, Cadence, GrantInput, Shares, VestingError,
    VestingEvent, VestingEventOverride, VestingState, SHARES_SCALE,
};
