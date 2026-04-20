//! `espp_purchases` repository (Slice 2 T20).
//!
//! One row per ESPP purchase window (AC-4.1..AC-4.5). Every query routes
//! through a `&mut Tx` borrowed via [`crate::Tx::for_user`], so RLS scopes
//! the visible row set to the owner (SEC-020..023). The ADR-016-§1
//! `espp_purchases_enforce_grant_instrument_trg` trigger rejects any write
//! whose parent `grant_id` does not point at an `instrument = 'espp'` row
//! — handlers translate the `check_violation` ERRCODE into a 422.
//!
//! # Numeric column bridging
//!
//! `shares_purchased` is `NUMERIC(20,4)` and carried as [`orbit_core::Shares`]
//! (an `i64` scaled by 10,000) — same bridge as [`crate::grants`]. The money
//! columns (`fmv_at_purchase`, `purchase_price_per_share`, `fmv_at_offering`,
//! `employer_discount_percent`) have no arithmetic surface in Slice 2 (the
//! tax engine lands in Slice 4 / 5); they are passed through as
//! `Option<String>` / `String` via `::numeric` casts on write and `::text`
//! casts on read, so a future `rust_decimal` dep can slot in without
//! breaking the Slice-2 boundary. This matches `grants.strike_amount`.
//!
//! Traces to:
//!   - ADR-016 §1 (espp_purchases DDL, all CHECK constraints, the
//!     grant-instrument trigger, touch_updated_at).
//!   - ADR-016 §2 (Slice-1 `grants.notes` lift — see
//!     [`migrate_notes_on_first_purchase`]).
//!   - docs/requirements/slice-2-acceptance-criteria.md §4.

use chrono::NaiveDate;
use orbit_core::Shares;
use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// An `espp_purchases` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspppPurchase {
    pub id: Uuid,
    pub user_id: Uuid,
    pub grant_id: Uuid,
    pub offering_date: NaiveDate,
    pub purchase_date: NaiveDate,
    /// Decimal passthrough (`NUMERIC(20,6)::text`). Always positive.
    pub fmv_at_purchase: String,
    /// Decimal passthrough (`NUMERIC(20,6)::text`). Always positive.
    pub purchase_price_per_share: String,
    /// Scaled-i64 (`orbit_core::Shares`). Convert to whole shares via
    /// `value / orbit_core::SHARES_SCALE` at the JSON boundary.
    pub shares_purchased: Shares,
    /// `USD | EUR | GBP` (DDL CHECK).
    pub currency: String,
    /// Decimal passthrough. `None` when the plan has no lookback leg.
    pub fmv_at_offering: Option<String>,
    /// Decimal passthrough in `[0, 100]`. `None` when unspecified.
    pub employer_discount_percent: Option<String>,
    pub notes: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Input shape for [`create`] / [`update`].
///
/// The handler layer is expected to have translated the raw JSON into this
/// struct and validated every cross-field constraint (`purchase_date >=
/// offering_date`, currency enum, positive numerics, discount in `[0, 100]`).
#[derive(Debug, Clone)]
pub struct EspppPurchaseForm {
    pub grant_id: Uuid,
    pub offering_date: NaiveDate,
    pub purchase_date: NaiveDate,
    pub fmv_at_purchase: String,
    pub purchase_price_per_share: String,
    pub shares_purchased: Shares,
    pub currency: String,
    pub fmv_at_offering: Option<String>,
    pub employer_discount_percent: Option<String>,
    pub notes: Option<String>,
}

/// INSERT a new ESPP purchase owned by `user_id`.
///
/// A `check_violation` from the trigger means the parent grant is not an
/// ESPP; a `foreign_key_violation` means the parent grant does not exist
/// (or is not owned by `user_id` — RLS on `grants` filters it out so the
/// trigger sees `NULL`).
pub async fn create(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    form: &EspppPurchaseForm,
) -> Result<EspppPurchase, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO espp_purchases (
            user_id, grant_id,
            offering_date, purchase_date,
            fmv_at_purchase, purchase_price_per_share,
            shares_purchased, currency,
            fmv_at_offering, employer_discount_percent,
            notes
        )
        VALUES (
            $1, $2,
            $3, $4,
            $5::numeric, $6::numeric,
            $7::numeric / 10000, $8,
            $9::numeric, $10::numeric,
            $11
        )
        RETURNING
            id, user_id, grant_id,
            offering_date, purchase_date,
            fmv_at_purchase::text        AS fmv_at_purchase_text,
            purchase_price_per_share::text AS purchase_price_per_share_text,
            (shares_purchased * 10000)::bigint AS shares_purchased_scaled,
            currency,
            fmv_at_offering::text        AS fmv_at_offering_text,
            employer_discount_percent::text AS employer_discount_percent_text,
            notes, created_at, updated_at
        "#,
    )
    .bind(user_id)
    .bind(form.grant_id)
    .bind(form.offering_date)
    .bind(form.purchase_date)
    .bind(&form.fmv_at_purchase)
    .bind(&form.purchase_price_per_share)
    .bind(form.shares_purchased)
    .bind(&form.currency)
    .bind(form.fmv_at_offering.as_deref())
    .bind(form.employer_discount_percent.as_deref())
    .bind(form.notes.as_deref())
    .fetch_one(tx.as_executor())
    .await?;

    row_to_purchase(&row)
}

/// List purchases for a single grant, most-recent-purchase-date first.
/// RLS scopes the row set to the owner.
pub async fn list_for_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Vec<EspppPurchase>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            id, user_id, grant_id,
            offering_date, purchase_date,
            fmv_at_purchase::text        AS fmv_at_purchase_text,
            purchase_price_per_share::text AS purchase_price_per_share_text,
            (shares_purchased * 10000)::bigint AS shares_purchased_scaled,
            currency,
            fmv_at_offering::text        AS fmv_at_offering_text,
            employer_discount_percent::text AS employer_discount_percent_text,
            notes, created_at, updated_at
          FROM espp_purchases
         WHERE user_id = $1 AND grant_id = $2
         ORDER BY purchase_date DESC, created_at DESC
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_purchase).collect()
}

/// Fetch a single purchase by id, scoped to the owner. Returns `None` when
/// the row does not exist or is not owned by `user_id` (AC-4.3.6: 404 not
/// 403 to avoid existence leaks).
pub async fn get_by_id(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    purchase_id: Uuid,
) -> Result<Option<EspppPurchase>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            id, user_id, grant_id,
            offering_date, purchase_date,
            fmv_at_purchase::text        AS fmv_at_purchase_text,
            purchase_price_per_share::text AS purchase_price_per_share_text,
            (shares_purchased * 10000)::bigint AS shares_purchased_scaled,
            currency,
            fmv_at_offering::text        AS fmv_at_offering_text,
            employer_discount_percent::text AS employer_discount_percent_text,
            notes, created_at, updated_at
          FROM espp_purchases
         WHERE id = $1 AND user_id = $2
        "#,
    )
    .bind(purchase_id)
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_purchase).transpose()
}

/// Full-replace UPDATE on a purchase. Returns `sqlx::Error::RowNotFound`
/// when no row matched (bad id or RLS-filtered).
///
/// Note: `grant_id` is intentionally not in the update surface — moving a
/// purchase between grants is not a supported user action (AC-4.3.4 edits
/// the fact fields only). If a future handler wants that, extend here and
/// the trigger re-fires on `UPDATE OF grant_id`.
pub async fn update(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    purchase_id: Uuid,
    form: &EspppPurchaseForm,
) -> Result<EspppPurchase, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE espp_purchases
           SET offering_date             = $3,
               purchase_date             = $4,
               fmv_at_purchase           = $5::numeric,
               purchase_price_per_share  = $6::numeric,
               shares_purchased          = $7::numeric / 10000,
               currency                  = $8,
               fmv_at_offering           = $9::numeric,
               employer_discount_percent = $10::numeric,
               notes                     = $11
         WHERE id = $1 AND user_id = $2
     RETURNING
            id, user_id, grant_id,
            offering_date, purchase_date,
            fmv_at_purchase::text        AS fmv_at_purchase_text,
            purchase_price_per_share::text AS purchase_price_per_share_text,
            (shares_purchased * 10000)::bigint AS shares_purchased_scaled,
            currency,
            fmv_at_offering::text        AS fmv_at_offering_text,
            employer_discount_percent::text AS employer_discount_percent_text,
            notes, created_at, updated_at
        "#,
    )
    .bind(purchase_id)
    .bind(user_id)
    .bind(form.offering_date)
    .bind(form.purchase_date)
    .bind(&form.fmv_at_purchase)
    .bind(&form.purchase_price_per_share)
    .bind(form.shares_purchased)
    .bind(&form.currency)
    .bind(form.fmv_at_offering.as_deref())
    .bind(form.employer_discount_percent.as_deref())
    .bind(form.notes.as_deref())
    .fetch_one(tx.as_executor())
    .await?;

    row_to_purchase(&row)
}

/// DELETE a purchase. Returns `Ok(())` whether or not a row matched — RLS
/// filters cross-tenant rows and the handler distinguishes "not found"
/// from "not owned" via a prior [`get_by_id`] (AC-4.3.6).
pub async fn delete(tx: &mut Tx<'_>, user_id: Uuid, purchase_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM espp_purchases WHERE id = $1 AND user_id = $2")
        .bind(purchase_id)
        .bind(user_id)
        .execute(tx.as_executor())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Slice-1 `grants.notes` ESPP compromise — non-lossy lift on first purchase
// ---------------------------------------------------------------------------

/// Result of the first-purchase notes lift (ADR-016 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesMigration {
    /// The discount percent lifted out of the JSON (rounded to 2dp at write
    /// time by the caller, who already owns the `NUMERIC(5,2)` binding). The
    /// caller treats this as a *default*, not an override — user-supplied
    /// `employer_discount_percent` wins when both are present.
    pub lifted_discount_percent: String,
    /// The free-text `note` field recovered from the Slice-1 JSON, if any.
    /// `None` means the JSON carried only the discount key, so the
    /// post-lift `grants.notes` should become `NULL`. `Some(s)` means the
    /// JSON also had a user note; the post-lift `grants.notes` becomes
    /// that string verbatim (no JSON wrapping).
    pub preserved_user_note: Option<String>,
}

/// Read `grants.notes` for the given grant and, if it is a Slice-1 ESPP
/// JSON blob (`{"estimated_discount_percent": N}` with an optional
/// `note` string), rewrite the `grants.notes` column per ADR-016 §2 and
/// return the lifted fields for the caller to splice into the pending
/// `espp_purchases` row.
///
/// The lift is a no-op (returns `Ok(None)`) when:
///   - the grant has no notes,
///   - `notes` is not valid JSON (free-text user note from Slice-1),
///   - `notes` is valid JSON but does not carry `estimated_discount_percent`,
///   - the grant does not exist or is not owned by `user_id` (RLS filters).
///
/// The caller is responsible for (a) checking this is the *first* purchase
/// for the grant — the lift guard is on top of this helper, not inside —
/// and (b) for honoring the user's `employer_discount_percent` override if
/// set (ADR-016 §2 "Determinism guarantees" bullet 2).
///
/// Every mutation happens inside the caller's transaction, so a subsequent
/// INSERT failure (e.g., CHECK violation on `shares_purchased`) rolls the
/// `grants.notes` rewrite back atomically.
pub async fn migrate_notes_on_first_purchase(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Option<NotesMigration>, sqlx::Error> {
    // Step 1: read the current `grants.notes`. RLS + `user_id = $2`
    // constrain this to the owner; a cross-tenant or deleted grant
    // returns `None` and the lift no-ops.
    let row = sqlx::query("SELECT notes FROM grants WHERE id = $1 AND user_id = $2")
        .bind(grant_id)
        .bind(user_id)
        .fetch_optional(tx.as_executor())
        .await?;
    let raw_notes: Option<String> = match row {
        Some(r) => r.try_get("notes")?,
        None => return Ok(None),
    };
    let Some(raw) = raw_notes else {
        return Ok(None);
    };

    // Step 2: try to parse as Slice-1 ESPP JSON. Any parse failure is
    // treated as free-text user notes and left alone.
    let Some(parsed) = parse_slice_1_espp_notes(&raw) else {
        return Ok(None);
    };

    // Step 3: rewrite `grants.notes` to the preserved user note (may be
    // NULL), in the same transaction. The UPDATE is scoped to the owner
    // by `user_id = $2` (defense-in-depth alongside RLS).
    sqlx::query("UPDATE grants SET notes = $1 WHERE id = $2 AND user_id = $3")
        .bind(parsed.preserved_user_note.as_deref())
        .bind(grant_id)
        .bind(user_id)
        .execute(tx.as_executor())
        .await?;

    Ok(Some(parsed))
}

/// Parse `{"estimated_discount_percent": N}` (optionally with
/// `"note": "..."`) into a [`NotesMigration`]. Returns `None` for any
/// other shape.
///
/// Slice-1 stored the percent as an integer (see
/// `orbit-api::handlers::grants::merge_notes`), but we accept any numeric
/// value and coerce to a 2-decimal string so the `NUMERIC(5,2)` bind on
/// the purchase insert round-trips cleanly. Handler-level formatting
/// (locale-aware display) is orthogonal.
fn parse_slice_1_espp_notes(raw: &str) -> Option<NotesMigration> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let obj = v.as_object()?;
    let pct = obj.get("estimated_discount_percent")?;
    // Accept int or float, reject strings / bools / null / nested.
    let pct_f: f64 = pct.as_f64()?;
    if !pct_f.is_finite() || !(0.0..=100.0).contains(&pct_f) {
        return None;
    }
    // Format to 2dp; NUMERIC(5,2) stores the same.
    let lifted = format!("{pct_f:.2}");

    // Optional user `note` — if absent, `grants.notes` lands `NULL`. Any
    // non-string value is treated as "absent" and logged via None.
    let preserved = obj
        .get("note")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    Some(NotesMigration {
        lifted_discount_percent: lifted,
        preserved_user_note: preserved,
    })
}

fn row_to_purchase(row: &sqlx::postgres::PgRow) -> Result<EspppPurchase, sqlx::Error> {
    Ok(EspppPurchase {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        grant_id: row.try_get("grant_id")?,
        offering_date: row.try_get("offering_date")?,
        purchase_date: row.try_get("purchase_date")?,
        fmv_at_purchase: row.try_get("fmv_at_purchase_text")?,
        purchase_price_per_share: row.try_get("purchase_price_per_share_text")?,
        shares_purchased: row.try_get("shares_purchased_scaled")?,
        currency: row.try_get("currency")?,
        fmv_at_offering: row.try_get("fmv_at_offering_text")?,
        employer_discount_percent: row.try_get("employer_discount_percent_text")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

// ---------------------------------------------------------------------------
// Tests — parser only (no DB). Integration coverage lives in
// tests/rls_cross_tenant.rs + the Slice-1 notes-lift probe.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slice_1_discount_only() {
        let got = parse_slice_1_espp_notes(r#"{"estimated_discount_percent":15}"#).unwrap();
        assert_eq!(got.lifted_discount_percent, "15.00");
        assert_eq!(got.preserved_user_note, None);
    }

    #[test]
    fn parses_slice_1_discount_with_user_note() {
        let got = parse_slice_1_espp_notes(
            r#"{"estimated_discount_percent":12.5,"note":"April window"}"#,
        )
        .unwrap();
        assert_eq!(got.lifted_discount_percent, "12.50");
        assert_eq!(got.preserved_user_note, Some("April window".to_string()));
    }

    #[test]
    fn rejects_free_text() {
        assert!(parse_slice_1_espp_notes("just a plain note").is_none());
    }

    #[test]
    fn rejects_json_without_discount_key() {
        assert!(parse_slice_1_espp_notes(r#"{"other":"value"}"#).is_none());
    }

    #[test]
    fn rejects_out_of_range_discount() {
        assert!(parse_slice_1_espp_notes(r#"{"estimated_discount_percent":150}"#).is_none());
        assert!(parse_slice_1_espp_notes(r#"{"estimated_discount_percent":-5}"#).is_none());
    }

    #[test]
    fn rejects_non_numeric_discount() {
        assert!(parse_slice_1_espp_notes(r#"{"estimated_discount_percent":"15"}"#).is_none());
    }
}
