//! Vesting-event override endpoints (Slice 3 T29, extended in Slice 3b T38).
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
//! Audit allowlists (SEC-101 + AC-8.10; Slice 3b ADR-018 §5):
//!
//!   * `vesting_event.override` — `{ grant_id, fields_changed }`
//!   * `vesting_event.clear_override` — `{ grant_id, cleared_fields, preserved }`
//!   * `vesting_event.bulk_fmv` — `{ grant_id, applied_count, skipped_count }`
//!   * `vesting_event.sell_to_cover_override` — `{ grant_id, fields_changed }`
//!   * `vesting_event.clear_sell_to_cover_override` — `{ grant_id }`
//!
//! # Slice 3b extension (ADR-018 §3)
//!
//! The PUT body grows five optional keys:
//!
//!   * `taxWithholdingPercent: string | null` — fraction in `[0, 1]`.
//!   * `shareSellPrice: string | null` — per-share sell price.
//!   * `shareSellCurrency: "USD" | "EUR" | "GBP" | null` — defaults to
//!     `fmvCurrency` per AC-7.3.4.
//!   * `clearSellToCoverOverride: bool` — narrow clear (preserves FMV).
//!
//! `clearOverride: true` now clears BOTH tracks (ADR-018 §2 supersede);
//! the DB layer [`orbit_db::vesting_events::clear_override`] is the
//! nuclear revert.
//!
//! **Default-sourcing of `tax_withholding_percent`** per ADR-018 §4 and
//! the AC-7.6.3 resolution: when the key is ABSENT from the body AND
//! `shareSellPrice` is present non-null AND the row is not yet
//! sell-to-cover-overridden AND the user's active
//! `user_tax_preferences` row has `sell_to_cover_enabled = true` with
//! a non-null `rendimiento_del_trabajo_percent`, the handler seeds
//! from the profile. Explicit `null` suppresses seeding (explicit
//! clear); present-non-null takes verbatim.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, NaiveDate, Utc};
use orbit_core::{compute_sell_to_cover, SellToCoverComputeError, SellToCoverInput, SHARES_SCALE};
use orbit_db::vesting_events::{
    self, ClearOutcome, OverrideOutcome, SellToCoverClearOutcome, SellToCoverOutcome,
    SellToCoverOverridePatch, VestingEventOverridePatch, VestingEventRow,
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
const MAX_PERCENT_LEN: usize = 16;

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

    // --- Slice 3b additions (ADR-018 §3) ---
    /// Fraction in `[0, 1]` stringified (e.g., `"0.4500"`). `Some(None)`
    /// = explicit clear (suppresses default-sourcing per AC-7.6.3);
    /// `None` = key absent in body (default-sourcing may fire).
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub tax_withholding_percent: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub share_sell_price: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub share_sell_currency: Option<Option<String>>,
    /// Narrow revert: clears only the sell-to-cover triplet; preserves
    /// FMV, vest_date, shares, and the Slice-3 override flag.
    #[serde(default)]
    pub clear_sell_to_cover_override: bool,
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

    // Mixed-body rejections — fire before we even open a tx so we don't
    // waste a connection on a bad body (ADR-018 §3).
    if body.clear_override
        && (body.vest_date.is_some()
            || body.shares_vested.is_some()
            || body.fmv_at_vest.is_some()
            || body.fmv_currency.is_some()
            || body.tax_withholding_percent.is_some()
            || body.share_sell_price.is_some()
            || body.share_sell_currency.is_some()
            || body.clear_sell_to_cover_override)
    {
        return Err(AppError::Validation(vec![FieldError {
            field: "clearOverride".into(),
            code: "vesting_event.clear_conflict.full".into(),
        }]));
    }
    if body.clear_sell_to_cover_override
        && (body.tax_withholding_percent.is_some()
            || body.share_sell_price.is_some()
            || body.share_sell_currency.is_some())
    {
        return Err(AppError::Validation(vec![FieldError {
            field: "clearSellToCoverOverride".into(),
            code: "vesting_event.clear_conflict.narrow".into(),
        }]));
    }

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;

    // Ensure the event belongs to this grant + this user; RLS filters
    // + explicit grant_id predicate.
    let existing = vesting_events::get_event(&mut tx, auth.user_id, event_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if existing.grant_id != grant_id {
        return Err(AppError::NotFound);
    }

    // --- Branch 1: full clear (ADR-018 §2 supersede — clears both tracks).
    if body.clear_override {
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
        // Two audit rows in deterministic order per ADR-018 §5 +
        // AC-7.7.3. The Slice-3 `cleared_fields` array now always
        // includes "fmv" (it's always cleared on full-clear);
        // `preserved` is always `[]`.
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::VestingEventClearOverride,
            auth.user_id,
            Some(event_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({
                "grant_id": grant_id,
                "cleared_fields": ["vest_date", "shares", "fmv"],
                "preserved": Vec::<&str>::new(),
            }),
        )
        .await?;
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::VestingEventClearSellToCoverOverride,
            auth.user_id,
            Some(event_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({ "grant_id": grant_id }),
        )
        .await?;
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(row_to_json(&row))).into_response());
    }

    // --- Branch 2: narrow clear (sell-to-cover only).
    if body.clear_sell_to_cover_override {
        let outcome = vesting_events::clear_sell_to_cover_override(
            &mut tx,
            auth.user_id,
            event_id,
            body.expected_updated_at,
        )
        .await?;
        let row = match outcome {
            SellToCoverClearOutcome::Cleared(row) => row,
            SellToCoverClearOutcome::NotFound => return Err(AppError::NotFound),
            SellToCoverClearOutcome::Conflict => return Err(AppError::Conflict),
        };
        let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::VestingEventClearSellToCoverOverride,
            auth.user_id,
            Some(event_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({ "grant_id": grant_id }),
        )
        .await?;
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(row_to_json(&row))).into_response());
    }

    // --- Branch 3: forward write (Slice-3 FMV track + Slice-3b sell-to-cover
    // track, composed).

    // Future-row immutability: only FMV-or-sell-to-cover edits allowed
    // (AC-8.3.1; Slice-3b extension: sell-to-cover is an FMV-adjacent
    // surface that is legal on future rows only when vest_date/shares
    // are not in the body).
    let is_future = existing.vest_date > today;
    if is_future && (body.vest_date.is_some() || body.shares_vested.is_some()) {
        return Err(AppError::Validation(vec![FieldError {
            field: "vestDate".into(),
            code: "vesting_event.future_row.immutable_schedule".into(),
        }]));
    }

    // Per-field validation (Slice-3 + Slice-3b).
    let mut errors: Vec<FieldError> = Vec::new();
    if let Some(d) = body.vest_date {
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
    // AC-8.2.4 FMV pair coherence (Slice-3).
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

    // Slice-3b per-field validation.
    if let Some(Some(ref pct)) = body.tax_withholding_percent {
        if pct.len() > MAX_PERCENT_LEN {
            errors.push(FieldError {
                field: "taxWithholdingPercent".into(),
                code: "length".into(),
            });
        } else {
            match pct.parse::<f64>() {
                Ok(v) if v.is_finite() && (0.0..=1.0).contains(&v) => {}
                _ => errors.push(FieldError {
                    field: "taxWithholdingPercent".into(),
                    code: "vesting_event.sell_to_cover.percent_out_of_range".into(),
                }),
            }
        }
    }
    if let Some(Some(ref price)) = body.share_sell_price {
        if price.len() > MAX_PRICE_LEN {
            errors.push(FieldError {
                field: "shareSellPrice".into(),
                code: "length".into(),
            });
        } else {
            match price.parse::<f64>() {
                Ok(v) if v > 0.0 => {}
                _ => errors.push(FieldError {
                    field: "shareSellPrice".into(),
                    code: "must_be_positive".into(),
                }),
            }
        }
    }
    if let Some(Some(ref cur)) = body.share_sell_currency {
        if !CURRENCIES.contains(&cur.as_str()) {
            errors.push(FieldError {
                field: "shareSellCurrency".into(),
                code: "unsupported".into(),
            });
        }
    }

    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }

    // --- Slice 3b default-sourcing of tax_withholding_percent (ADR-018 §4).
    //
    // The one-shot rule: seed from user_tax_preferences.rendimiento when
    //   (a) the row is not yet sell-to-cover-overridden (one-shot),
    //   (b) body.shareSellPrice is present non-null,
    //   (c) body.taxWithholdingPercent KEY is ABSENT (not explicit null),
    //   (d) active tax-preferences row has sell_to_cover_enabled = true
    //       and a non-null rendimiento.
    //
    // `body.tax_withholding_percent` is:
    //   None            → key absent (eligible for seeding)
    //   Some(None)      → explicit null (NOT eligible; explicit clear)
    //   Some(Some(v))   → explicit value (no seed needed)
    let mut tax_patch = body.tax_withholding_percent.clone();
    let wants_sell_to_cover_write = matches!(body.share_sell_price, Some(Some(_)));
    if tax_patch.is_none() && wants_sell_to_cover_write && !existing.is_sell_to_cover_override {
        let prefs = orbit_db::user_tax_preferences::current(&mut tx, auth.user_id).await?;
        if let Some(p) = prefs {
            if p.sell_to_cover_enabled && p.rendimiento_del_trabajo_percent.is_some() {
                tax_patch = Some(Some(p.rendimiento_del_trabajo_percent.unwrap()));
            }
        }
    }

    // --- Slice 3b currency defaulting: shareSellCurrency defaults to
    // fmvCurrency when the sell-to-cover write is happening but the
    // client omitted (or explicitly nulled) the currency. AC-7.3.4.
    let mut sell_currency_patch = body.share_sell_currency.clone();
    if wants_sell_to_cover_write && !matches!(sell_currency_patch, Some(Some(_))) {
        // Prefer the body's fmvCurrency (if the user is updating it in
        // the same save), else fall back to the row's existing
        // fmv_currency.
        let src_from_body = match &body.fmv_currency {
            Some(Some(s)) => Some(s.clone()),
            _ => None,
        };
        let src = src_from_body.or_else(|| existing.fmv_currency.clone());
        sell_currency_patch = src.map(Some);
    }

    // --- Slice 3b triplet-coherence + currency-match checks. These
    // run AFTER default-sourcing so the eventual row's state is what
    // gets validated, not just the body.
    //
    // Effective triplet after applying the patch to the existing row:
    //   - if body carries a Some(_) (non-null), use it;
    //   - if body carries Some(None), the column will be null;
    //   - if body carries None (absent), the column keeps its
    //     existing value.
    let effective_tax_present =
        patch_effective_has_value(&tax_patch, &existing.tax_withholding_percent);
    let effective_price_present =
        patch_effective_has_value(&body.share_sell_price, &existing.share_sell_price);
    let effective_currency_present =
        patch_effective_has_value(&sell_currency_patch, &existing.share_sell_currency);

    let effective_count = [
        effective_tax_present,
        effective_price_present,
        effective_currency_present,
    ]
    .iter()
    .filter(|&&x| x)
    .count();

    if effective_count != 0 && effective_count != 3 {
        return Err(AppError::Validation(vec![FieldError {
            field: "shareSellPrice".into(),
            code: "vesting_event.sell_to_cover.triplet_incomplete".into(),
        }]));
    }

    // Currency equality when the triplet is populated: the final
    // fmv_currency must equal the final share_sell_currency (ADR-018
    // §2 currency policy). Pull the effective values.
    if effective_count == 3 {
        let final_fmv_currency = match &body.fmv_currency {
            Some(v) => v.clone(),
            None => existing.fmv_currency.clone(),
        };
        let final_sell_currency = match &sell_currency_patch {
            Some(v) => v.clone(),
            None => existing.share_sell_currency.clone(),
        };
        match (final_fmv_currency, final_sell_currency) {
            (Some(f), Some(s)) if f == s => {}
            (Some(_), None) | (None, Some(_)) | (None, None) => {
                // Triplet says present but fmv isn't — this means the
                // user is submitting sell-to-cover without an fmv on
                // the row or in the body. Surface a dedicated 422 per
                // ADR-018 §3.
                return Err(AppError::Validation(vec![FieldError {
                    field: "shareSellPrice".into(),
                    code: "vesting_event.sell_to_cover.requires_fmv".into(),
                }]));
            }
            (Some(_), Some(_)) => {
                return Err(AppError::Validation(vec![FieldError {
                    field: "shareSellCurrency".into(),
                    code: "vesting_event.sell_to_cover.currency_mismatch".into(),
                }]));
            }
        }
    }

    // --- Compose patches and apply. Both tracks ride inside this one
    // tx so audit + DB state land atomically (T25 / S1).
    let fmv_patch = VestingEventOverridePatch {
        vest_date: body.vest_date,
        shares_vested_this_event: shares_scaled,
        fmv_at_vest: body.fmv_at_vest.clone(),
        fmv_currency: body.fmv_currency.clone(),
    };

    let sell_patch = SellToCoverOverridePatch {
        tax_withholding_percent: tax_patch.clone(),
        share_sell_price: body.share_sell_price.clone(),
        share_sell_currency: sell_currency_patch.clone(),
    };

    // Decide whether each track has anything to write.
    let fmv_track_fields: Vec<&'static str> = {
        let mut v = Vec::new();
        if fmv_patch.vest_date.is_some() {
            v.push("vest_date");
        }
        if fmv_patch.shares_vested_this_event.is_some() {
            v.push("shares");
        }
        if fmv_patch.fmv_at_vest.is_some() || fmv_patch.fmv_currency.is_some() {
            v.push("fmv");
        }
        v
    };
    let sell_track_fields: Vec<&'static str> = {
        let mut v = Vec::new();
        if sell_patch.tax_withholding_percent.is_some() {
            v.push("tax_percent");
        }
        if sell_patch.share_sell_price.is_some() {
            v.push("sell_price");
        }
        if sell_patch.share_sell_currency.is_some() {
            v.push("sell_currency");
        }
        v
    };

    if fmv_track_fields.is_empty() && sell_track_fields.is_empty() {
        // Nothing to write. Echo the existing row; no audit.
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(row_to_json(&existing))).into_response());
    }

    // Track the token we pass to the second UPDATE if both tracks
    // need to write. The first write advances `updated_at` via the
    // trigger; the second write must predicate on the new token.
    let mut token = body.expected_updated_at;
    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    let mut latest_row: Option<VestingEventRow> = None;

    if !fmv_track_fields.is_empty() {
        let outcome =
            vesting_events::apply_override(&mut tx, auth.user_id, event_id, &fmv_patch, token)
                .await?;
        let row = match outcome {
            OverrideOutcome::Applied(r) => r,
            OverrideOutcome::Conflict => return Err(AppError::Conflict),
        };
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::VestingEventOverride,
            auth.user_id,
            Some(event_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({
                "grant_id": grant_id,
                "fields_changed": fmv_track_fields,
            }),
        )
        .await?;
        token = row.updated_at;
        latest_row = Some(row);
    }

    if !sell_track_fields.is_empty() {
        let outcome = vesting_events::apply_sell_to_cover_override(
            &mut tx,
            auth.user_id,
            event_id,
            &sell_patch,
            token,
        )
        .await?;
        let row = match outcome {
            SellToCoverOutcome::Applied(r) => r,
            SellToCoverOutcome::NotFound => return Err(AppError::NotFound),
            SellToCoverOutcome::Conflict => return Err(AppError::Conflict),
        };
        audit::record_wizard_in_tx(
            tx.as_executor(),
            WizardAction::VestingEventSellToCoverOverride,
            auth.user_id,
            Some(event_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({
                "grant_id": grant_id,
                "fields_changed": sell_track_fields,
            }),
        )
        .await?;
        latest_row = Some(row);
    }

    // Validate computed derived values if the sell-to-cover triplet is
    // complete — surfacing compute errors as envelope 422s before we
    // commit so a typo'd tax+sell pair doesn't persist.
    let committed_row = latest_row.expect("at least one track wrote a row");
    if let Some(Err(err)) = try_compute_derived(&committed_row) {
        return Err(map_compute_error(err));
    }

    tx.commit().await?;
    Ok((StatusCode::OK, Json(row_to_json(&committed_row))).into_response())
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

/// Render a full `VestingEventRow` into the Slice-3 + Slice-3b JSON
/// envelope, including derived sell-to-cover values when the triplet
/// is populated (Slice-3b AC-7.8 §"dialog response shape").
pub(crate) fn row_to_json(r: &VestingEventRow) -> serde_json::Value {
    let derived = derived_values_for_row(r);
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
        // Slice 3b columns
        "taxWithholdingPercent": r.tax_withholding_percent,
        "shareSellPrice": r.share_sell_price,
        "shareSellCurrency": r.share_sell_currency,
        "isSellToCoverOverride": r.is_sell_to_cover_override,
        "sellToCoverOverriddenAt": r.sell_to_cover_overridden_at,
        // Slice 3b derived values (null when triplet incomplete)
        "grossAmount": derived.as_ref().and_then(|d| d.gross.clone()),
        "sharesSoldForTaxes": derived.as_ref().and_then(|d| d.sold.clone()),
        "netSharesDelivered": derived.as_ref().and_then(|d| d.net.clone()),
        "cashWithheld": derived.as_ref().and_then(|d| d.cash.clone()),
    })
}

/// String-rendered derived values for a row, or `None` when the
/// sell-to-cover triplet is incomplete / the compute errored.
struct DerivedValues {
    gross: Option<String>,
    sold: Option<String>,
    net: Option<String>,
    cash: Option<String>,
}

fn derived_values_for_row(r: &VestingEventRow) -> Option<DerivedValues> {
    let input = build_compute_input(r)?;
    let result = compute_sell_to_cover(input).ok()?;
    Some(DerivedValues {
        gross: Some(scaled_decimal_string(result.gross_amount_scaled)),
        sold: Some(scaled_decimal_string(result.shares_sold_for_taxes_scaled)),
        net: Some(scaled_decimal_string(result.net_shares_delivered_scaled)),
        cash: Some(scaled_decimal_string(result.cash_withheld_scaled)),
    })
}

/// Build a [`SellToCoverInput`] from a `VestingEventRow` when the
/// triplet + FMV are all populated. Returns `None` on any missing
/// piece (caller reports `null` derived values).
fn build_compute_input(r: &VestingEventRow) -> Option<SellToCoverInput> {
    let fmv = r.fmv_at_vest.as_deref()?;
    let tax = r.tax_withholding_percent.as_deref()?;
    let price = r.share_sell_price.as_deref()?;
    Some(SellToCoverInput {
        fmv_at_vest_scaled: parse_scaled_numeric(fmv)?,
        shares_vested_scaled: r.shares_vested_this_event,
        tax_withholding_percent_scaled: parse_scaled_numeric(tax)?,
        share_sell_price_scaled: parse_scaled_numeric(price)?,
    })
}

/// Try to compute derived values and surface the error variant when
/// the sell-to-cover triplet is populated and the compute is
/// meaningful. Returns `None` when the triplet is incomplete (derived
/// values stay null without an error).
fn try_compute_derived(r: &VestingEventRow) -> Option<Result<(), SellToCoverComputeError>> {
    let input = build_compute_input(r)?;
    Some(compute_sell_to_cover(input).map(|_| ()))
}

/// Convert a `NUMERIC(x,y)` string like `"0.45"` or `"42.000000"`
/// to scaled-i64 (× `SHARES_SCALE`, truncating beyond 4 dp). Mirrors
/// [`parse_shares_decimal`] for the cross-column numeric story — the
/// scaling is `SHARES_SCALE = 10_000`, so `0.4500` → `4500`. Returns
/// `None` on parse failure or overflow.
fn parse_scaled_numeric(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
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

/// Render a scaled-i64 value as a decimal string with four digits of
/// precision (matches the NUMERIC(20,4) / (5,4) wire convention used
/// elsewhere in the API). Preserves sign for the uncommon negative
/// case.
fn scaled_decimal_string(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let abs = (v as i128).unsigned_abs();
    let scale = SHARES_SCALE as u128;
    let int_part = abs / scale;
    let frac_part = abs % scale;
    format!("{sign}{int_part}.{frac_part:04}")
}

/// Is the effective column value non-null after applying `patch` to
/// `existing`? `None` patch leaves `existing` untouched; `Some(None)`
/// nulls it; `Some(Some(_))` makes it non-null.
fn patch_effective_has_value(patch: &Option<Option<String>>, existing: &Option<String>) -> bool {
    match patch {
        None => existing.is_some(),
        Some(None) => false,
        Some(Some(_)) => true,
    }
}

fn map_compute_error(err: SellToCoverComputeError) -> AppError {
    let (field, code) = match err {
        SellToCoverComputeError::NegativeNetShares => (
            "shareSellPrice",
            "vesting_event.sell_to_cover.negative_net_shares",
        ),
        SellToCoverComputeError::ZeroSellPriceWithPositiveTax => (
            "shareSellPrice",
            "vesting_event.sell_to_cover.zero_sell_price",
        ),
    };
    AppError::Validation(vec![FieldError {
        field: field.into(),
        code: code.into(),
    }])
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
    parse_scaled_numeric(s)
}
