//! Dashboard endpoints (Slice 2 T21, ADR-016 §4).
//!
//! `GET /api/v1/dashboard/stacked` returns the multi-grant stacked
//! cumulative view: per-employer curves + a combined envelope. The
//! computation is delegated to `orbit_core::stacked_grants::stack_dashboard`,
//! which is pure + deterministic and backed by the shared fixture
//! `stacked_grants_cases.json`. The handler's job is just to load grants
//! and their `vesting_events` rows, shape them into
//! `(GrantMeta, Vec<VestingEvent>)` pairs, and serialize the result.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use orbit_core::stacked_grants::{stack_dashboard, GrantMeta};
use orbit_core::vesting::VestingEvent;
use serde_json::json;

use crate::error::AppError;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

/// `GET /api/v1/dashboard/stacked`
pub async fn stacked(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;

    let grants = orbit_db::grants::list_grants(&mut tx, auth.user_id).await?;

    // One `list_for_grant` per grant. Slice-2 dashboards are ≤20 grants
    // per spec §7.8; the per-grant scan uses the
    // (grant_id, vest_date) index already on `vesting_events`. A future
    // slice may batch this into a single `WHERE grant_id = ANY($1)` query
    // if profiling says so — not a gate today.
    let mut inputs: Vec<(GrantMeta, Vec<VestingEvent>)> = Vec::with_capacity(grants.len());
    for g in grants.iter() {
        let rows = orbit_db::vesting_events::list_for_grant(&mut tx, auth.user_id, g.id).await?;
        let events: Vec<VestingEvent> = rows
            .iter()
            .map(|r| VestingEvent {
                vest_date: r.vest_date,
                shares_vested_this_event: r.shares_vested_this_event,
                cumulative_shares_vested: r.cumulative_shares_vested,
                state: r.state,
            })
            .collect();
        inputs.push((
            GrantMeta {
                id: g.id,
                employer_name: g.employer_name.clone(),
                instrument: g.instrument.clone(),
                created_at: g.created_at,
            },
            events,
        ));
    }

    tx.commit().await?;

    let dashboard = stack_dashboard(inputs);

    Ok((
        StatusCode::OK,
        Json(json!({
            "byEmployer": dashboard.by_employer,
            "combined": dashboard.combined,
        })),
    )
        .into_response())
}
