//! FX endpoints (Slice 3 T29, ADR-017 §3).
//!
//! Public reads of `fx_rates` — no auth, no onboarding gate (reference
//! data, same posture as `/residency/autonomias`). The `orbit_db::fx_rates`
//! helpers take a `&PgPool` directly because `fx_rates` is not RLS-scoped.
//!
//! No audit rows: these are read-only and carry no user-scoped context.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{NaiveDate, Utc};
use orbit_db::fx_rates::Staleness;
use serde::Deserialize;
use serde_json::json;

use crate::error::AppError;
use crate::state::AppState;

const MAX_WALKBACK_DAYS: u32 = 7;
const DEFAULT_QUOTE: &str = "USD";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxRateQuery {
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub on: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxLatestQuery {
    #[serde(default)]
    pub quote: Option<String>,
}

fn staleness_str(s: Staleness) -> &'static str {
    match s {
        Staleness::Fresh => "fresh",
        Staleness::Walkback => "walkback",
        Staleness::Stale => "stale",
        Staleness::Unavailable => "unavailable",
    }
}

/// `GET /api/v1/fx/rate?quote=USD&on=YYYY-MM-DD`
pub async fn get_rate(
    State(state): State<AppState>,
    Query(q): Query<FxRateQuery>,
) -> Result<Response, AppError> {
    let quote = q.quote.as_deref().unwrap_or(DEFAULT_QUOTE);
    if quote.len() != 3 || !quote.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(AppError::Validation(vec![crate::error::FieldError {
            field: "quote".into(),
            code: "format".into(),
        }]));
    }
    let on = match q.on.as_deref() {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
            AppError::Validation(vec![crate::error::FieldError {
                field: "on".into(),
                code: "format".into(),
            }])
        })?,
        None => Utc::now().date_naive(),
    };

    let result =
        orbit_db::fx_rates::lookup_walkback(&state.pool, "EUR", quote, on, MAX_WALKBACK_DAYS)
            .await?;

    let body = match result {
        Some(r) => json!({
            "quote": quote,
            "rateDate": r.rate_date.format("%Y-%m-%d").to_string(),
            "rate": r.rate,
            "walkback": r.walkback_days,
            "staleness": staleness_str(r.staleness),
        }),
        None => json!({
            "quote": quote,
            "rateDate": null,
            "rate": null,
            "walkback": null,
            "staleness": "unavailable",
        }),
    };

    Ok((StatusCode::OK, Json(body)).into_response())
}

/// `GET /api/v1/fx/latest?quote=USD`
pub async fn get_latest(
    State(state): State<AppState>,
    Query(q): Query<FxLatestQuery>,
) -> Result<Response, AppError> {
    let quote = q.quote.as_deref().unwrap_or(DEFAULT_QUOTE);
    if quote.len() != 3 || !quote.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(AppError::Validation(vec![crate::error::FieldError {
            field: "quote".into(),
            code: "format".into(),
        }]));
    }

    let row = orbit_db::fx_rates::latest(&state.pool, "EUR", quote).await?;
    let body = match row {
        Some(r) => json!({
            "quote": quote,
            "rateDate": r.rate_date.format("%Y-%m-%d").to_string(),
            "rate": r.rate,
        }),
        None => json!({
            "quote": quote,
            "rateDate": null,
            "rate": null,
        }),
    };
    Ok((StatusCode::OK, Json(body)).into_response())
}
