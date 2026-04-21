//! Grants endpoints (Slice 1 T13b).
//!
//! All five CRUD endpoints plus the vesting read. Every mutation recomputes
//! the `vesting_events` rows via `orbit_core::vesting::derive_vesting_events`
//! inside the same `Tx::for_user`, so the DB invariant is always "grant rows
//! and their derived vesting rows match".
//!
//! Field-level validation is in this file (cross-field rules — cliff vs
//! total, strike required for NSO/ISO — don't fit the `validator` derive
//! cleanly). Length caps that the DB CHECK also enforces stay in
//! `validator` so the client gets a 422 instead of a 500.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{NaiveDate, Utc};
use orbit_core::vesting::{self, Cadence, GrantInput, VestingEvent, VestingState};
use orbit_db::grants::{Grant, GrantForm};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

const INSTRUMENTS_IN: &[&str] = &["rsu", "nso", "espp", "iso"];
const CURRENCIES: &[&str] = &["USD", "EUR", "GBP"];
const CADENCES: &[&str] = &["monthly", "quarterly"];

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GrantBody {
    /// `rsu | nso | espp | iso` — ISO is stored as `iso_mapped_to_nso`
    /// (AC-4.2.1).
    #[validate(length(min = 1, max = 32))]
    pub instrument: String,
    pub grant_date: NaiveDate,
    /// Whole shares. Fractional input is rejected for Slice 1 (the PoC
    /// grant form does not surface fractional ESPP allocations; scaled i64
    /// storage still supports it if a future slice opens the surface).
    pub share_count: i64,
    /// Decimal string, e.g. `"8.00"`. Required for NSO / ISO.
    #[serde(default)]
    #[validate(length(max = 32))]
    pub strike_amount: Option<String>,
    #[serde(default)]
    #[validate(length(equal = 3))]
    pub strike_currency: Option<String>,
    pub vesting_start: NaiveDate,
    #[validate(range(min = 1, max = 240))]
    pub vesting_total_months: i32,
    #[validate(range(min = 0, max = 240))]
    pub cliff_months: i32,
    #[validate(length(min = 1, max = 16))]
    pub vesting_cadence: String,
    #[serde(default)]
    pub double_trigger: bool,
    #[serde(default)]
    pub liquidity_event_date: Option<NaiveDate>,
    #[validate(length(min = 1, max = 256))]
    pub employer_name: String,
    #[serde(default)]
    #[validate(length(min = 1, max = 8))]
    pub ticker: Option<String>,
    #[serde(default)]
    #[validate(length(max = 2048))]
    pub notes: Option<String>,
    /// ESPP-only: slice-1 compromise per T13b spec. Stored as JSON in
    /// `grants.notes` alongside any free-text notes. Slice 2 migrates this
    /// into a dedicated column with the ESPP purchase detail schema.
    #[serde(default)]
    #[validate(range(min = 0, max = 50))]
    pub espp_estimated_discount_pct: Option<i32>,
}

/// Hand-rolled ticker check matching the DDL CHECK
/// `^[A-Z0-9.\-]{1,8}$` — avoids pulling `regex` + `once_cell` into the
/// workspace just for this one pattern.
fn ticker_is_valid(t: &str) -> bool {
    let bytes = t.as_bytes();
    if bytes.is_empty() || bytes.len() > 8 {
        return false;
    }
    bytes
        .iter()
        .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantDto {
    pub id: Uuid,
    pub instrument: String,
    pub grant_date: NaiveDate,
    /// Whole-share integer, serialized as a string (never a JSON number)
    /// so that a future fractional extension doesn't silently lose the
    /// 4-decimal precision at the wire boundary. Slice 1 is always whole.
    pub share_count: String,
    /// The raw scaled-i64 (10_000ths of a share) for clients that want
    /// exact arithmetic.
    pub share_count_scaled: i64,
    pub strike_amount: Option<String>,
    pub strike_currency: Option<String>,
    pub vesting_start: NaiveDate,
    pub vesting_total_months: i32,
    pub cliff_months: i32,
    pub vesting_cadence: String,
    pub double_trigger: bool,
    pub liquidity_event_date: Option<NaiveDate>,
    pub double_trigger_satisfied_by: Option<String>,
    pub employer_name: String,
    pub ticker: Option<String>,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<Grant> for GrantDto {
    fn from(g: Grant) -> Self {
        let whole = g.share_count / orbit_core::SHARES_SCALE;
        GrantDto {
            id: g.id,
            instrument: g.instrument,
            grant_date: g.grant_date,
            share_count: whole.to_string(),
            share_count_scaled: g.share_count,
            strike_amount: g.strike_amount,
            strike_currency: g.strike_currency,
            vesting_start: g.vesting_start,
            vesting_total_months: g.vesting_total_months,
            cliff_months: g.cliff_months,
            vesting_cadence: g.vesting_cadence,
            double_trigger: g.double_trigger,
            liquidity_event_date: g.liquidity_event_date,
            double_trigger_satisfied_by: g.double_trigger_satisfied_by,
            employer_name: g.employer_name,
            ticker: g.ticker,
            notes: g.notes,
            created_at: g.created_at,
            updated_at: g.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VestingEventDto {
    pub vest_date: NaiveDate,
    pub shares_vested_this_event: String,
    pub shares_vested_this_event_scaled: i64,
    pub cumulative_shares_vested: String,
    pub cumulative_shares_vested_scaled: i64,
    pub state: &'static str,
}

impl From<&VestingEvent> for VestingEventDto {
    fn from(e: &VestingEvent) -> Self {
        VestingEventDto {
            vest_date: e.vest_date,
            shares_vested_this_event: scaled_to_whole_string(e.shares_vested_this_event),
            shares_vested_this_event_scaled: e.shares_vested_this_event,
            cumulative_shares_vested: scaled_to_whole_string(e.cumulative_shares_vested),
            cumulative_shares_vested_scaled: e.cumulative_shares_vested,
            state: match e.state {
                VestingState::Upcoming => "upcoming",
                VestingState::TimeVestedAwaitingLiquidity => "time_vested_awaiting_liquidity",
                VestingState::Vested => "vested",
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/grants`
pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    ip: ClientIp,
    Json(body): Json<GrantBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    let form = body_to_form(&body)?;

    let today = Utc::now().date_naive();
    let grant_input = form_to_grant_input(&form).map_err(map_vesting_error)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let grant = orbit_db::grants::create_grant(&mut tx, auth.user_id, &form).await?;
    let events = vesting::derive_vesting_events(&grant_input, today).map_err(map_vesting_error)?;
    orbit_db::vesting_events::replace_for_grant(&mut tx, auth.user_id, grant.id, events.clone())
        .await?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::GrantCreate,
        auth.user_id,
        Some(grant.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "instrument": grant.instrument,
            "double_trigger": grant.double_trigger,
            "cadence": grant.vesting_cadence,
        }),
    )
    .await?;
    tx.commit().await?;

    let dto: GrantDto = grant.into();
    let events_dto: Vec<VestingEventDto> = events.iter().map(VestingEventDto::from).collect();
    Ok((
        StatusCode::CREATED,
        Json(json!({ "grant": dto, "vestingEvents": events_dto })),
    )
        .into_response())
}

/// `GET /api/v1/grants`
pub async fn list(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let grants = orbit_db::grants::list_grants(&mut tx, auth.user_id).await?;
    tx.commit().await?;

    let dtos: Vec<GrantDto> = grants.into_iter().map(GrantDto::from).collect();
    Ok((StatusCode::OK, Json(json!({ "grants": dtos }))).into_response())
}

/// `GET /api/v1/grants/:id`
pub async fn get_one(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;
    tx.commit().await?;

    let dto: GrantDto = grant.into();
    Ok((StatusCode::OK, Json(json!({ "grant": dto }))).into_response())
}

/// `PUT /api/v1/grants/:id`
pub async fn update(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
    ip: ClientIp,
    Json(body): Json<GrantBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    let form = body_to_form(&body)?;

    let today = Utc::now().date_naive();
    let grant_input = form_to_grant_input(&form).map_err(map_vesting_error)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // Confirm the row is owned by the caller before UPDATE — `update_grant`
    // returns `RowNotFound` via RLS scoping, which the `From<sqlx::Error>`
    // impl maps to 404 (AC-7.3).
    let grant = match orbit_db::grants::update_grant(&mut tx, auth.user_id, grant_id, &form).await {
        Ok(g) => g,
        Err(sqlx::Error::RowNotFound) => return Err(AppError::NotFound),
        Err(e) => return Err(e.into()),
    };
    let events = vesting::derive_vesting_events(&grant_input, today).map_err(map_vesting_error)?;
    orbit_db::vesting_events::replace_for_grant(&mut tx, auth.user_id, grant.id, events.clone())
        .await?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::GrantUpdate,
        auth.user_id,
        Some(grant.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "instrument": grant.instrument,
            "double_trigger": grant.double_trigger,
            "cadence": grant.vesting_cadence,
        }),
    )
    .await?;
    tx.commit().await?;

    let dto: GrantDto = grant.into();
    let events_dto: Vec<VestingEventDto> = events.iter().map(VestingEventDto::from).collect();
    Ok((
        StatusCode::OK,
        Json(json!({ "grant": dto, "vestingEvents": events_dto })),
    )
        .into_response())
}

/// `DELETE /api/v1/grants/:id`
pub async fn delete(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
    ip: ClientIp,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // Fetch first so we can 404 on not-owned (AC-7.3) and capture the
    // instrument for the audit payload.
    let grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;
    orbit_db::grants::delete_grant(&mut tx, auth.user_id, grant_id).await?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::GrantDelete,
        auth.user_id,
        Some(grant_id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({ "instrument": grant.instrument }),
    )
    .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/v1/grants/:id/vesting`
pub async fn vesting_for_grant(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // Confirm ownership via a read — RLS already does this but we need the
    // grant's instrument for the optional state echo, and a not-found must
    // 404 (AC-7.3).
    let grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let rows = orbit_db::vesting_events::list_for_grant(&mut tx, auth.user_id, grant.id).await?;
    tx.commit().await?;

    let events: Vec<VestingEvent> = rows
        .iter()
        .map(|r| VestingEvent {
            vest_date: r.vest_date,
            shares_vested_this_event: r.shares_vested_this_event,
            cumulative_shares_vested: r.cumulative_shares_vested,
            state: r.state,
        })
        .collect();
    let (vested, awaiting) = vesting::vested_to_date(&events, Utc::now().date_naive());
    let events_dto: Vec<VestingEventDto> = events.iter().map(VestingEventDto::from).collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "vestingEvents": events_dto,
            "vestedToDate": scaled_to_whole_string(vested),
            "vestedToDateScaled": vested,
            "awaitingLiquidity": scaled_to_whole_string(awaiting),
            "awaitingLiquidityScaled": awaiting,
        })),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn scaled_to_whole_string(v: i64) -> String {
    // Slice 1 is whole-share only; floor-divide is exact for the values
    // we round-trip. A future fractional surface would format to four
    // decimal places here.
    (v / orbit_core::SHARES_SCALE).to_string()
}

fn form_to_grant_input(form: &GrantForm) -> Result<GrantInput, vesting::VestingError> {
    let cadence = match form.vesting_cadence.as_str() {
        "monthly" => Cadence::Monthly,
        "quarterly" => Cadence::Quarterly,
        // Unreachable under validation; keep as a conservative fallback.
        _ => Cadence::Monthly,
    };
    let total: u32 = form.vesting_total_months.try_into().unwrap_or(0);
    let cliff: u32 = form.cliff_months.try_into().unwrap_or(0);
    Ok(GrantInput {
        share_count: form.share_count,
        vesting_start: form.vesting_start,
        vesting_total_months: total,
        cliff_months: cliff,
        cadence,
        double_trigger: form.double_trigger,
        liquidity_event_date: form.liquidity_event_date,
    })
}

/// Cross-field validation + mapping to the DB form.
fn body_to_form(body: &GrantBody) -> Result<GrantForm, AppError> {
    let mut errors: Vec<FieldError> = Vec::new();

    if !INSTRUMENTS_IN.contains(&body.instrument.as_str()) {
        errors.push(FieldError {
            field: "instrument".into(),
            code: "unsupported".into(),
        });
    }
    if !CADENCES.contains(&body.vesting_cadence.as_str()) {
        errors.push(FieldError {
            field: "vestingCadence".into(),
            code: "unsupported".into(),
        });
    }
    if body.share_count <= 0 {
        errors.push(FieldError {
            field: "shareCount".into(),
            code: "must_be_positive".into(),
        });
    }
    if body.cliff_months > body.vesting_total_months {
        errors.push(FieldError {
            field: "cliffMonths".into(),
            code: "cliff_exceeds_vesting".into(),
        });
    }

    // Normalize the instrument before the strike-required check.
    let stored_instrument = match body.instrument.as_str() {
        "iso" => "iso_mapped_to_nso",
        other => other,
    };
    let options_like = matches!(stored_instrument, "nso" | "iso_mapped_to_nso");
    if options_like {
        if body.strike_amount.is_none() {
            errors.push(FieldError {
                field: "strikeAmount".into(),
                code: "required_for_options".into(),
            });
        }
        if body.strike_currency.is_none() {
            errors.push(FieldError {
                field: "strikeCurrency".into(),
                code: "required_for_options".into(),
            });
        }
    }
    if let Some(ccy) = body.strike_currency.as_deref() {
        if !CURRENCIES.contains(&ccy) {
            errors.push(FieldError {
                field: "strikeCurrency".into(),
                code: "unsupported".into(),
            });
        }
    }
    if let Some(ticker) = body.ticker.as_deref() {
        if !ticker_is_valid(ticker) {
            errors.push(FieldError {
                field: "ticker".into(),
                code: "format".into(),
            });
        }
    }
    if body.double_trigger && stored_instrument != "rsu" {
        errors.push(FieldError {
            field: "doubleTrigger".into(),
            code: "rsu_only".into(),
        });
    }

    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }

    // Build the `notes` field. ESPP estimated discount is packed into a
    // JSON blob alongside any free-text note (T13b Slice-1 compromise;
    // Slice 2 replaces this with a dedicated `grants.espp_*` column set).
    let notes = merge_notes(body.notes.as_deref(), body.espp_estimated_discount_pct);

    // RSU/ESPP silently drop strike fields (AC: "RSU ignores strike field
    // if sent"). Options-like keeps what was sent.
    let (strike_amount, strike_currency) = if options_like {
        (body.strike_amount.clone(), body.strike_currency.clone())
    } else {
        (None, None)
    };

    Ok(GrantForm {
        instrument: stored_instrument.to_string(),
        grant_date: body.grant_date,
        share_count: orbit_core::whole_shares(body.share_count),
        strike_amount,
        strike_currency,
        vesting_start: body.vesting_start,
        vesting_total_months: body.vesting_total_months,
        cliff_months: body.cliff_months,
        vesting_cadence: body.vesting_cadence.clone(),
        double_trigger: body.double_trigger,
        liquidity_event_date: body.liquidity_event_date,
        double_trigger_satisfied_by: None,
        employer_name: body.employer_name.clone(),
        ticker: body.ticker.clone(),
        notes,
    })
}

fn merge_notes(notes: Option<&str>, espp_pct: Option<i32>) -> Option<String> {
    match (notes, espp_pct) {
        (None, None) => None,
        (Some(n), None) => Some(n.to_string()),
        (None, Some(p)) => Some(format!(r#"{{"estimated_discount_percent":{p}}}"#)),
        (Some(n), Some(p)) => Some(format!(
            r#"{{"estimated_discount_percent":{p},"note":{}}}"#,
            serde_json::to_string(n).unwrap_or_else(|_| "\"\"".to_string())
        )),
    }
}

fn map_vesting_error(err: vesting::VestingError) -> AppError {
    let field = match err {
        vesting::VestingError::NonPositiveShareCount => FieldError {
            field: "shareCount".into(),
            code: "must_be_positive".into(),
        },
        vesting::VestingError::TotalMonthsOutOfRange => FieldError {
            field: "vestingTotalMonths".into(),
            code: "out_of_range".into(),
        },
        vesting::VestingError::CliffExceedsTotal => FieldError {
            field: "cliffMonths".into(),
            code: "cliff_exceeds_vesting".into(),
        },
        vesting::VestingError::DateOverflow => FieldError {
            field: "vestingStart".into(),
            code: "date_overflow".into(),
        },
    };
    AppError::Validation(vec![field])
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn base_body() -> GrantBody {
        GrantBody {
            instrument: "rsu".into(),
            grant_date: NaiveDate::from_ymd_opt(2024, 9, 15).unwrap(),
            share_count: 30_000,
            strike_amount: None,
            strike_currency: None,
            vesting_start: NaiveDate::from_ymd_opt(2024, 9, 15).unwrap(),
            vesting_total_months: 48,
            cliff_months: 12,
            vesting_cadence: "monthly".into(),
            double_trigger: true,
            liquidity_event_date: None,
            employer_name: "ACME Inc.".into(),
            ticker: None,
            notes: None,
            espp_estimated_discount_pct: None,
        }
    }

    #[test]
    fn rsu_happy_path_builds_form() {
        let form = body_to_form(&base_body()).expect("ok");
        assert_eq!(form.instrument, "rsu");
        assert_eq!(form.share_count, 30_000 * orbit_core::SHARES_SCALE);
        assert!(form.strike_amount.is_none());
    }

    #[test]
    fn iso_maps_to_iso_mapped_to_nso_and_requires_strike() {
        let mut body = base_body();
        body.instrument = "iso".into();
        body.double_trigger = false;
        let err = body_to_form(&body).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.field == "strikeAmount"));
            }
            _ => panic!("expected Validation"),
        }

        body.strike_amount = Some("8.00".into());
        body.strike_currency = Some("USD".into());
        let form = body_to_form(&body).unwrap();
        assert_eq!(form.instrument, "iso_mapped_to_nso");
        assert_eq!(form.strike_currency.as_deref(), Some("USD"));
    }

    #[test]
    fn rsu_ignores_sent_strike_fields() {
        let mut body = base_body();
        body.strike_amount = Some("8.00".into());
        body.strike_currency = Some("USD".into());
        let form = body_to_form(&body).unwrap();
        assert!(form.strike_amount.is_none());
        assert!(form.strike_currency.is_none());
    }

    #[test]
    fn rejects_cliff_exceeds_total() {
        let mut body = base_body();
        body.cliff_months = 49;
        let err = body_to_form(&body).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "cliff_exceeds_vesting"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_zero_shares() {
        let mut body = base_body();
        body.share_count = 0;
        let err = body_to_form(&body).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "must_be_positive"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_double_trigger_on_non_rsu() {
        let mut body = base_body();
        body.instrument = "nso".into();
        body.strike_amount = Some("8".into());
        body.strike_currency = Some("USD".into());
        body.double_trigger = true;
        let err = body_to_form(&body).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.field == "doubleTrigger"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn espp_estimated_discount_packs_into_notes_json() {
        let mut body = base_body();
        body.instrument = "espp".into();
        body.double_trigger = false;
        body.espp_estimated_discount_pct = Some(15);
        let form = body_to_form(&body).unwrap();
        let notes = form.notes.unwrap();
        assert!(notes.contains("\"estimated_discount_percent\":15"));
    }
}
