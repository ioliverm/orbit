//! Rule-set chip endpoint (Slice 3 T29, ADR-017 §3).
//!
//! `GET /api/v1/rule-set-chip` — returns the ECB date the footer chip
//! should display plus the Orbit engine version. Per AC-7.1.6 the chip
//! in Slice 3 never surfaces a tax rule-set version — that ships in
//! Slice 4 when the calculator lands.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use serde_json::json;

use crate::error::AppError;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

/// `GET /api/v1/rule-set-chip`
pub async fn get(
    State(state): State<AppState>,
    Extension(_auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let today = Utc::now().date_naive();
    let fx = orbit_db::fx_rates::lookup_walkback(&state.pool, "EUR", "USD", today, 7).await?;

    let (fx_date, staleness_days) = match fx {
        Some(r) => (
            Some(r.rate_date.format("%Y-%m-%d").to_string()),
            Some(r.walkback_days),
        ),
        None => (None, None),
    };

    Ok((
        StatusCode::OK,
        Json(json!({
            "fxDate": fx_date,
            "stalenessDays": staleness_days,
            "engineVersion": env!("CARGO_PKG_VERSION"),
        })),
    )
        .into_response())
}
