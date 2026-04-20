//! Onboarding-gate middleware (ADR-014 §3 / AC G-8).
//!
//! Applied to `/api/v1/` routes that sit **past** a wizard step. The
//! router wires the layer onto the gated subtree directly, so this
//! middleware does not carry a per-path allowlist.
//!
//! # Stage ladder
//!
//! ```text
//! disclaimer  → residency  → first_grant  → complete
//!     0            1              2             3
//! ```
//!
//! Each route declares the minimum stage a user must have reached before
//! it accepts their request. The `/grants*` subtree requires `first_grant`
//! (index 2): a user can POST their first grant from the `first_grant`
//! stage, and once committed they are bumped to `complete` where reads /
//! updates / deletes continue to pass. A user stuck at `disclaimer` or
//! `residency` is 403'd with `onboarding.required` and the stage the SPA
//! should resume at.
//!
//! Cost: one SELECT on `users` + one via `current_period`. Cheap enough
//! for Slice 1; cache per-session in Slice 2 if profiling says so.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::error::AppError;
use crate::handlers::residency;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

/// A user's current onboarding ladder position. Numeric ordering is
/// load-bearing: `rank(required) <= rank(user_stage)` means the user may
/// proceed past the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StageRank {
    Disclaimer = 0,
    Residency = 1,
    FirstGrant = 2,
    Complete = 3,
}

impl StageRank {
    fn from_str(s: &str) -> Self {
        match s {
            "disclaimer" => StageRank::Disclaimer,
            "residency" => StageRank::Residency,
            "first_grant" => StageRank::FirstGrant,
            _ => StageRank::Complete,
        }
    }
}

/// Require at least `first_grant` — the `/grants*` subtree. A user at
/// `first_grant` can POST to create their first grant; once a grant
/// exists the stage advances to `complete` and all grant reads / writes
/// continue to pass.
pub async fn require_first_grant_or_later(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    gate(&state, req, next, StageRank::FirstGrant).await
}

async fn gate(
    state: &AppState,
    req: Request<Body>,
    next: Next,
    required: StageRank,
) -> Result<Response, AppError> {
    let auth = *req
        .extensions()
        .get::<SessionAuth>()
        .ok_or(AppError::Unauthenticated)?;
    let stage = residency::resolve_stage(&state.pool, auth.user_id).await?;
    if StageRank::from_str(stage) < required {
        return Err(AppError::OnboardingRequired { stage });
    }
    Ok(next.run(req).await)
}
