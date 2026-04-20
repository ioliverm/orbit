//! Infrastructure health + readiness endpoints.
//!
//! Per ADR-010 these are **not** under `/api/v1`. `/healthz` is cheap
//! (process-up) and returns 204. `/readyz` confirms DB reachability with a
//! single `SELECT 1` and returns 204 / 503.

use axum::extract::State;
use axum::http::StatusCode;

use crate::state::AppState;

pub async fn healthz() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub async fn readyz(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
