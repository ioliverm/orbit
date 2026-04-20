//! `grants` repository (Slice 1 T12).
//!
//! Every query routes through a `&mut Tx` borrowed via
//! [`crate::Tx::for_user`], so RLS scopes the visible row set to the owner
//! (SEC-020..023). The DDL-level CHECK + FK constraints are the last line
//! of defense; callers are expected to have validated input upstream.
//!
//! # `NUMERIC(20,4)` at the Rust boundary
//!
//! `grants.share_count` is `NUMERIC(20,4)`. Neither `rust_decimal` nor
//! `bigdecimal` is in the workspace (ADR-014 §"Upstream ambiguities" item 3
//! leaves the call to the implementation engineer). We carry share counts as
//! [`orbit_core::Shares`] — an `i64` scaled by 10,000 — which matches what
//! `derive_vesting_events` already consumes. The conversion is done in SQL:
//!
//!   - On write: `$n::numeric / 10000` coerces the bound `i64` back to the
//!     `NUMERIC(20,4)` the column expects.
//!   - On read: `(share_count * 10000)::bigint` returns the scaled-i64.
//!
//! `grants.strike_amount` is `NUMERIC(20,6)` and has no arithmetic surface
//! in Slice 1 (AC-4.2.8 validation only). We pass it through as an
//! `Option<String>` cast via `::numeric` on write and `::text` on read, so
//! that the tax-engine crates (Slice 4+) can introduce a proper Decimal
//! dep without breaking the Slice-1 boundary.
//!
//! Traces to:
//!   - ADR-014 §1 (grants DDL, all CHECK constraints, touch_updated_at
//!     trigger).
//!   - docs/requirements/slice-1-acceptance-criteria.md §4.2.

use chrono::NaiveDate;
use orbit_core::Shares;
use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// A `grants` row.
///
/// `share_count` is the scaled-i64 form (`orbit_core::Shares`); convert to
/// whole shares or display via `share_count / orbit_core::SHARES_SCALE` at
/// the UI/JSON boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub id: Uuid,
    pub user_id: Uuid,
    pub instrument: String,
    pub grant_date: NaiveDate,
    pub share_count: Shares,
    /// Decimal passthrough (NUMERIC(20,6)::text). `None` for RSU/ESPP per
    /// the DDL `strike_required_for_options` CHECK.
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

/// Input shape for [`create_grant`] / [`update_grant`].
///
/// Mirrors the grant form fields (AC-4.2.1..4.2.4). The handler layer is
/// expected to have translated the raw JSON into this struct and validated
/// every cross-field constraint (cliff ≤ total, strike-for-options,
/// double-trigger-only-on-RSU).
#[derive(Debug, Clone)]
pub struct GrantForm {
    pub instrument: String,
    pub grant_date: NaiveDate,
    pub share_count: Shares,
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
}

/// INSERT a new grant owned by `user_id`.
pub async fn create_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    form: &GrantForm,
) -> Result<Grant, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO grants (
            user_id, instrument, grant_date,
            share_count, strike_amount, strike_currency,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            double_trigger, liquidity_event_date, double_trigger_satisfied_by,
            employer_name, ticker, notes
        )
        VALUES (
            $1, $2, $3,
            $4::numeric / 10000, $5::numeric, $6,
            $7, $8, $9, $10,
            $11, $12, $13,
            $14, $15, $16
        )
        RETURNING
            id, user_id, instrument, grant_date,
            (share_count * 10000)::bigint AS share_count_scaled,
            strike_amount::text AS strike_amount_text, strike_currency,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            double_trigger, liquidity_event_date, double_trigger_satisfied_by,
            employer_name, ticker, notes,
            created_at, updated_at
        "#,
    )
    .bind(user_id)
    .bind(&form.instrument)
    .bind(form.grant_date)
    .bind(form.share_count)
    .bind(form.strike_amount.as_deref())
    .bind(form.strike_currency.as_deref())
    .bind(form.vesting_start)
    .bind(form.vesting_total_months)
    .bind(form.cliff_months)
    .bind(&form.vesting_cadence)
    .bind(form.double_trigger)
    .bind(form.liquidity_event_date)
    .bind(form.double_trigger_satisfied_by.as_deref())
    .bind(&form.employer_name)
    .bind(form.ticker.as_deref())
    .bind(form.notes.as_deref())
    .fetch_one(tx.as_executor())
    .await?;

    row_to_grant(&row)
}

/// UPDATE an existing grant in place. The `updated_at` column is refreshed
/// by the `grants_touch_updated_at` trigger.
///
/// Returns `sqlx::Error::RowNotFound` if no row matched — either the grant
/// id is wrong or RLS filtered it (the caller is not the owner).
pub async fn update_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
    form: &GrantForm,
) -> Result<Grant, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE grants
           SET instrument = $3,
               grant_date = $4,
               share_count = $5::numeric / 10000,
               strike_amount = $6::numeric,
               strike_currency = $7,
               vesting_start = $8,
               vesting_total_months = $9,
               cliff_months = $10,
               vesting_cadence = $11,
               double_trigger = $12,
               liquidity_event_date = $13,
               double_trigger_satisfied_by = $14,
               employer_name = $15,
               ticker = $16,
               notes = $17
         WHERE id = $1 AND user_id = $2
     RETURNING
            id, user_id, instrument, grant_date,
            (share_count * 10000)::bigint AS share_count_scaled,
            strike_amount::text AS strike_amount_text, strike_currency,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            double_trigger, liquidity_event_date, double_trigger_satisfied_by,
            employer_name, ticker, notes,
            created_at, updated_at
        "#,
    )
    .bind(grant_id)
    .bind(user_id)
    .bind(&form.instrument)
    .bind(form.grant_date)
    .bind(form.share_count)
    .bind(form.strike_amount.as_deref())
    .bind(form.strike_currency.as_deref())
    .bind(form.vesting_start)
    .bind(form.vesting_total_months)
    .bind(form.cliff_months)
    .bind(&form.vesting_cadence)
    .bind(form.double_trigger)
    .bind(form.liquidity_event_date)
    .bind(form.double_trigger_satisfied_by.as_deref())
    .bind(&form.employer_name)
    .bind(form.ticker.as_deref())
    .bind(form.notes.as_deref())
    .fetch_one(tx.as_executor())
    .await?;

    row_to_grant(&row)
}

/// DELETE a grant. Returns `Ok(())` whether or not a row matched — RLS-aware
/// handlers distinguish "not found" from "not owned" via the prior
/// `get_grant` lookup, per AC-7.3.
pub async fn delete_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM grants WHERE id = $1 AND user_id = $2")
        .bind(grant_id)
        .bind(user_id)
        .execute(tx.as_executor())
        .await?;
    Ok(())
}

/// Fetch a single grant by id, scoped to the owner. Returns `None` if the
/// row does not exist or is not owned by `user_id` — RLS-first by
/// construction (AC-7.3: 404, not 403, to avoid existence leaks).
pub async fn get_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Option<Grant>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            id, user_id, instrument, grant_date,
            (share_count * 10000)::bigint AS share_count_scaled,
            strike_amount::text AS strike_amount_text, strike_currency,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            double_trigger, liquidity_event_date, double_trigger_satisfied_by,
            employer_name, ticker, notes,
            created_at, updated_at
          FROM grants
         WHERE id = $1 AND user_id = $2
        "#,
    )
    .bind(grant_id)
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_grant).transpose()
}

/// List every grant owned by `user_id`, newest first.
pub async fn list_grants(tx: &mut Tx<'_>, user_id: Uuid) -> Result<Vec<Grant>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            id, user_id, instrument, grant_date,
            (share_count * 10000)::bigint AS share_count_scaled,
            strike_amount::text AS strike_amount_text, strike_currency,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            double_trigger, liquidity_event_date, double_trigger_satisfied_by,
            employer_name, ticker, notes,
            created_at, updated_at
          FROM grants
         WHERE user_id = $1
         ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_grant).collect()
}

fn row_to_grant(row: &sqlx::postgres::PgRow) -> Result<Grant, sqlx::Error> {
    Ok(Grant {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        instrument: row.try_get("instrument")?,
        grant_date: row.try_get("grant_date")?,
        share_count: row.try_get("share_count_scaled")?,
        strike_amount: row.try_get("strike_amount_text")?,
        strike_currency: row.try_get("strike_currency")?,
        vesting_start: row.try_get("vesting_start")?,
        vesting_total_months: row.try_get("vesting_total_months")?,
        cliff_months: row.try_get("cliff_months")?,
        vesting_cadence: row.try_get("vesting_cadence")?,
        double_trigger: row.try_get("double_trigger")?,
        liquidity_event_date: row.try_get("liquidity_event_date")?,
        double_trigger_satisfied_by: row.try_get("double_trigger_satisfied_by")?,
        employer_name: row.try_get("employer_name")?,
        ticker: row.try_get("ticker")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
