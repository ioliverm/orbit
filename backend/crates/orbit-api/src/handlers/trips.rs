//! Art. 7.p trip endpoints (Slice 2 T21, ADR-016 §3 "Art. 7.p trips").
//!
//! Routes:
//!
//!   * `POST   /api/v1/trips`
//!   * `GET    /api/v1/trips` (with `?year=YYYY` override for the
//!     annual-cap tracker)
//!   * `GET    /api/v1/trips/:id`
//!   * `PUT    /api/v1/trips/:id`
//!   * `DELETE /api/v1/trips/:id`
//!
//! All under the `gated_authed` subtree.
//!
//! The five-criterion checklist is validated at the handler boundary per
//! ADR-016 §9.1 — the DB only enforces `jsonb_typeof = 'object'`, and this
//! module is the authoritative validator for the allowlisted keys. The
//! criteria keys must match the column-comment values in the migration:
//! `services_outside_spain`, `non_spanish_employer`, `not_tax_haven`,
//! `no_double_exemption`, `within_annual_cap`. Each value is `true`,
//! `false`, or `null` (unanswered). AC-5.2.3 says a save is REJECTED if any
//! of the five keys is `null`; the repo still accepts nulls for the edit
//! form's prefill round-trip — the handler is the gatekeeper.
//!
//! Audit-log allowlist (SEC-101):
//! `{ destination_country_iso2, criteria_answered, employer_paid }` on
//! create/update (the country code is low-entropy and already on the
//! session row; dates and raw purpose text are excluded). `{}` on
//! delete. T25 / N2: the key is `destination_country_iso2` — not
//! `country` or `destination_country` — to disambiguate from the DDL
//! column name while still excluding raw destination strings from the
//! forbidden-keys sweep.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{Datelike, NaiveDate, Utc};
use orbit_db::art_7p_trips::{Art7pTrip, Art7pTripForm};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use validator::Validate;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

const CRITERIA_KEYS: &[&str] = &[
    "services_outside_spain",
    "non_spanish_employer",
    "not_tax_haven",
    "no_double_exemption",
    "within_annual_cap",
];

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TripBody {
    /// ISO 3166-1 alpha-2. Handler uppercases before storage so `es` and
    /// `ES` are indistinguishable to the DB CHECK `length = 2`.
    #[validate(length(equal = 2))]
    pub destination_country: String,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub employer_paid: bool,
    #[serde(default)]
    #[validate(length(max = 1024))]
    pub purpose: Option<String>,
    /// Five-key object per ADR-016 §9.1. Values are `true | false | null`.
    /// On CREATE the handler rejects any `null` (AC-5.2.3); on UPDATE the
    /// same rule applies — the `null` pre-fill is a form-level concern,
    /// never persisted.
    pub eligibility_criteria: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TripDto {
    pub id: Uuid,
    pub destination_country: String,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub employer_paid: bool,
    pub purpose: Option<String>,
    pub eligibility_criteria: Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<Art7pTrip> for TripDto {
    fn from(t: Art7pTrip) -> Self {
        TripDto {
            id: t.id,
            destination_country: t.destination_country,
            from_date: t.from_date,
            to_date: t.to_date,
            employer_paid: t.employer_paid,
            purpose: t.purpose,
            eligibility_criteria: t.eligibility_criteria,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Year for the annual-cap tracker (AC-5.1.4). Defaults to the
    /// current calendar year.
    #[serde(default)]
    pub year: Option<i32>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/trips`
pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    ip: ClientIp,
    Json(body): Json<TripBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    let form = body_to_form(&body, /* on_create = */ true)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let trip = orbit_db::art_7p_trips::create(&mut tx, auth.user_id, &form).await?;

    let criteria_answered = count_criteria_answered_true(&trip.eligibility_criteria);
    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::TripCreate,
        auth.user_id,
        Some(trip.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "destination_country_iso2": trip.destination_country,
            "criteria_answered": criteria_answered,
            "employer_paid": trip.employer_paid,
        }),
    )
    .await?;
    tx.commit().await?;

    let dto: TripDto = trip.into();
    Ok((StatusCode::CREATED, Json(json!({ "trip": dto }))).into_response())
}

/// `GET /api/v1/trips`
pub async fn list(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Query(q): Query<ListQuery>,
) -> Result<Response, AppError> {
    let year = q.year.unwrap_or_else(|| Utc::now().date_naive().year());

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let trips = orbit_db::art_7p_trips::list(&mut tx, auth.user_id).await?;
    let summary = orbit_db::art_7p_trips::annual_summary(&mut tx, auth.user_id, year).await?;
    tx.commit().await?;

    let dtos: Vec<TripDto> = trips.into_iter().map(TripDto::from).collect();
    Ok((
        StatusCode::OK,
        Json(json!({
            "trips": dtos,
            "annualCapTracker": {
                "year": summary.year,
                "tripCount": summary.trip_count,
                "dayCountDeclared": summary.day_count_declared,
                "employerPaidTripCount": summary.employer_paid_trip_count,
                "criteriaMetCountByKey": {
                    "services_outside_spain": summary.criterion_services_outside_spain_yes,
                    "non_spanish_employer": summary.criterion_non_spanish_employer_yes,
                    "not_tax_haven": summary.criterion_not_tax_haven_yes,
                    "no_double_exemption": summary.criterion_no_double_exemption_yes,
                    "within_annual_cap": summary.criterion_within_annual_cap_yes,
                },
            }
        })),
    )
        .into_response())
}

/// `GET /api/v1/trips/:id`
pub async fn get_one(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let trip = orbit_db::art_7p_trips::get_by_id(&mut tx, auth.user_id, id)
        .await?
        .ok_or(AppError::NotFound)?;
    tx.commit().await?;

    let dto: TripDto = trip.into();
    Ok((StatusCode::OK, Json(json!({ "trip": dto }))).into_response())
}

/// `PUT /api/v1/trips/:id`
pub async fn update(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(id): Path<Uuid>,
    ip: ClientIp,
    Json(body): Json<TripBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    let form = body_to_form(&body, /* on_create = */ false)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // Ownership: 404 if missing.
    let _ = orbit_db::art_7p_trips::get_by_id(&mut tx, auth.user_id, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let trip = match orbit_db::art_7p_trips::update(&mut tx, auth.user_id, id, &form).await {
        Ok(t) => t,
        Err(sqlx::Error::RowNotFound) => return Err(AppError::NotFound),
        Err(e) => return Err(e.into()),
    };

    let criteria_answered = count_criteria_answered_true(&trip.eligibility_criteria);
    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::TripUpdate,
        auth.user_id,
        Some(trip.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "destination_country_iso2": trip.destination_country,
            "criteria_answered": criteria_answered,
            "employer_paid": trip.employer_paid,
        }),
    )
    .await?;
    tx.commit().await?;

    let dto: TripDto = trip.into();
    Ok((StatusCode::OK, Json(json!({ "trip": dto }))).into_response())
}

/// `DELETE /api/v1/trips/:id`
pub async fn delete(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(id): Path<Uuid>,
    ip: ClientIp,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let _ = orbit_db::art_7p_trips::get_by_id(&mut tx, auth.user_id, id)
        .await?
        .ok_or(AppError::NotFound)?;
    orbit_db::art_7p_trips::delete(&mut tx, auth.user_id, id).await?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::TripDelete,
        auth.user_id,
        Some(id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({}),
    )
    .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Validation + mapping
// ---------------------------------------------------------------------------

fn body_to_form(body: &TripBody, _on_create: bool) -> Result<Art7pTripForm, AppError> {
    let mut errors: Vec<FieldError> = Vec::new();

    let country = body.destination_country.to_ascii_uppercase();
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_alphabetic()) {
        errors.push(FieldError {
            field: "destinationCountry".into(),
            code: "format".into(),
        });
    }
    if body.to_date < body.from_date {
        errors.push(FieldError {
            field: "toDate".into(),
            code: "before_from_date".into(),
        });
    }

    // Eligibility-criteria shape (ADR-016 §9.1).
    validate_eligibility(&body.eligibility_criteria, &mut errors);

    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }

    Ok(Art7pTripForm {
        destination_country: country,
        from_date: body.from_date,
        to_date: body.to_date,
        employer_paid: body.employer_paid,
        purpose: body.purpose.clone(),
        eligibility_criteria: body.eligibility_criteria.clone(),
    })
}

fn validate_eligibility(v: &Value, errors: &mut Vec<FieldError>) {
    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            errors.push(FieldError {
                field: "eligibilityCriteria".into(),
                code: "must_be_object".into(),
            });
            return;
        }
    };
    // Reject unknown keys.
    for k in obj.keys() {
        if !CRITERIA_KEYS.contains(&k.as_str()) {
            errors.push(FieldError {
                field: format!("eligibilityCriteria.{k}"),
                code: "unknown_key".into(),
            });
        }
    }
    // Require every known key, with a true|false|null value — handler
    // rejects `null` (AC-5.2.3) but still tolerates a missing key via the
    // same `answer_required` code, so the UI can treat both as "please
    // answer this criterion".
    for k in CRITERIA_KEYS {
        match obj.get(*k) {
            Some(Value::Bool(_)) => {}
            Some(Value::Null) => errors.push(FieldError {
                field: format!("eligibilityCriteria.{k}"),
                code: "answer_required".into(),
            }),
            Some(_) => errors.push(FieldError {
                field: format!("eligibilityCriteria.{k}"),
                code: "must_be_bool_or_null".into(),
            }),
            None => errors.push(FieldError {
                field: format!("eligibilityCriteria.{k}"),
                code: "answer_required".into(),
            }),
        }
    }
}

fn count_criteria_answered_true(v: &Value) -> i64 {
    let mut n = 0i64;
    if let Some(obj) = v.as_object() {
        for k in CRITERIA_KEYS {
            if obj.get(*k).and_then(|x| x.as_bool()).unwrap_or(false) {
                n += 1;
            }
        }
    }
    n
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
    use serde_json::json;

    fn ok_criteria() -> Value {
        json!({
            "services_outside_spain": true,
            "non_spanish_employer": true,
            "not_tax_haven": true,
            "no_double_exemption": true,
            "within_annual_cap": true,
        })
    }

    fn body(c: Value) -> TripBody {
        TripBody {
            destination_country: "US".into(),
            from_date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            to_date: NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
            employer_paid: true,
            purpose: Some("Kickoff".into()),
            eligibility_criteria: c,
        }
    }

    #[test]
    fn accepts_happy_path() {
        let form = body_to_form(&body(ok_criteria()), true).unwrap();
        assert_eq!(form.destination_country, "US");
    }

    #[test]
    fn uppercases_country() {
        let mut b = body(ok_criteria());
        b.destination_country = "us".into();
        let form = body_to_form(&b, true).unwrap();
        assert_eq!(form.destination_country, "US");
    }

    #[test]
    fn rejects_unknown_criteria_key() {
        let mut c = ok_criteria();
        c.as_object_mut()
            .unwrap()
            .insert("new_key".into(), json!(true));
        let err = body_to_form(&body(c), true).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "unknown_key"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_null_criterion() {
        let mut c = ok_criteria();
        c.as_object_mut()
            .unwrap()
            .insert("not_tax_haven".into(), json!(null));
        let err = body_to_form(&body(c), true).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "answer_required"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_missing_criterion() {
        let mut c = ok_criteria();
        c.as_object_mut().unwrap().remove("no_double_exemption");
        let err = body_to_form(&body(c), true).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "answer_required"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_non_bool_criterion() {
        let mut c = ok_criteria();
        c.as_object_mut()
            .unwrap()
            .insert("services_outside_spain".into(), json!("sí"));
        let err = body_to_form(&body(c), true).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "must_be_bool_or_null"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_dates_out_of_order() {
        let mut b = body(ok_criteria());
        b.from_date = NaiveDate::from_ymd_opt(2026, 4, 15).unwrap();
        b.to_date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let err = body_to_form(&b, true).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "before_from_date"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn counts_yes_answers() {
        let mut c = ok_criteria();
        c.as_object_mut()
            .unwrap()
            .insert("within_annual_cap".into(), json!(false));
        assert_eq!(count_criteria_answered_true(&c), 4);
    }
}
