//! `grant_current_price_overrides` repository (Slice 3 T28).
//!
//! Per-grant current-price override (AC-5.3.1..AC-5.3.5, Q1). One row
//! per grant. Every query routes through a `&mut Tx` borrowed via
//! [`crate::Tx::for_user`], so RLS scopes the visible row set to the
//! owner (SEC-020..023). Like [`crate::ticker_current_prices`], this
//! surface writes no audit rows (AC-5.2.6).
//!
//! # Decimal passthrough
//!
//! `price` is `NUMERIC(20,6)` carried as `String` via `::numeric` on
//! write / `::text` on read (same convention as the sibling module).
//!
//! Traces to:
//!   - ADR-017 §1 (DDL, RLS tenant_isolation, unique key on grant_id).
//!   - docs/requirements/slice-3-acceptance-criteria.md §5 (AC-5.3).

use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// A `grant_current_price_overrides` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantPriceOverride {
    pub id: Uuid,
    pub user_id: Uuid,
    pub grant_id: Uuid,
    /// Decimal passthrough (`NUMERIC(20,6)::text`). Always positive.
    pub price: String,
    /// `USD | EUR | GBP` (DDL CHECK).
    pub currency: String,
    pub entered_at: chrono::DateTime<chrono::Utc>,
}

/// Fetch the override for `grant_id`. Returns `None` when no override
/// has been set, or the grant does not exist / is not owned by
/// `user_id` (RLS-filtered).
pub async fn get(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Option<GrantPriceOverride>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, user_id, grant_id, price::text AS price_text, currency, entered_at
          FROM grant_current_price_overrides
         WHERE user_id = $1 AND grant_id = $2
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_override).transpose()
}

/// Upsert by `grant_id`. On conflict, overwrite `price`, `currency`,
/// and `entered_at`. The `user_id` column is never rewritten (the
/// grant's ownership cannot change); it is bound fresh on INSERT
/// from the priming `app.user_id` GUC for RLS `WITH CHECK` purposes.
pub async fn upsert(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
    price: &str,
    currency: &str,
) -> Result<GrantPriceOverride, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO grant_current_price_overrides (user_id, grant_id, price, currency)
        VALUES ($1, $2, $3::numeric, $4)
        ON CONFLICT (grant_id) DO UPDATE
           SET price = EXCLUDED.price,
               currency = EXCLUDED.currency,
               entered_at = now()
        RETURNING id, user_id, grant_id, price::text AS price_text, currency, entered_at
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .bind(price)
    .bind(currency)
    .fetch_one(tx.as_executor())
    .await?;

    row_to_override(&row)
}

/// Remove the override for `grant_id`. Returns `true` on deletion,
/// `false` when no row matched.
pub async fn delete(tx: &mut Tx<'_>, user_id: Uuid, grant_id: Uuid) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM grant_current_price_overrides WHERE user_id = $1 AND grant_id = $2",
    )
    .bind(user_id)
    .bind(grant_id)
    .execute(tx.as_executor())
    .await?;

    Ok(res.rows_affected() > 0)
}

fn row_to_override(row: &sqlx::postgres::PgRow) -> Result<GrantPriceOverride, sqlx::Error> {
    Ok(GrantPriceOverride {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        grant_id: row.try_get("grant_id")?,
        price: row.try_get("price_text")?,
        currency: row.try_get("currency")?,
        entered_at: row.try_get("entered_at")?,
    })
}
