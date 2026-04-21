//! Residency endpoints (Slice 1 T13b).
//!
//!   * `GET  /api/v1/residency/autonomias` — unauthenticated, canonical
//!     autonomía list (AC-4.1.2).
//!   * `POST /api/v1/residency` — authenticated, creates a new period;
//!     closes the prior open row if any (AC-4.1.7).
//!   * `GET  /api/v1/residency` — authenticated, returns the current
//!     period + `users.primary_currency`.

use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::residency::autonomias::{self, AUTONOMIAS};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// GET /residency/autonomias (unauthenticated)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutonomiaDto {
    code: &'static str,
    name_es: &'static str,
    name_en: &'static str,
    foral: bool,
}

/// `GET /api/v1/residency/autonomias` — server-authoritative list.
/// Cache-friendly (1h TTL) because the list is a constant.
pub async fn list_autonomias() -> Response {
    let list: Vec<AutonomiaDto> = AUTONOMIAS
        .iter()
        .map(|a| AutonomiaDto {
            code: a.code,
            name_es: a.name_es,
            name_en: a.name_en,
            foral: a.foral,
        })
        .collect();
    let mut resp = (StatusCode::OK, Json(json!({ "autonomias": list }))).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    resp
}

// ---------------------------------------------------------------------------
// POST /residency
// ---------------------------------------------------------------------------

const ALLOWED_JURISDICTIONS: &[&str] = &["ES"]; // UK arrives in a later slice.
const ALLOWED_CURRENCIES: &[&str] = &["EUR", "USD"];
const ALLOWED_REGIME_FLAGS: &[&str] = &["beckham_law", "foral_pais_vasco", "foral_navarra"];

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ResidencyBody {
    #[validate(length(equal = 2))]
    pub jurisdiction: String,
    #[serde(default)]
    pub sub_jurisdiction: Option<String>,
    #[validate(length(equal = 3))]
    pub primary_currency: String,
    #[serde(default)]
    pub regime_flags: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidencyDto {
    pub id: Uuid,
    pub jurisdiction: String,
    pub sub_jurisdiction: Option<String>,
    pub from_date: chrono::NaiveDate,
    pub to_date: Option<chrono::NaiveDate>,
    pub regime_flags: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidencyResponse {
    pub residency: ResidencyDto,
    pub primary_currency: String,
}

/// `POST /api/v1/residency`
pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    ip: ClientIp,
    Json(body): Json<ResidencyBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;

    let mut field_errors: Vec<FieldError> = Vec::new();

    if !ALLOWED_JURISDICTIONS.contains(&body.jurisdiction.as_str()) {
        field_errors.push(FieldError {
            field: "jurisdiction".into(),
            code: "unsupported".into(),
        });
    }
    if !ALLOWED_CURRENCIES.contains(&body.primary_currency.as_str()) {
        field_errors.push(FieldError {
            field: "primaryCurrency".into(),
            code: "unsupported".into(),
        });
    }
    let sub = body.sub_jurisdiction.as_deref();
    match sub {
        Some(code) if !autonomias::is_known(code) => {
            field_errors.push(FieldError {
                field: "subJurisdiction".into(),
                code: "unknown_autonomia".into(),
            });
        }
        // Jurisdiction ES requires a sub-jurisdiction (autonomía) — AC-4.1.1.
        None if body.jurisdiction == "ES" => {
            field_errors.push(FieldError {
                field: "subJurisdiction".into(),
                code: "required".into(),
            });
        }
        _ => {}
    }
    for flag in &body.regime_flags {
        if !ALLOWED_REGIME_FLAGS.contains(&flag.as_str()) {
            field_errors.push(FieldError {
                field: "regimeFlags".into(),
                code: "unknown_flag".into(),
            });
        }
    }
    // Foral flags must match the selected autonomía (spec §3).
    if body.regime_flags.iter().any(|f| f == "foral_pais_vasco") && sub != Some("ES-PV") {
        field_errors.push(FieldError {
            field: "regimeFlags".into(),
            code: "foral_mismatch".into(),
        });
    }
    if body.regime_flags.iter().any(|f| f == "foral_navarra") && sub != Some("ES-NA") {
        field_errors.push(FieldError {
            field: "regimeFlags".into(),
            code: "foral_mismatch".into(),
        });
    }
    if !field_errors.is_empty() {
        return Err(AppError::Validation(field_errors));
    }

    // Read the prior state (for the audit `*_changed` booleans). The reads
    // and the write happen in the same Tx::for_user so the snapshot is
    // consistent with what we persist.
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;

    let prior = orbit_db::residency::current_period(&mut tx, auth.user_id).await?;
    let prior_currency: String = sqlx::query_scalar(
        "SELECT primary_currency FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(auth.user_id)
    .fetch_one(tx.as_executor())
    .await?;

    let today = Utc::now().date_naive();
    let new_row = orbit_db::residency::close_and_create(
        &mut tx,
        auth.user_id,
        &body.jurisdiction,
        sub,
        today,
        &body.regime_flags,
    )
    .await?;

    if prior_currency != body.primary_currency {
        sqlx::query("UPDATE users SET primary_currency = $2 WHERE id = $1")
            .bind(auth.user_id)
            .bind(&body.primary_currency)
            .execute(tx.as_executor())
            .await?;
    }

    // Audit summary: booleans only (SEC-101 / AC-4.1.8) — never the
    // values. Rides the same tx as the residency write (T25 / S1).
    let autonomia_changed = prior
        .as_ref()
        .map(|p| p.sub_jurisdiction.as_deref() != sub)
        .unwrap_or(true);
    let prior_beckham = prior
        .as_ref()
        .map(|p| p.regime_flags.iter().any(|f| f == "beckham_law"))
        .unwrap_or(false);
    let new_beckham = body.regime_flags.iter().any(|f| f == "beckham_law");
    let beckham_changed = prior_beckham != new_beckham;
    let currency_changed = prior_currency != body.primary_currency;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::ResidencyCreate,
        auth.user_id,
        Some(new_row.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "autonomia_changed": autonomia_changed,
            "beckham_changed": beckham_changed,
            "currency_changed": currency_changed,
        }),
    )
    .await?;
    tx.commit().await?;

    let body = ResidencyResponse {
        residency: ResidencyDto {
            id: new_row.id,
            jurisdiction: new_row.jurisdiction,
            sub_jurisdiction: new_row.sub_jurisdiction,
            from_date: new_row.from_date,
            to_date: new_row.to_date,
            regime_flags: new_row.regime_flags,
        },
        primary_currency: body.primary_currency,
    };
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

/// `GET /api/v1/residency`
pub async fn get(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Json<Value>, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let current = orbit_db::residency::current_period(&mut tx, auth.user_id).await?;
    let primary_currency: String = sqlx::query_scalar(
        "SELECT primary_currency FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(auth.user_id)
    .fetch_one(tx.as_executor())
    .await?;
    tx.commit().await?;

    let residency = current.map(|p| ResidencyDto {
        id: p.id,
        jurisdiction: p.jurisdiction,
        sub_jurisdiction: p.sub_jurisdiction,
        from_date: p.from_date,
        to_date: p.to_date,
        regime_flags: p.regime_flags,
    });

    Ok(Json(json!({
        "residency": residency,
        "primaryCurrency": primary_currency,
    })))
}

// ---------------------------------------------------------------------------
// Internals shared with `/auth/me`
// ---------------------------------------------------------------------------

/// Shared helper: compute the current onboarding stage + a JSON residency
/// summary for `/auth/me`. Cheap enough for Slice 1 (two small SELECTs).
pub(crate) async fn stage_and_summary(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    disclaimer_accepted: bool,
) -> Result<(&'static str, Option<Value>), AppError> {
    let mut tx = orbit_db::Tx::for_user(pool, user_id).await?;
    let current = orbit_db::residency::current_period(&mut tx, user_id).await?;
    let has_grant: bool = {
        let row = sqlx::query("SELECT EXISTS(SELECT 1 FROM grants WHERE user_id = $1) AS g")
            .bind(user_id)
            .fetch_one(tx.as_executor())
            .await?;
        row.try_get::<bool, _>("g")
            .map_err(|_| AppError::Internal)?
    };
    tx.commit().await?;

    let stage = if !disclaimer_accepted {
        "disclaimer"
    } else if current.is_none() {
        "residency"
    } else if !has_grant {
        "first_grant"
    } else {
        "complete"
    };

    let summary = current.map(|p| {
        json!({
            "id": p.id,
            "jurisdiction": p.jurisdiction,
            "subJurisdiction": p.sub_jurisdiction,
            "fromDate": p.from_date,
            "toDate": p.to_date,
            "regimeFlags": p.regime_flags,
        })
    });

    Ok((stage, summary))
}

/// Stage resolution used by the onboarding-gate middleware.
pub(crate) async fn resolve_stage(
    pool: &sqlx::PgPool,
    user_id: Uuid,
) -> Result<&'static str, AppError> {
    let disclaimer: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT disclaimer_accepted_at FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    let disclaimer_accepted = disclaimer.is_some();
    let (stage, _) = stage_and_summary(pool, user_id, disclaimer_accepted).await?;
    Ok(stage)
}

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
