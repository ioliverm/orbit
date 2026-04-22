//! User tax-preferences endpoints (Slice 3b T38, ADR-018 §3).
//!
//! Routes:
//!
//!   * `POST /api/v1/user-tax-preferences` — close-and-create + same-day
//!     idempotency per ADR-016 close-and-create pattern.
//!   * `GET  /api/v1/user-tax-preferences/current` — active row or null.
//!   * `GET  /api/v1/user-tax-preferences` — full history, `from_date DESC`.
//!
//! Close-and-create semantics delegated to
//! [`orbit_db::user_tax_preferences::create_or_upsert_same_day`]; the
//! handler's job is to (a) validate the country allowlist, (b) validate
//! the `rendimiento_del_trabajo_percent` fraction is in `[0, 1]` (or
//! null), (c) gate `sellToCoverEnabled` presence (NOT NULL, no server
//! default), (d) translate the outcome into an HTTP status + audit
//! payload.
//!
//! Audit-log allowlist (SEC-101-strict, ADR-018 §5): `{ outcome }`.
//! **Never** country, percent, or the boolean toggle — an auditor
//! reconstructing the series sees only save cadence.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{NaiveDate, Utc};
use orbit_db::user_tax_preferences::{
    self, UpsertOutcome, UserTaxPreference, UserTaxPreferenceUpsertForm,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

/// Curated v1 country list (ADR-018 §1 — handler-gated, ISO-3166
/// alpha-2 uppercase). Growing the list is a one-line change here,
/// not a schema migration. Spain + EU-5 + UK per AC-4.2.1.
const COUNTRIES: &[&str] = &["ES", "PT", "FR", "IT", "DE", "NL", "GB"];

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertBody {
    /// ISO-3166 alpha-2, uppercase. Curated to the v1 list (see
    /// [`COUNTRIES`]).
    pub country_iso2: String,
    /// Stringified fraction in `[0, 1]` (e.g., `"0.4500"`), or `null`
    /// when hidden / blank (AC-4.2.2, AC-4.2.3). The client converts
    /// from the user-facing percent (`45`) to the fraction (`0.4500`)
    /// before sending.
    #[serde(default)]
    pub rendimiento_del_trabajo_percent: Option<String>,
    /// NOT NULL on the DDL; required in the body per ADR-018 §1 (no
    /// server default).
    pub sell_to_cover_enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTaxPreferenceDto {
    pub id: Uuid,
    pub country_iso2: String,
    /// NUMERIC(5,4) passthrough. Rendered as `"0.4500"` or `null`.
    pub rendimiento_del_trabajo_percent: Option<String>,
    pub sell_to_cover_enabled: bool,
    pub from_date: NaiveDate,
    pub to_date: Option<NaiveDate>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<UserTaxPreference> for UserTaxPreferenceDto {
    fn from(r: UserTaxPreference) -> Self {
        UserTaxPreferenceDto {
            id: r.id,
            country_iso2: r.country_iso2,
            rendimiento_del_trabajo_percent: r.rendimiento_del_trabajo_percent,
            sell_to_cover_enabled: r.sell_to_cover_enabled,
            from_date: r.from_date,
            to_date: r.to_date,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/user-tax-preferences`
pub async fn upsert(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    ip: ClientIp,
    Json(body): Json<UpsertBody>,
) -> Result<Response, AppError> {
    let (country, percent, sell_to_cover_enabled) = validate_upsert_shape(&body)?;

    let today = Utc::now().date_naive();
    let form = UserTaxPreferenceUpsertForm {
        country_iso2: country,
        rendimiento_del_trabajo_percent: percent,
        sell_to_cover_enabled,
        today,
    };

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let outcome =
        user_tax_preferences::create_or_upsert_same_day(&mut tx, auth.user_id, &form).await?;

    let (status, outcome_label, write_audit) = match &outcome {
        UpsertOutcome::Inserted(_) => (StatusCode::CREATED, "inserted", true),
        UpsertOutcome::ClosedAndCreated(_) => (StatusCode::CREATED, "closed_and_created", true),
        UpsertOutcome::UpdatedSameDay(_) => (StatusCode::OK, "updated_same_day", true),
        UpsertOutcome::NoOp(_) => (StatusCode::OK, "no_op", false),
    };

    if write_audit {
        let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::UserTaxPreferencesUpsert,
            auth.user_id,
            Some(outcome.row().id),
            ip_hash.as_ref().map(|s| &s[..]),
            // SEC-101-strict per ADR-018 §5: `{ outcome }` only —
            // no country, no percent, no toggle.
            json!({ "outcome": outcome_label }),
        )
        .await?;
    }
    tx.commit().await?;

    let dto: UserTaxPreferenceDto = outcome.row().clone().into();
    let mut body = json!({
        "current": dto,
        "outcome": outcome_label,
    });
    if matches!(outcome, UpsertOutcome::NoOp(_)) {
        body["unchanged"] = json!(true);
    }
    Ok((status, Json(body)).into_response())
}

/// `GET /api/v1/user-tax-preferences/current`
pub async fn get_current(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let current = user_tax_preferences::current(&mut tx, auth.user_id).await?;
    tx.commit().await?;

    let dto: Option<UserTaxPreferenceDto> = current.map(UserTaxPreferenceDto::from);
    Ok((StatusCode::OK, Json(json!({ "current": dto }))).into_response())
}

/// `GET /api/v1/user-tax-preferences` — history, `from_date DESC`.
pub async fn list_history(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let all = user_tax_preferences::history(&mut tx, auth.user_id).await?;
    tx.commit().await?;

    let dtos: Vec<UserTaxPreferenceDto> = all.into_iter().map(UserTaxPreferenceDto::from).collect();
    Ok((StatusCode::OK, Json(json!({ "preferences": dtos }))).into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validates the body and normalizes into (country, percent, toggle).
/// Returns Validation errors with the stable codes pinned by
/// ADR-018 §3.
fn validate_upsert_shape(body: &UpsertBody) -> Result<(String, Option<String>, bool), AppError> {
    let mut errors: Vec<FieldError> = Vec::new();

    // Country — normalize to upper + check against curated list.
    let country_upper = body.country_iso2.trim().to_uppercase();
    if country_upper.len() != 2 || !COUNTRIES.contains(&country_upper.as_str()) {
        errors.push(FieldError {
            field: "countryIso2".into(),
            code: "user_tax_preferences.country.invalid".into(),
        });
    }

    // Percent — must parse as a decimal in `[0, 1]` or be null.
    let percent = body
        .rendimiento_del_trabajo_percent
        .as_deref()
        .map(str::trim);
    if let Some(raw) = percent {
        if raw.is_empty() {
            // Empty string is treated as null (client shouldn't send
            // empty strings but be forgiving). The row gets NULL.
            // Nothing to push.
        } else {
            match raw.parse::<f64>() {
                Ok(v) if v.is_finite() && (0.0..=1.0).contains(&v) => {}
                _ => errors.push(FieldError {
                    field: "rendimientoDelTrabajoPercent".into(),
                    code: "user_tax_preferences.percent.out_of_range".into(),
                }),
            }
        }
    }

    // sellToCoverEnabled — required, no default (ADR-018 §1 rationale).
    if body.sell_to_cover_enabled.is_none() {
        errors.push(FieldError {
            field: "sellToCoverEnabled".into(),
            code: "user_tax_preferences.sell_to_cover_enabled.required".into(),
        });
    }

    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }

    let percent_out: Option<String> = match percent {
        None => None,
        Some("") => None,
        Some(s) => Some(s.to_string()),
    };
    Ok((
        country_upper,
        percent_out,
        body.sell_to_cover_enabled.unwrap(),
    ))
}

// ---------------------------------------------------------------------------
// Tests (pure)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> UpsertBody {
        UpsertBody {
            country_iso2: "ES".into(),
            rendimiento_del_trabajo_percent: Some("0.4500".into()),
            sell_to_cover_enabled: Some(true),
        }
    }

    #[test]
    fn happy_path() {
        let (c, p, s) = validate_upsert_shape(&body()).unwrap();
        assert_eq!(c, "ES");
        assert_eq!(p.as_deref(), Some("0.4500"));
        assert!(s);
    }

    #[test]
    fn lowercase_country_normalizes() {
        let mut b = body();
        b.country_iso2 = "es".into();
        let (c, _, _) = validate_upsert_shape(&b).unwrap();
        assert_eq!(c, "ES");
    }

    #[test]
    fn rejects_unknown_country() {
        let mut b = body();
        b.country_iso2 = "XX".into();
        let err = validate_upsert_shape(&b).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v
                    .iter()
                    .any(|f| f.code == "user_tax_preferences.country.invalid"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_percent_out_of_range() {
        let mut b = body();
        b.rendimiento_del_trabajo_percent = Some("1.5000".into());
        let err = validate_upsert_shape(&b).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v
                    .iter()
                    .any(|f| f.code == "user_tax_preferences.percent.out_of_range"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn accepts_null_percent() {
        let mut b = body();
        b.rendimiento_del_trabajo_percent = None;
        let (_, p, _) = validate_upsert_shape(&b).unwrap();
        assert!(p.is_none());
    }

    #[test]
    fn rejects_missing_sell_to_cover_toggle() {
        let mut b = body();
        b.sell_to_cover_enabled = None;
        let err = validate_upsert_shape(&b).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v
                    .iter()
                    .any(|f| f.code == "user_tax_preferences.sell_to_cover_enabled.required"));
            }
            _ => panic!("expected Validation"),
        }
    }
}
