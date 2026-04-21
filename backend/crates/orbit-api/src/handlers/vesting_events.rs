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
    /// Whole-or-fractional share count. Accepted as either a JSON number
    /// (whole shares — legacy) or a decimal string with up to 4 dp
    /// (preserves fractional precision on round-trip via `SHARES_SCALE`,
    /// matches the `fmvAtVest` convention). The handler normalizes both
    /// to scaled-i64 [`Shares`] before reaching the repo.
    #[serde(default, deserialize_with = "deserialize_optional_shares_input")]
    pub shares_vested: Option<SharesInput>,
    /// Absent = leave FMV alone; present = upsert (or clear with `null`).
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub fmv_at_vest: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub fmv_currency: Option<Option<String>>,
    #[serde(default)]
    pub clear_override: bool,
    pub expected_updated_at: DateTime<Utc>,
}

/// Internal representation of the `sharesVested` JSON value before
/// per-field validation. `Integer` is the legacy whole-shares form;
/// `Decimal` carries up to 4 dp of precision (truncated beyond).
#[derive(Debug, Clone)]
pub enum SharesInput {
    Integer(i64),
    Decimal(String),
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
    // output and preserves FMV per AC-8.7.1. OCC (AC-10.5) is enforced
    // inside the repo's UPDATE predicate — a stale `expected_updated_at`
    // resolves as `ClearOutcome::Conflict` and surfaces as 409
    // `resource.stale_client_state`, same as `apply_override`.
    if body.clear_override {
        let had_fmv = existing.fmv_at_vest.is_some();
        let outcome = vesting_events::clear_override(
            &mut tx,
            auth.user_id,
            event_id,
            today,
            body.expected_updated_at,
        )
        .await?;
        let row = match outcome {
            ClearOutcome::Cleared(row) => row,
            ClearOutcome::NotFound => return Err(AppError::NotFound),
            ClearOutcome::Conflict => return Err(AppError::Conflict),
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
    let shares_scaled: Option<i64> = match &body.shares_vested {
        None => None,
        Some(SharesInput::Integer(n)) => {
            if *n <= 0 {
                errors.push(FieldError {
                    field: "sharesVested".into(),
                    code: "must_be_positive".into(),
                });
                None
            } else {
                Some(n.saturating_mul(SHARES_SCALE))
            }
        }
        Some(SharesInput::Decimal(s)) => match parse_shares_decimal(s) {
            Some(scaled) if scaled > 0 => Some(scaled),
            _ => {
                errors.push(FieldError {
                    field: "sharesVested".into(),
                    code: "must_be_positive".into(),
                });
                None
            }
        },
    };
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

    // Compose the patch. `shares_scaled` is the handler-normalized
    // scaled-i64 form; per-field validation above already rejected
    // non-positive or malformed inputs.
    let patch = VestingEventOverridePatch {
        vest_date: body.vest_date,
        shares_vested_this_event: shares_scaled,
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

/// Custom deserializer: accepts either a JSON integer (whole shares —
/// legacy clients) or a JSON string containing a decimal (up to 4 dp —
/// the new fractional form used by the frontend editor). Strings with
/// more than 4 dp are truncated at the scaling step, not here.
fn deserialize_optional_shares_input<'de, D>(d: D) -> Result<Option<SharesInput>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let raw: Option<serde_json::Value> = Option::deserialize(d)?;
    Ok(match raw {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .map(SharesInput::Integer)
            .or_else(|| n.as_f64().map(|f| SharesInput::Decimal(f.to_string()))),
        Some(serde_json::Value::String(s)) => Some(SharesInput::Decimal(s)),
        Some(other) => {
            return Err(D::Error::custom(format!(
                "sharesVested must be a number or decimal string, got {other:?}",
            )));
        }
    })
}

/// Parse a decimal string like "12.3400" into scaled-i64 shares. The
/// fraction is truncated (not rounded) at 4 dp. Returns `None` on any
/// parse failure or on a value that would overflow `i64` after scaling.
fn parse_shares_decimal(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Allow a single leading sign for robustness; the caller rejects
    // non-positive values separately.
    let (sign, rest) = match trimmed.as_bytes().first() {
        Some(b'+') => (1i64, &trimmed[1..]),
        Some(b'-') => (-1i64, &trimmed[1..]),
        _ => (1i64, trimmed),
    };
    if rest.is_empty() {
        return None;
    }
    let (int_part, frac_part) = match rest.find('.') {
        Some(idx) => (&rest[..idx], &rest[idx + 1..]),
        None => (rest, ""),
    };
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    // Take up to 4 frac digits, right-pad with zeros so "12.34" → "3400".
    let mut frac = frac_part.to_string();
    if frac.len() > 4 {
        frac.truncate(4);
    }
    while frac.len() < 4 {
        frac.push('0');
    }
    let int_val: i64 = int_part.parse().ok()?;
    let frac_val: i64 = frac.parse().ok()?;
    let scaled = int_val
        .checked_mul(SHARES_SCALE)?
        .checked_add(frac_val)?
        .checked_mul(sign)?;
    Some(scaled)
}
