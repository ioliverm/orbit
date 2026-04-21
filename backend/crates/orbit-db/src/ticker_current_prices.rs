//! `ticker_current_prices` repository (Slice 3 T28).
//!
//! One row per (user, ticker). User-entered current price that feeds the
//! paper-gains tile (AC-5.2.1..AC-5.2.6, Q1). Every query routes through
//! a `&mut Tx` borrowed via [`crate::Tx::for_user`], so RLS scopes the
//! visible row set to the owner (SEC-020..023). The handler (T29) is
//! expected to have normalized the ticker (UPPER + trim) before reaching
//! this module; the DDL CHECK mirrors the `grants.ticker` regex
//! verbatim.
//!
//! Per AC-5.2.6 no `audit_log` row is written for current-price edits
//! — current prices are user workspace data, not regulated inputs.
//! This module therefore writes no audit rows; callers do not either.
//!
//! # Decimal passthrough
//!
//! `price` is `NUMERIC(20,6)` and carried across the crate boundary as
//! `String` via `::numeric` on write / `::text` on read, consistent
//! with `espp_purchases.fmv_at_purchase` and `grants.strike_amount`.
//!
//! Traces to:
//!   - ADR-017 §1 (DDL, RLS tenant_isolation, unique key).
//!   - docs/requirements/slice-3-acceptance-criteria.md §5 (AC-5.2).

use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// A `ticker_current_prices` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickerPrice {
    pub id: Uuid,
    pub user_id: Uuid,
    /// Matches `^[A-Z0-9.\-]{1,8}$` per the DDL CHECK.
    pub ticker: String,
    /// Decimal passthrough (`NUMERIC(20,6)::text`). Always positive.
    pub price: String,
    /// `USD | EUR | GBP` (DDL CHECK).
    pub currency: String,
    pub entered_at: chrono::DateTime<chrono::Utc>,
}

/// List every row for `user_id`, ticker-ascending (stable display order
/// in the "Precios actuales" dialog).
pub async fn list_for_user(
    tx: &mut Tx<'_>,
    user_id: Uuid,
) -> Result<Vec<TickerPrice>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, user_id, ticker, price::text AS price_text, currency, entered_at
          FROM ticker_current_prices
         WHERE user_id = $1
         ORDER BY ticker ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_ticker_price).collect()
}

/// Upsert by `(user_id, ticker)`. On conflict, overwrite `price`,
/// `currency`, and `entered_at`. Callers pass a pre-normalized
/// `ticker` (UPPER + trim); the DDL CHECK is the last line of defense.
pub async fn upsert(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    ticker: &str,
    price: &str,
    currency: &str,
) -> Result<TickerPrice, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO ticker_current_prices (user_id, ticker, price, currency)
        VALUES ($1, $2, $3::numeric, $4)
        ON CONFLICT (user_id, ticker) DO UPDATE
           SET price = EXCLUDED.price,
               currency = EXCLUDED.currency,
               entered_at = now()
        RETURNING id, user_id, ticker, price::text AS price_text, currency, entered_at
        "#,
    )
    .bind(user_id)
    .bind(ticker)
    .bind(price)
    .bind(currency)
    .fetch_one(tx.as_executor())
    .await?;

    row_to_ticker_price(&row)
}

/// Remove the row for `(user_id, ticker)`. Returns `true` if a row was
/// deleted, `false` when no row matched (either the ticker was not
/// present or RLS filtered it).
pub async fn delete(tx: &mut Tx<'_>, user_id: Uuid, ticker: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM ticker_current_prices WHERE user_id = $1 AND ticker = $2")
        .bind(user_id)
        .bind(ticker)
        .execute(tx.as_executor())
        .await?;

    Ok(res.rows_affected() > 0)
}

fn row_to_ticker_price(row: &sqlx::postgres::PgRow) -> Result<TickerPrice, sqlx::Error> {
    Ok(TickerPrice {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        ticker: row.try_get("ticker")?,
        price: row.try_get("price_text")?,
        currency: row.try_get("currency")?,
        entered_at: row.try_get("entered_at")?,
    })
}
