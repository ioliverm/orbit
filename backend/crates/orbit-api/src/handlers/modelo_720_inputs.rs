//! Modelo 720 user-input endpoints (Slice 2 T21, ADR-016 §3 "Modelo 720
//! user inputs").
//!
//! Routes:
//!
//!   * `POST /api/v1/modelo-720-inputs`
//!   * `GET  /api/v1/modelo-720-inputs/current?category=...`
//!   * `GET  /api/v1/modelo-720-inputs?category=...`
//!
//! Close-and-create semantics delegated to
//! `orbit_db::modelo_720_inputs::create_or_upsert_same_day`; the handler's
//! job is to (a) validate the two-category allowlist (AC-6.1.5: securities
//! stays derived — Slice 3), (b) clamp `reference_date` to today when
//! omitted, (c) select an HTTP status + audit payload based on the
//! returned `UpsertOutcome`.
//!
//! Audit-log allowlist (SEC-101): `{ category, outcome }` where `outcome`
//! is `"inserted" | "closed_and_created" | "updated_same_day"`. **NoOp**
//! writes no audit row per AC-6.2.5.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{NaiveDate, Utc};
use orbit_db::modelo_720_inputs::{Modelo720UpsertForm, Modelo720UserInput, UpsertOutcome};
use serde::{Deserialize, Serialize};
use serde_json::json;
use validator::Validate;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

const CATEGORIES: &[&str] = &["bank_accounts", "real_estate"];

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpsertBody {
    /// `bank_accounts | real_estate` — the securities category does not
    /// exist in Slice 2 (AC-6.1.5 stub).
    #[validate(length(min = 1, max = 32))]
    pub category: String,
    /// Decimal string with up to 2 dp; `>= 0`. Carried as `String` so
    /// clients that submit `"100"` vs `"100.00"` round-trip identically
    /// via the DB `NUMERIC` cast.
    #[validate(length(min = 1, max = 32))]
    pub total_eur: String,
    /// Defaults to today (UTC) when omitted.
    #[serde(default)]
    pub reference_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryQuery {
    pub category: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Modelo720InputDto {
    pub id: uuid::Uuid,
    pub category: String,
    pub amount_eur: String,
    pub reference_date: NaiveDate,
    pub from_date: NaiveDate,
    pub to_date: Option<NaiveDate>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<Modelo720UserInput> for Modelo720InputDto {
    fn from(r: Modelo720UserInput) -> Self {
        Modelo720InputDto {
            id: r.id,
            category: r.category,
            amount_eur: r.amount_eur,
            reference_date: r.reference_date,
            from_date: r.from_date,
            to_date: r.to_date,
            created_at: r.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/modelo-720-inputs`
pub async fn upsert(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    ip: ClientIp,
    Json(body): Json<UpsertBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    validate_upsert_shape(&body)?;

    let today = Utc::now().date_naive();
    let form = Modelo720UpsertForm {
        category: body.category.clone(),
        amount_eur: body.total_eur.clone(),
        reference_date: body.reference_date.unwrap_or(today),
        today,
    };

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let outcome =
        orbit_db::modelo_720_inputs::create_or_upsert_same_day(&mut tx, auth.user_id, &form)
            .await?;
    tx.commit().await?;

    let (status, outcome_label, write_audit) = match &outcome {
        UpsertOutcome::Inserted(_) => (StatusCode::CREATED, "inserted", true),
        UpsertOutcome::ClosedAndCreated(_) => (StatusCode::CREATED, "closed_and_created", true),
        UpsertOutcome::UpdatedSameDay(_) => (StatusCode::OK, "updated_same_day", true),
        UpsertOutcome::NoOp(_) => (StatusCode::OK, "no_op", false),
    };

    if write_audit {
        let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
        audit::record_wizard(
            &state.pool,
            WizardAction::Modelo720Upsert,
            auth.user_id,
            Some(outcome.row().id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({
                "category": outcome.row().category,
                "outcome": outcome_label,
            }),
        )
        .await?;
    }

    let dto: Modelo720InputDto = outcome.row().clone().into();
    let mut body = json!({
        "current": dto,
        "outcome": outcome_label,
    });
    if matches!(outcome, UpsertOutcome::NoOp(_)) {
        body["unchanged"] = json!(true);
    }
    Ok((status, Json(body)).into_response())
}

/// `GET /api/v1/modelo-720-inputs/current?category=...`
pub async fn get_current(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Query(q): Query<CategoryQuery>,
) -> Result<Response, AppError> {
    if !CATEGORIES.contains(&q.category.as_str()) {
        return Err(AppError::Validation(vec![FieldError {
            field: "category".into(),
            code: "unsupported".into(),
        }]));
    }

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let current = orbit_db::modelo_720_inputs::current(&mut tx, auth.user_id, &q.category).await?;
    tx.commit().await?;

    let dto: Option<Modelo720InputDto> = current.map(Modelo720InputDto::from);
    Ok((StatusCode::OK, Json(json!({ "current": dto }))).into_response())
}

/// `GET /api/v1/modelo-720-inputs?category=...` — full history for one
/// category, newest `from_date` first.
pub async fn list_history(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Query(q): Query<CategoryQuery>,
) -> Result<Response, AppError> {
    if !CATEGORIES.contains(&q.category.as_str()) {
        return Err(AppError::Validation(vec![FieldError {
            field: "category".into(),
            code: "unsupported".into(),
        }]));
    }

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // `history()` returns both categories; filter in-handler for a stable
    // per-category response shape.
    let all = orbit_db::modelo_720_inputs::history(&mut tx, auth.user_id).await?;
    tx.commit().await?;

    let dtos: Vec<Modelo720InputDto> = all
        .into_iter()
        .filter(|r| r.category == q.category)
        .map(Modelo720InputDto::from)
        .collect();
    Ok((StatusCode::OK, Json(json!({ "history": dtos }))).into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn validate_upsert_shape(body: &UpsertBody) -> Result<(), AppError> {
    let mut errors: Vec<FieldError> = Vec::new();

    if !CATEGORIES.contains(&body.category.as_str()) {
        errors.push(FieldError {
            field: "category".into(),
            code: "unsupported".into(),
        });
    }
    // Amount must parse as a non-negative decimal.
    let parsed: Result<f64, _> = body.total_eur.trim().parse();
    match parsed {
        Ok(v) if v.is_finite() && v >= 0.0 => {}
        _ => errors.push(FieldError {
            field: "totalEur".into(),
            code: "must_be_non_negative".into(),
        }),
    }

    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }
    Ok(())
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

// ---------------------------------------------------------------------------
// Tests (pure)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> UpsertBody {
        UpsertBody {
            category: "bank_accounts".into(),
            total_eur: "25000.00".into(),
            reference_date: None,
        }
    }

    #[test]
    fn accepts_valid() {
        validate_upsert_shape(&body()).expect("ok");
    }

    #[test]
    fn rejects_securities_category() {
        let mut b = body();
        b.category = "securities".into();
        let err = validate_upsert_shape(&b).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.field == "category"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_negative_amount() {
        let mut b = body();
        b.total_eur = "-10".into();
        let err = validate_upsert_shape(&b).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "must_be_non_negative"));
            }
            _ => panic!("expected Validation"),
        }
    }
}
