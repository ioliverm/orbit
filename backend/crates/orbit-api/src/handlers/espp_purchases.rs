//! ESPP-purchase endpoints (Slice 2 T21, ADR-016 §5.1).
//!
//! Wires the `orbit_db::espp_purchases` repo to the HTTP layer:
//!
//!   * `POST   /api/v1/grants/:grant_id/espp-purchases`
//!   * `GET    /api/v1/grants/:grant_id/espp-purchases`
//!   * `GET    /api/v1/espp-purchases/:id`
//!   * `PUT    /api/v1/espp-purchases/:id`
//!   * `DELETE /api/v1/espp-purchases/:id`
//!
//! All under the `gated_authed` subtree (onboarding-gated — the parent
//! grant has already been created).
//!
//! Audit-log allowlist (SEC-101): `{ currency, had_lookback, had_discount,
//! notes_lift }` on create/update; `{ currency }` on delete. No FMV, no
//! purchase price, no share count, no raw notes text.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::NaiveDate;
use orbit_core::{Shares, SHARES_SCALE};
use orbit_db::espp_purchases::{EspppPurchase, EspppPurchaseForm};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

const CURRENCIES: &[&str] = &["USD", "EUR", "GBP"];

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RecordEsppPurchaseBody {
    pub offering_date: NaiveDate,
    pub purchase_date: NaiveDate,
    /// Decimal string up to 4 dp; `> 0` (AC-4.2.5).
    #[validate(length(min = 1, max = 32))]
    pub fmv_at_purchase: String,
    /// Decimal string up to 4 dp; `> 0` (AC-4.2.5).
    #[validate(length(min = 1, max = 32))]
    pub purchase_price_per_share: String,
    /// Whole shares; `>= 1` (AC-4.2.4). Scaled to `Shares` before the repo.
    pub shares_purchased: i64,
    /// `USD | EUR | GBP` (AC-4.2.6).
    #[validate(length(equal = 3))]
    pub currency: String,
    #[serde(default)]
    #[validate(length(max = 32))]
    pub fmv_at_offering: Option<String>,
    #[serde(default)]
    #[validate(length(max = 16))]
    pub employer_discount_percent: Option<String>,
    #[serde(default)]
    #[validate(length(max = 2048))]
    pub notes: Option<String>,
    /// AC-4.2.8 soft-warn override. When `true` the handler skips the
    /// duplicate-purchase guard and permits a second row with the same
    /// `(offering_date, purchase_date, shares_purchased)` triple.
    #[serde(default)]
    pub force_duplicate: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsppPurchaseDto {
    pub id: Uuid,
    pub grant_id: Uuid,
    pub offering_date: NaiveDate,
    pub purchase_date: NaiveDate,
    /// Decimal string (`NUMERIC(20,6)::text`).
    pub fmv_at_purchase: String,
    pub purchase_price_per_share: String,
    /// Whole-share integer as a string, mirroring Slice-1 `GrantDto`.
    pub shares_purchased: String,
    /// Scaled-i64 (`shares * 10_000`) for clients that want exact arithmetic.
    pub shares_purchased_scaled: Shares,
    pub currency: String,
    pub fmv_at_offering: Option<String>,
    pub employer_discount_percent: Option<String>,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<EspppPurchase> for EsppPurchaseDto {
    fn from(p: EspppPurchase) -> Self {
        let whole = p.shares_purchased / SHARES_SCALE;
        EsppPurchaseDto {
            id: p.id,
            grant_id: p.grant_id,
            offering_date: p.offering_date,
            purchase_date: p.purchase_date,
            fmv_at_purchase: p.fmv_at_purchase,
            purchase_price_per_share: p.purchase_price_per_share,
            shares_purchased: whole.to_string(),
            shares_purchased_scaled: p.shares_purchased,
            currency: p.currency,
            fmv_at_offering: p.fmv_at_offering,
            employer_discount_percent: p.employer_discount_percent,
            notes: p.notes,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/grants/:grant_id/espp-purchases`
pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
    ip: ClientIp,
    Json(body): Json<RecordEsppPurchaseBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    validate_purchase_shape(&body, /* on_update = */ false)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;

    // Serialize first-purchase POSTs on the same grant (T25 / S-2.1).
    // Two concurrent callers would otherwise both observe an empty
    // `existing` and both claim `notes_lift: true` in their audit
    // payloads — stored state is still correct (one lift wins, the
    // second finds `notes = NULL` and no-ops), but the audit log
    // would misreport two lifts. Locking the parent grant row here
    // closes the race. `user_id` guard is defense-in-depth alongside
    // the RLS policy on `grants`.
    sqlx::query("SELECT id FROM grants WHERE id = $1 AND user_id = $2 FOR UPDATE")
        .bind(grant_id)
        .bind(auth.user_id)
        .fetch_optional(tx.as_executor())
        .await?
        .ok_or(AppError::NotFound)?;

    // Ownership + instrument check (ADR-016 §5.1 step 2). RLS already
    // filters cross-tenant rows to NotFound; explicit instrument check
    // lets us surface a clean 422 before the trigger fires.
    let grant = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if grant.instrument != "espp" {
        return Err(AppError::Validation(vec![FieldError {
            field: "grantId".into(),
            code: "not_espp".into(),
        }]));
    }

    // Notes-lift on first purchase (ADR-016 §2, AC-4.5.1).
    let existing =
        orbit_db::espp_purchases::list_for_grant(&mut tx, auth.user_id, grant_id).await?;
    let is_first = existing.is_empty();
    let mut notes_lift = false;
    let lift = if is_first {
        let lifted = orbit_db::espp_purchases::migrate_notes_on_first_purchase(
            &mut tx,
            auth.user_id,
            grant_id,
        )
        .await?;
        if lifted.is_some() {
            notes_lift = true;
        }
        lifted
    } else {
        None
    };

    // Duplicate-purchase soft-warn (AC-4.2.8). Shares come in as whole
    // integers; compare against the scaled form the repo stored.
    let shares_scaled = body.shares_purchased.saturating_mul(SHARES_SCALE);
    if !body.force_duplicate
        && existing.iter().any(|p| {
            p.offering_date == body.offering_date
                && p.purchase_date == body.purchase_date
                && p.shares_purchased == shares_scaled
        })
    {
        return Err(AppError::Validation(vec![FieldError {
            field: "purchase".into(),
            code: "duplicate".into(),
        }]));
    }

    // Merge: user-supplied discount wins; else lifted value.
    let effective_discount = body
        .employer_discount_percent
        .clone()
        .or_else(|| lift.as_ref().map(|l| l.lifted_discount_percent.clone()));

    let form = EspppPurchaseForm {
        grant_id,
        offering_date: body.offering_date,
        purchase_date: body.purchase_date,
        fmv_at_purchase: body.fmv_at_purchase.clone(),
        purchase_price_per_share: body.purchase_price_per_share.clone(),
        shares_purchased: shares_scaled,
        currency: body.currency.clone(),
        fmv_at_offering: body.fmv_at_offering.clone(),
        employer_discount_percent: effective_discount.clone(),
        notes: body.notes.clone(),
    };
    let purchase = insert_or_map_check_violation(
        orbit_db::espp_purchases::create(&mut tx, auth.user_id, &form).await,
    )?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::EsppPurchaseCreate,
        auth.user_id,
        Some(purchase.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "currency": purchase.currency,
            "had_lookback": purchase.fmv_at_offering.is_some(),
            "had_discount": effective_discount.is_some(),
            "notes_lift": notes_lift,
        }),
    )
    .await?;
    tx.commit().await?;

    let dto: EsppPurchaseDto = purchase.into();
    Ok((
        StatusCode::CREATED,
        Json(json!({ "purchase": dto, "migratedFromNotes": notes_lift })),
    )
        .into_response())
}

/// `GET /api/v1/grants/:grant_id/espp-purchases`
pub async fn list_for_grant(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(grant_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // RLS + explicit ownership: 404 if grant is missing or not owned.
    let _ = orbit_db::grants::get_grant(&mut tx, auth.user_id, grant_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let rows = orbit_db::espp_purchases::list_for_grant(&mut tx, auth.user_id, grant_id).await?;
    tx.commit().await?;

    let dtos: Vec<EsppPurchaseDto> = rows.into_iter().map(EsppPurchaseDto::from).collect();
    Ok((StatusCode::OK, Json(json!({ "purchases": dtos }))).into_response())
}

/// `GET /api/v1/espp-purchases/:id`
pub async fn get_one(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let purchase = orbit_db::espp_purchases::get_by_id(&mut tx, auth.user_id, id)
        .await?
        .ok_or(AppError::NotFound)?;
    tx.commit().await?;

    let dto: EsppPurchaseDto = purchase.into();
    Ok((StatusCode::OK, Json(json!({ "purchase": dto }))).into_response())
}

/// `PUT /api/v1/espp-purchases/:id`
pub async fn update(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(id): Path<Uuid>,
    ip: ClientIp,
    Json(body): Json<RecordEsppPurchaseBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;
    validate_purchase_shape(&body, /* on_update = */ true)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    // Ownership: 404 if the purchase is missing. The repo's UPDATE
    // preserves `grant_id`, so we don't need the grant row here.
    let existing = orbit_db::espp_purchases::get_by_id(&mut tx, auth.user_id, id)
        .await?
        .ok_or(AppError::NotFound)?;

    let shares_scaled = body.shares_purchased.saturating_mul(SHARES_SCALE);
    let form = EspppPurchaseForm {
        grant_id: existing.grant_id,
        offering_date: body.offering_date,
        purchase_date: body.purchase_date,
        fmv_at_purchase: body.fmv_at_purchase.clone(),
        purchase_price_per_share: body.purchase_price_per_share.clone(),
        shares_purchased: shares_scaled,
        currency: body.currency.clone(),
        fmv_at_offering: body.fmv_at_offering.clone(),
        employer_discount_percent: body.employer_discount_percent.clone(),
        notes: body.notes.clone(),
    };
    let purchase = match orbit_db::espp_purchases::update(&mut tx, auth.user_id, id, &form).await {
        Ok(p) => p,
        Err(sqlx::Error::RowNotFound) => return Err(AppError::NotFound),
        Err(e) => return Err(map_sqlx_err(e)),
    };

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::EsppPurchaseUpdate,
        auth.user_id,
        Some(purchase.id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({
            "currency": purchase.currency,
            "had_lookback": purchase.fmv_at_offering.is_some(),
            "had_discount": purchase.employer_discount_percent.is_some(),
            "notes_lift": false,
        }),
    )
    .await?;
    tx.commit().await?;

    let dto: EsppPurchaseDto = purchase.into();
    Ok((StatusCode::OK, Json(json!({ "purchase": dto }))).into_response())
}

/// `DELETE /api/v1/espp-purchases/:id`
pub async fn delete(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    Path(id): Path<Uuid>,
    ip: ClientIp,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;
    let existing = orbit_db::espp_purchases::get_by_id(&mut tx, auth.user_id, id)
        .await?
        .ok_or(AppError::NotFound)?;
    orbit_db::espp_purchases::delete(&mut tx, auth.user_id, id).await?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_wizard_in_tx(
        tx.as_executor(),
        WizardAction::EsppPurchaseDelete,
        auth.user_id,
        Some(id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({ "currency": existing.currency }),
    )
    .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn validate_purchase_shape(
    body: &RecordEsppPurchaseBody,
    _on_update: bool,
) -> Result<(), AppError> {
    let mut errors: Vec<FieldError> = Vec::new();

    if !CURRENCIES.contains(&body.currency.as_str()) {
        errors.push(FieldError {
            field: "currency".into(),
            code: "unsupported".into(),
        });
    }
    if body.shares_purchased <= 0 {
        errors.push(FieldError {
            field: "sharesPurchased".into(),
            code: "must_be_positive".into(),
        });
    }
    if body.purchase_date < body.offering_date {
        errors.push(FieldError {
            field: "purchaseDate".into(),
            code: "before_offering_date".into(),
        });
    }
    // Positive-decimal guard for FMV + price. We only assert non-blank +
    // first-char-looks-numeric — the NUMERIC cast in the repo's INSERT
    // translates any malformed string into a 500, which the CI tests
    // catch. A 4-digit-bound regex here would duplicate the DDL CHECK.
    if !looks_positive_decimal(&body.fmv_at_purchase) {
        errors.push(FieldError {
            field: "fmvAtPurchase".into(),
            code: "must_be_positive".into(),
        });
    }
    if !looks_positive_decimal(&body.purchase_price_per_share) {
        errors.push(FieldError {
            field: "purchasePricePerShare".into(),
            code: "must_be_positive".into(),
        });
    }
    if let Some(f) = body.fmv_at_offering.as_deref() {
        if !f.is_empty() && !looks_positive_decimal(f) {
            errors.push(FieldError {
                field: "fmvAtOffering".into(),
                code: "must_be_positive".into(),
            });
        }
    }
    if let Some(p) = body.employer_discount_percent.as_deref() {
        if !p.is_empty() {
            match p.parse::<f64>() {
                Ok(v) if (0.0..=100.0).contains(&v) && v.is_finite() => {}
                _ => errors.push(FieldError {
                    field: "employerDiscountPercent".into(),
                    code: "out_of_range".into(),
                }),
            }
        }
    }

    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }
    Ok(())
}

/// Minimal "looks like a positive decimal" check. Bounces on the obvious
/// garbage without pulling in a regex dep or duplicating the DDL CHECK.
/// The DB's `::numeric` cast is the authority; this is just for a nice 422.
fn looks_positive_decimal(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    let mut seen_digit = false;
    for (i, c) in t.chars().enumerate() {
        match c {
            '+' | '-' if i == 0 => {}
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot => seen_dot = true,
            _ => return false,
        }
    }
    if !seen_digit {
        return false;
    }
    // Reject non-positive values (leading '-' plus any digits).
    if let Some(first) = t.chars().next() {
        if first == '-' {
            return false;
        }
    }
    // Parse for a hard numeric check (zero-or-negative via `<= 0`).
    matches!(t.parse::<f64>(), Ok(v) if v.is_finite() && v > 0.0)
}

/// The `espp_purchases_enforce_grant_instrument_trg` trigger fires when a
/// non-ESPP grant_id sneaks past the handler's pre-check (race, or a stale
/// client). Map its `check_violation` ERRCODE to a clean 422.
fn insert_or_map_check_violation<T>(r: Result<T, sqlx::Error>) -> Result<T, AppError> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(map_sqlx_err(e)),
    }
}

fn map_sqlx_err(e: sqlx::Error) -> AppError {
    if let Some(db_err) = e.as_database_error() {
        if let Some(code) = db_err.code() {
            // 23514 = check_violation; the trigger raises this for a
            // non-ESPP parent grant. 23503 = foreign_key_violation
            // (unreachable given our pre-check but map it defensively).
            if code == "23514" {
                return AppError::Validation(vec![FieldError {
                    field: "grantId".into(),
                    code: "not_espp".into(),
                }]);
            }
        }
    }
    AppError::from(e)
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
// Tests (pure — validator shape). DB round-trips live in the
// integration-tests suite.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> RecordEsppPurchaseBody {
        RecordEsppPurchaseBody {
            offering_date: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
            purchase_date: NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
            fmv_at_purchase: "45.00".into(),
            purchase_price_per_share: "38.25".into(),
            shares_purchased: 100,
            currency: "USD".into(),
            fmv_at_offering: None,
            employer_discount_percent: None,
            notes: None,
            force_duplicate: false,
        }
    }

    #[test]
    fn accepts_valid_body() {
        validate_purchase_shape(&body(), false).expect("ok");
    }

    #[test]
    fn rejects_purchase_before_offering() {
        let mut b = body();
        b.purchase_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let err = validate_purchase_shape(&b, false).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "before_offering_date"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_bad_currency() {
        let mut b = body();
        b.currency = "JPY".into();
        let err = validate_purchase_shape(&b, false).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.field == "currency"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_zero_shares() {
        let mut b = body();
        b.shares_purchased = 0;
        let err = validate_purchase_shape(&b, false).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "must_be_positive"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_negative_fmv() {
        let mut b = body();
        b.fmv_at_purchase = "-1".into();
        let err = validate_purchase_shape(&b, false).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.field == "fmvAtPurchase"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn rejects_out_of_range_discount() {
        let mut b = body();
        b.employer_discount_percent = Some("150".into());
        let err = validate_purchase_shape(&b, false).unwrap_err();
        match err {
            AppError::Validation(v) => {
                assert!(v.iter().any(|f| f.code == "out_of_range"));
            }
            _ => panic!("expected Validation"),
        }
    }

    #[test]
    fn looks_positive_decimal_happy_paths() {
        assert!(looks_positive_decimal("45"));
        assert!(looks_positive_decimal("45.25"));
        assert!(looks_positive_decimal("0.0001"));
        assert!(!looks_positive_decimal(""));
        assert!(!looks_positive_decimal("-1"));
        assert!(!looks_positive_decimal("abc"));
        assert!(!looks_positive_decimal("0"));
    }
}
