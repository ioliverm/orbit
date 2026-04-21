//! Vesting-event override endpoints (Slice 3 T29, ADR-017 §3).
//!
//! Covers:
//!
//!   * `PUT  /api/v1/grants/:grantId/vesting-events/:eventId`
//!   * `POST /api/v1/grants/:grantId/vesting-events/bulk-fmv`
//!
//! Optimistic concurrency (AC-10.5): the PUT carries the client's
//! `expectedUpdatedAt` from the last read. The DB repo's
//! `apply_override` predicates the UPDATE on `updated_at = $expected`
//! and returns `OverrideOutcome::Conflict` on mismatch — the handler
//! then returns a 409 with `code = "resource.stale_client_state"`.
//!
//! Audit allowlists (SEC-101 + AC-8.10):
//!
//!   * `vesting_event.override` — `{ grant_id, fields_changed }`
//!   * `vesting_event.clear_override` — `{ grant_id, cleared_fields, preserved }`
//!   * `vesting_event.bulk_fmv` — `{ grant_id, applied_count, skipped_count }`

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, NaiveDate, Utc};
use orbit_core::SHARES_SCALE;
use orbit_db::vesting_events::{
    self, ClearOutcome, OverrideOutcome, VestingEventOverridePatch, VestingEventRow,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

const CURRENCIES: &[&str] = &["USD", "EUR", "GBP"];
const MAX_FUTURE_DAYS: i64 = 365;
const MAX_PRICE_LEN: usize = 32;

// ---------------------------------------------------------------------------
// PUT /grants/:grantId/vesting-events/:eventId
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideBody {
    #[serde(default)]
    pub vest_date: Option<NaiveDate>,
    #[serde(default)]
    pub shares_vested: Option<i64>,
    /// Absent = leave FMV alone; present = upsert (or clear with `null`).
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub fmv_at_vest: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub fmv_currency: Option<Option<String>>,
    #[serde(default)]
    pub clear_override: bool,
    pub expected_updated_at: DateTime<Utc>,
}

/// `PUT /api/v1/grants/:grantId/vesting-events/:eventId`
pub async fn upsert_override(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path((grant_id, event_id)): Path<(Uuid, Uuid)>,
    ip: ClientIp,
    Json(body): Json<OverrideBody>,
) -> Result<Response, AppError> {
    let today = Utc::now().date_naive();

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;

    // Ensure the event belongs to this grant + this user; RLS filters
    // + explicit grant_id predicate.
    let existing = vesting_events::get_event(&mut tx, auth.user_id, event_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if existing.grant_id != grant_id {
        return Err(AppError::NotFound);
    }

    // The clear-override branch reverts date/shares to the algorithmic
    // output and preserves FMV per AC-8.7.1.
    if body.clear_override {
        let had_fmv = existing.fmv_at_vest.is_some();
        // AC-10.5 OCC check before any revert.
        if existing.updated_at != body.expected_updated_at {
            return Err(AppError::Conflict);
        }
        let outcome =
            vesting_events::clear_override(&mut tx, auth.user_id, event_id, today).await?;
        let row = match outcome {
            ClearOutcome::Cleared(row) => row,
            ClearOutcome::NotFound => return Err(AppError::NotFound),
            ClearOutcome::NoAlgorithmicMatch => {
                return Err(AppError::Validation(vec![FieldError {
                    field: "clearOverride".into(),
                    code: "vesting_event.no_algorithmic_match".into(),
                }]));
            }
        };

        let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::VestingEventClearOverride,
            auth.user_id,
            Some(event_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({
                "grant_id": grant_id,
                "cleared_fields": ["vest_date", "shares"],
                "preserved": if had_fmv { vec!["fmv"] } else { vec![] },
            }),
        )
        .await?;
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(row_to_json(&row))).into_response());
    }

    // Future-row immutability: only FMV edits allowed (AC-8.3.1).
    let is_future = existing.vest_date > today;
    if is_future && (body.vest_date.is_some() || body.shares_vested.is_some()) {
        return Err(AppError::Validation(vec![FieldError {
            field: "vestDate".into(),
            code: "vesting_event.future_row.immutable_schedule".into(),
        }]));
    }

    // Per-field validation.
    let mut errors: Vec<FieldError> = Vec::new();
    if let Some(d) = body.vest_date {
        // `<= today + 365 days` hard reject (AC-8.2.2).
        if (d - today).num_days() > MAX_FUTURE_DAYS {
            errors.push(FieldError {
                field: "vestDate".into(),
                code: "vesting_event.out_of_window".into(),
            });
        }
    }
    if let Some(s) = body.shares_vested {
        if s <= 0 {
            errors.push(FieldError {
                field: "sharesVested".into(),
                code: "must_be_positive".into(),
            });
        }
    }
    if let Some(Some(ref fmv)) = body.fmv_at_vest {
        if fmv.len() > MAX_PRICE_LEN {
            errors.push(FieldError {
                field: "fmvAtVest".into(),
                code: "length".into(),
            });
        } else {
            match fmv.parse::<f64>() {
                Ok(v) if v > 0.0 => {}
                _ => errors.push(FieldError {
                    field: "fmvAtVest".into(),
                    code: "vesting_event.fmv_pair_incoherent".into(),
                }),
            }
        }
    }
    if let Some(Some(ref cur)) = body.fmv_currency {
        if !CURRENCIES.contains(&cur.as_str()) {
            errors.push(FieldError {
                field: "fmvCurrency".into(),
                code: "unsupported".into(),
            });
        }
    }
    // AC-8.2.4: fmv_at_vest and fmv_currency must be set together (or both
    // cleared together). The repo's CHECK enforces this at the DB layer;
    // surfacing here as a 422 keeps the error envelope consistent.
    if let (Some(f), Some(c)) = (&body.fmv_at_vest, &body.fmv_currency) {
        let f_none = f.is_none();
        let c_none = c.is_none();
        if f_none != c_none {
            errors.push(FieldError {
                field: "fmvAtVest".into(),
                code: "vesting_event.fmv_pair_incoherent".into(),
            });
        }
    }
    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }

    // Compose the patch.
    let patch = VestingEventOverridePatch {
        vest_date: body.vest_date,
        shares_vested_this_event: body.shares_vested.map(|s| s.saturating_mul(SHARES_SCALE)),
        fmv_at_vest: body.fmv_at_vest.clone(),
        fmv_currency: body.fmv_currency.clone(),
    };

    // Fields-changed allowlist; computed from the patch so audit is
    // per-field precise (AC-8.10.1).
    let mut fields_changed: Vec<&'static str> = Vec::new();
    if patch.vest_date.is_some() {
        fields_changed.push("vest_date");
    }
    if patch.shares_vested_this_event.is_some() {
        fields_changed.push("shares");
    }
    if patch.fmv_at_vest.is_some() || patch.fmv_currency.is_some() {
        fields_changed.push("fmv");
    }
    if fields_changed.is_empty() {
        // AC-8.10.1: empty `fields_changed` → no audit, no DB write.
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(row_to_json(&existing))).into_response());
    }

    let outcome = vesting_events::apply_override(
        &mut tx,
        auth.user_id,
        event_id,
        &patch,
        body.expected_updated_at,
    )
    .await?;
    let row = match outcome {
        OverrideOutcome::Applied(r) => r,
        OverrideOutcome::Conflict => return Err(AppError::Conflict),
    };

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::VestingEventOverride,
        auth.user_id,
        Some(event_id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "grant_id": grant_id,
            "fields_changed": fields_changed,
        }),
    )
    .await?;
    tx.commit().await?;

    Ok((StatusCode::OK, Json(row_to_json(&row))).into_response())
}

// ---------------------------------------------------------------------------
// POST /grants/:grantId/vesting-events/bulk-fmv
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkFmvBody {
    pub fmv: String,
    pub currency: String,
}

/// `POST /api/v1/grants/:grantId/vesting-events/bulk-fmv`
pub async fn bulk_fmv(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
    ip: ClientIp,
    Json(body): Json<BulkFmvBody>,
) -> Result<Response, AppError> {
    // Validate.
    let mut errors: Vec<FieldError> = Vec::new();
    match body.fmv.parse::<f64>() {
        Ok(v) if v > 0.0 => {}
        _ => errors.push(FieldError {
            field: "fmv".into(),
            code: "must_be_positive".into(),
        }),
    }
    if !CURRENCIES.contains(&body.currency.as_str()) {
        errors.push(FieldError {
            field: "currency".into(),
            code: "unsupported".into(),
        });
    }
    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // Confirm grant exists + is owned (AC-10.3: cross-tenant → 404).
    let _grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let outcome =
        vesting_events::bulk_fill_fmv(&mut tx, auth.user_id, grant_id, &body.fmv, &body.currency)
            .await?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::VestingEventBulkFmv,
        auth.user_id,
        Some(grant_id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "grant_id": grant_id,
            "applied_count": outcome.applied_count,
            "skipped_count": outcome.skipped_count,
        }),
    )
    .await?;
    tx.commit().await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "appliedCount": outcome.applied_count,
            "skippedCount": outcome.skipped_count,
        })),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn row_to_json(r: &VestingEventRow) -> serde_json::Value {
    json!({
        "id": r.id,
        "grantId": r.grant_id,
        "vestDate": r.vest_date.format("%Y-%m-%d").to_string(),
        "sharesVestedThisEvent": (r.shares_vested_this_event / SHARES_SCALE).to_string(),
        "sharesVestedThisEventScaled": r.shares_vested_this_event,
        "cumulativeSharesVested": (r.cumulative_shares_vested / SHARES_SCALE).to_string(),
        "fmvAtVest": r.fmv_at_vest,
        "fmvCurrency": r.fmv_currency,
        "isUserOverride": r.is_user_override,
        "updatedAt": r.updated_at,
    })
}

/// Custom deserializer: distinguishes "field absent" (None) from
/// "field present but null" (Some(None)). serde's default for
/// `Option<Option<T>>` treats both as None.
fn deserialize_optional_nullable_string<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    Option::<Option<String>>::deserialize(d)
}
