//! `residency_periods` repository (Slice 1 T12).
//!
//! Every query routes through a `&mut Tx` borrowed via
//! [`crate::Tx::for_user`], so the priming `app.user_id` GUC scopes the
//! visible row set to the owner (SEC-020..023). Callers must not call
//! `pool.acquire()` directly — the xtask lint rejects it outside
//! `orbit-db/src/tx.rs`.
//!
//! Traces to:
//!   - ADR-014 §1 (residency_periods DDL).
//!   - docs/requirements/slice-1-acceptance-criteria.md §4.1 (AC-4.1.3,
//!     AC-4.1.4, AC-4.1.7).

use chrono::NaiveDate;
use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// A `residency_periods` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidencyPeriod {
    pub id: Uuid,
    pub user_id: Uuid,
    /// ISO 3166-1 alpha-2 jurisdiction code, currently `ES` or `UK`.
    pub jurisdiction: String,
    /// Autonomía code such as `ES-MD`, `ES-PV`, …; `None` when not applicable.
    pub sub_jurisdiction: Option<String>,
    pub from_date: NaiveDate,
    /// `None` means "still the current period" (AC-4.1.7).
    pub to_date: Option<NaiveDate>,
    /// Subset of `beckham_law`, `foral_pais_vasco`, `foral_navarra` (DDL CHECK).
    pub regime_flags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// INSERT a new residency period for `user_id`.
///
/// The caller is expected to have validated the input (jurisdiction enum,
/// regime_flags subset) upstream; the DDL CHECK constraints are the
/// last line of defense.
pub async fn create_period(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    jurisdiction: &str,
    sub_jurisdiction: Option<&str>,
    from_date: NaiveDate,
    regime_flags: &[String],
) -> Result<ResidencyPeriod, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO residency_periods (
            user_id, jurisdiction, sub_jurisdiction, from_date, regime_flags
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, user_id, jurisdiction, sub_jurisdiction,
                  from_date, to_date, regime_flags, created_at
        "#,
    )
    .bind(user_id)
    .bind(jurisdiction)
    .bind(sub_jurisdiction)
    .bind(from_date)
    .bind(regime_flags)
    .fetch_one(tx.as_executor())
    .await?;

    row_to_residency(&row)
}

/// Close the prior open period (if any) and INSERT the new one in a single
/// DB round-trip (AC-4.1.7).
///
/// The close step sets `to_date = today` on every period for this user where
/// `to_date IS NULL`. Idempotency under repeated calls on the same day is
/// provided by last-write-wins: a second call on the same day closes the
/// just-inserted row with `to_date = today` (equal to `from_date`) and
/// inserts a fresh row. The caller is expected to rate-limit this path at
/// the handler layer (AC-4.1.7 semantics).
pub async fn close_and_create(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    jurisdiction: &str,
    sub_jurisdiction: Option<&str>,
    today: NaiveDate,
    regime_flags: &[String],
) -> Result<ResidencyPeriod, sqlx::Error> {
    // Single CTE: close all open rows for the user, then INSERT the new row.
    // RLS still applies to both the UPDATE target and the INSERT row (the
    // tenant_isolation policy has USING + WITH CHECK), so a caller trying to
    // close another user's row would hit an empty match and then a
    // WITH CHECK violation on the INSERT. In practice the RLS scope keeps
    // this strictly owner-only.
    let row = sqlx::query(
        r#"
        WITH closed AS (
            UPDATE residency_periods
               SET to_date = $4
             WHERE user_id = $1 AND to_date IS NULL
         RETURNING id
        )
        INSERT INTO residency_periods (
            user_id, jurisdiction, sub_jurisdiction, from_date, regime_flags
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, user_id, jurisdiction, sub_jurisdiction,
                  from_date, to_date, regime_flags, created_at
        "#,
    )
    .bind(user_id)
    .bind(jurisdiction)
    .bind(sub_jurisdiction)
    .bind(today)
    .bind(regime_flags)
    .fetch_one(tx.as_executor())
    .await?;

    row_to_residency(&row)
}

/// Fetch the single `residency_periods` row for this user where
/// `to_date IS NULL`. Returns `None` for a user with no residency set yet.
pub async fn current_period(
    tx: &mut Tx<'_>,
    user_id: Uuid,
) -> Result<Option<ResidencyPeriod>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, user_id, jurisdiction, sub_jurisdiction,
               from_date, to_date, regime_flags, created_at
          FROM residency_periods
         WHERE user_id = $1 AND to_date IS NULL
         LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_residency).transpose()
}

fn row_to_residency(row: &sqlx::postgres::PgRow) -> Result<ResidencyPeriod, sqlx::Error> {
    Ok(ResidencyPeriod {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        jurisdiction: row.try_get("jurisdiction")?,
        sub_jurisdiction: row.try_get("sub_jurisdiction")?,
        from_date: row.try_get("from_date")?,
        to_date: row.try_get("to_date")?,
        regime_flags: row.try_get("regime_flags")?,
        created_at: row.try_get("created_at")?,
    })
}
