//! Current-price endpoints (Slice 3 T29, ADR-017 §3).
//!
//! Covers:
//!
//!   * `GET    /api/v1/current-prices`
//!   * `PUT    /api/v1/current-prices/:ticker`
//!   * `DELETE /api/v1/current-prices/:ticker`
//!   * `GET    /api/v1/grants/:id/current-price-override`
//!   * `PUT    /api/v1/grants/:id/current-price-override`
//!   * `DELETE /api/v1/grants/:id/current-price-override`
//!
//! Per AC-5.2.6: per-ticker price edits write **no** audit row (user
//! workspace data). Per-grant overrides DO audit (`grant.current_price_override.upsert|delete`)
//! with a minimal allowlisted payload (SEC-101).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

const CURRENCIES: &[&str] = &["USD", "EUR", "GBP"];
const MAX_PRICE_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Shared validators
// ---------------------------------------------------------------------------

fn ticker_is_valid(t: &str) -> bool {
    let bytes = t.as_bytes();
    if bytes.is_empty() || bytes.len() > 8 {
        return false;
    }
    bytes
        .iter()
        .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

fn normalize_ticker(raw: &str) -> String {
    raw.trim().to_ascii_uppercase()
}

fn validate_price_currency(price: &str, currency: &str) -> Result<(), AppError> {
    let mut errors: Vec<FieldError> = Vec::new();
    if price.trim().is_empty() || price.len() > MAX_PRICE_LEN {
        errors.push(FieldError {
            field: "price".into(),
            code: "current_price.invalid.price".into(),
        });
    }
    match price.parse::<f64>() {
        Ok(v) if v > 0.0 => {}
        _ => errors.push(FieldError {
            field: "price".into(),
            code: "current_price.invalid.price".into(),
        }),
    }
    if !CURRENCIES.contains(&currency) {
        errors.push(FieldError {
            field: "currency".into(),
            code: "current_price.invalid.currency".into(),
        });
    }
    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-ticker current prices
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TickerPriceBody {
    #[validate(length(min = 1, max = 32))]
    pub price: String,
    #[validate(length(equal = 3))]
    pub currency: String,
}

/// `GET /api/v1/current-prices`
pub async fn list(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let rows = orbit_db::ticker_current_prices::list_for_user(&mut tx, auth.user_id).await?;
    tx.commit().await?;

    let prices: Vec<_> = rows
        .into_iter()
        .map(|r| {
            json!({
                "ticker": r.ticker,
                "price": r.price,
                "currency": r.currency,
                "enteredAt": r.entered_at,
            })
        })
        .collect();

    Ok((StatusCode::OK, Json(json!({ "prices": prices }))).into_response())
}

/// `PUT /api/v1/current-prices/:ticker`
pub async fn upsert(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(raw_ticker): Path<String>,
    Json(body): Json<TickerPriceBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    validate_price_currency(&body.price, &body.currency)?;

    let ticker = normalize_ticker(&raw_ticker);
    if !ticker_is_valid(&ticker) {
        return Err(AppError::Validation(vec![FieldError {
            field: "ticker".into(),
            code: "current_price.invalid.ticker".into(),
        }]));
    }

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let row = orbit_db::ticker_current_prices::upsert(
        &mut tx,
        auth.user_id,
        &ticker,
        &body.price,
        &body.currency,
    )
    .await?;
    tx.commit().await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "ticker": row.ticker,
            "price": row.price,
            "currency": row.currency,
            "enteredAt": row.entered_at,
        })),
    )
        .into_response())
}

/// `DELETE /api/v1/current-prices/:ticker`
pub async fn delete(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(raw_ticker): Path<String>,
) -> Result<Response, AppError> {
    let ticker = normalize_ticker(&raw_ticker);
    if !ticker_is_valid(&ticker) {
        return Err(AppError::Validation(vec![FieldError {
            field: "ticker".into(),
            code: "current_price.invalid.ticker".into(),
        }]));
    }

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    orbit_db::ticker_current_prices::delete(&mut tx, auth.user_id, &ticker).await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Per-grant current-price override
// ---------------------------------------------------------------------------

/// `GET /api/v1/grants/:id/current-price-override`
pub async fn get_grant_override(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // Confirm ownership so a miss distinguishes 404 from "no override".
    let grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let row = orbit_db::grant_current_price_overrides::get(&mut tx, auth.user_id, grant.id).await?;
    tx.commit().await?;

    let body = match row {
        Some(r) => json!({
            "override": {
                "price": r.price,
                "currency": r.currency,
                "enteredAt": r.entered_at,
            }
        }),
        None => json!({ "override": null }),
    };
    Ok((StatusCode::OK, Json(body)).into_response())
}

/// `PUT /api/v1/grants/:id/current-price-override`
pub async fn upsert_grant_override(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
    ip: ClientIp,
    Json(body): Json<TickerPriceBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    validate_price_currency(&body.price, &body.currency)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let had_prior = orbit_db::grant_current_price_overrides::get(&mut tx, auth.user_id, grant.id)
        .await?
        .is_some();

    let row = orbit_db::grant_current_price_overrides::upsert(
        &mut tx,
        auth.user_id,
        grant.id,
        &body.price,
        &body.currency,
    )
    .await?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::GrantCurrentPriceOverrideUpsert,
        auth.user_id,
        Some(grant.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "grant_id": grant.id,
            "had_prior": had_prior,
        }),
    )
    .await?;
    tx.commit().await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "override": {
                "price": row.price,
                "currency": row.currency,
                "enteredAt": row.entered_at,
            }
        })),
    )
        .into_response())
}

/// `DELETE /api/v1/grants/:id/current-price-override`
pub async fn delete_grant_override(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
    ip: ClientIp,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let removed =
        orbit_db::grant_current_price_overrides::delete(&mut tx, auth.user_id, grant.id).await?;

    if removed {
        let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::GrantCurrentPriceOverrideDelete,
            auth.user_id,
            Some(grant.id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({
                "grant_id": grant.id,
            }),
        )
        .await?;
    }
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn validation_errors(err: validator::ValidationErrors) -> AppError {
    let fields = err
        .field_errors()
        .into_iter()
        .flat_map(|(name, errs)| {
            errs.iter().map(move |e| FieldError {
                field: name.to_string(),
                code: e.code.to_string(),
            })
        })
        .collect();
    AppError::Validation(fields)
}
