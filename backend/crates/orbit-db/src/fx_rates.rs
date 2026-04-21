//! `fx_rates` repository (Slice 3 T28).
//!
//! ECB FX reference data. Unlike every other repo module, `fx_rates` is
//! **NOT RLS-scoped**: the rates are global facts (EUR/USD on 2026-04-17
//! is the same for every user) and the migration grants `orbit_app` only
//! `SELECT + INSERT` — no UPDATE, no DELETE. See ADR-017 §1 "Why
//! `fx_rates` is NOT RLS-scoped" and "Why append-only on `fx_rates`" for
//! the rationale.
//!
//! Because there is no per-user scoping, read-only queries take a
//! `&PgPool` rather than a `&mut Tx` — they do not need the
//! `app.user_id` GUC primed and they do not require atomicity with any
//! other tenant-scoped write. Writes (`upsert_ecb`) still take a
//! `&mut Tx` so the worker's fetch + insert + `fx.fetch_success` audit
//! row land in one transaction; see `orbit_worker` for the call site.
//! This mirrors the residency-stage helper pattern (read from pool,
//! write through tx) and keeps the `orbit-db/src/tx.rs` allow-list on
//! `.acquire()` intact (sqlx's `fetch_*(&pool)` takes care of checkout
//! internally and does not call `.acquire()` explicitly).
//!
//! # Staleness ladder (AC-4.5.1..AC-4.5.4)
//!
//! The walkback helper maps a concrete `walkback_days` to a four-tier
//! [`Staleness`] ladder; the handler renders a UI chip off the tier, not
//! off the raw day count.
//!
//!   * `walkback_days == 0`           → [`Staleness::Fresh`]
//!   * `1 <= walkback_days <= 2`      → [`Staleness::Walkback`]
//!   * `3 <= walkback_days <= 7`      → [`Staleness::Stale`]
//!   * no row within `max_days`       → [`Staleness::Unavailable`]
//!     (encoded as `lookup_walkback` returning `Ok(None)`).
//!
//! # Decimal passthrough
//!
//! Following the Slice-1/-2 convention, `rate` and `price` columns cross
//! the crate boundary as `String` via `::text` casts on read and
//! `::numeric` casts on write. No `rust_decimal` dep in Slice 3.
//!
//! Traces to:
//!   - ADR-017 §1 (DDL, GRANT posture, unique key, index).
//!   - ADR-007 (lookup_rate, walkback, bootstrap, fetch-on-demand).
//!   - docs/requirements/slice-3-acceptance-criteria.md §4 (AC-4.5).

use chrono::NaiveDate;
use sqlx::{PgPool, Row};

use crate::Tx;

/// An `fx_rates` row.
///
/// `rate` is a `NUMERIC(20,10)` carried as `String` (see module docs for
/// the decimal-passthrough convention).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FxRate {
    pub id: uuid::Uuid,
    /// Always `"EUR"` in Slice 3 (DDL CHECK).
    pub base: String,
    /// ISO 4217 alpha-3 quote currency; `length(quote) = 3` per the DDL.
    pub quote: String,
    pub rate_date: NaiveDate,
    /// Decimal passthrough (`NUMERIC(20,10)::text`). Always positive.
    pub rate: String,
    /// `"ecb"` for worker-inserted rows; `"user_override"` reserved for
    /// Slice-4 calculation-scoped overrides (no Slice-3 handler writes it).
    pub source: String,
    pub published_at: chrono::DateTime<chrono::Utc>,
}

/// Four-tier staleness ladder per AC-4.5. The handler renders a chip
/// off the tier, not off the raw day count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    /// `walkback_days == 0` — today's rate is present.
    Fresh,
    /// `1..=2` day gap. Chip reads "stale N día(s)".
    Walkback,
    /// `3..=7` day gap. Dashboard banner fires (AC-4.5.3).
    Stale,
    /// No row within `max_days`. Chip suppressed; handler renders
    /// AC-4.5.4 / AC-5.5.4 "FX no disponible" state.
    Unavailable,
}

/// Result of [`lookup_walkback`]. `None` from the helper maps to
/// [`Staleness::Unavailable`] at the caller; this struct is only
/// returned when a row was found within the walkback window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkbackResult {
    /// Decimal passthrough (`NUMERIC(20,10)::text`).
    pub rate: String,
    /// The `rate_date` of the row that answered the query (NOT the
    /// caller's `on_date` when the helper walked back).
    pub rate_date: NaiveDate,
    /// `on_date - rate_date` in days; `>= 0`.
    pub walkback_days: u32,
    /// Tier derived from `walkback_days` per [`Staleness`].
    pub staleness: Staleness,
}

/// Map a concrete `walkback_days` value to the [`Staleness`] tier
/// per AC-4.5. `max_days` is the caller-supplied upper bound (typically
/// 7); values strictly greater than it are [`Staleness::Unavailable`]
/// but the caller path returns `None` in that case, so this function
/// is only ever called with `walkback_days <= max_days`.
fn staleness_for(walkback_days: u32) -> Staleness {
    match walkback_days {
        0 => Staleness::Fresh,
        1..=2 => Staleness::Walkback,
        3..=7 => Staleness::Stale,
        // 8+ maps to Unavailable in terms of tier, but practically
        // `lookup_walkback` returns `None` before we get here — the
        // SQL only selects rows within the window.
        _ => Staleness::Unavailable,
    }
}

/// Idempotent ECB row upsert. `INSERT ... ON CONFLICT DO NOTHING` on the
/// `(base, quote, rate_date, source)` unique key — a same-day repeat
/// fetch writes zero rows (AC-4.1.3).
///
/// Writes through a `&mut Tx` so the worker's fetch + insert + audit
/// row land atomically; on any failure the whole transaction rolls
/// back. `Tx::for_user` is NOT the right home here (fx_rates is not
/// user-scoped), but the worker runs with a Tx primed to a system
/// pseudo-user for audit-log scoping — see `orbit_worker`.
///
/// Returns the number of rows inserted (`0` on idempotent re-run,
/// `1` on first write).
pub async fn upsert_ecb(
    tx: &mut Tx<'_>,
    base: &str,
    quote: &str,
    rate_date: NaiveDate,
    rate: &str,
    published_at: chrono::DateTime<chrono::Utc>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO fx_rates (base, quote, rate_date, rate, source, published_at)
        VALUES ($1, $2, $3, $4::numeric, 'ecb', $5)
        ON CONFLICT (base, quote, rate_date, source) DO NOTHING
        "#,
    )
    .bind(base)
    .bind(quote)
    .bind(rate_date)
    .bind(rate)
    .bind(published_at)
    .execute(tx.as_executor())
    .await?;

    Ok(result.rows_affected())
}

/// Exact-match lookup for `(base, quote, on_date, source='ecb')`.
/// Returns `None` when no row landed on that specific date (holidays,
/// weekends, missed fetches — caller typically reaches for
/// [`lookup_walkback`] instead).
pub async fn lookup_on_date(
    pool: &PgPool,
    base: &str,
    quote: &str,
    on_date: NaiveDate,
) -> Result<Option<FxRate>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, base, quote, rate_date, rate::text AS rate_text, source, published_at
          FROM fx_rates
         WHERE base = $1
           AND quote = $2
           AND rate_date = $3
           AND source = 'ecb'
         LIMIT 1
        "#,
    )
    .bind(base)
    .bind(quote)
    .bind(on_date)
    .fetch_optional(pool)
    .await?;

    row.as_ref().map(row_to_fx_rate).transpose()
}

/// Walkback lookup: return the most-recent ECB rate for `(base, quote)`
/// whose `rate_date` falls in `[on_date - max_days, on_date]`. Returns
/// `None` when no row exists within the window (caller maps this to
/// [`Staleness::Unavailable`]).
///
/// The SQL filters `source = 'ecb'` explicitly so that Slice-4's
/// `user_override` rows do not leak into the reference-price lookup;
/// see ADR-017 §11 "fx_rates.source CHECK list widens in Slice 4".
pub async fn lookup_walkback(
    pool: &PgPool,
    base: &str,
    quote: &str,
    on_date: NaiveDate,
    max_days: u32,
) -> Result<Option<WalkbackResult>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT rate_date, rate::text AS rate_text
          FROM fx_rates
         WHERE base = $1
           AND quote = $2
           AND source = 'ecb'
           AND rate_date BETWEEN ($3::date - make_interval(days => $4::int)) AND $3
         ORDER BY rate_date DESC
         LIMIT 1
        "#,
    )
    .bind(base)
    .bind(quote)
    .bind(on_date)
    .bind(max_days as i32)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let rate_date: NaiveDate = row.try_get("rate_date")?;
    let rate: String = row.try_get("rate_text")?;
    // `on_date - rate_date` is non-negative by construction (the SQL
    // clamps `rate_date <= on_date`); the cast is safe.
    let walkback_days: u32 = (on_date - rate_date).num_days().max(0) as u32;

    Ok(Some(WalkbackResult {
        rate,
        rate_date,
        walkback_days,
        staleness: staleness_for(walkback_days),
    }))
}

/// Most-recent row for `(base, quote, source='ecb')`. Used by the
/// "just give me the latest" paths — `GET /fx/latest` is the obvious
/// caller in Slice 3.
pub async fn latest(pool: &PgPool, base: &str, quote: &str) -> Result<Option<FxRate>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, base, quote, rate_date, rate::text AS rate_text, source, published_at
          FROM fx_rates
         WHERE base = $1
           AND quote = $2
           AND source = 'ecb'
         ORDER BY rate_date DESC
         LIMIT 1
        "#,
    )
    .bind(base)
    .bind(quote)
    .fetch_optional(pool)
    .await?;

    row.as_ref().map(row_to_fx_rate).transpose()
}

/// Count of `source='ecb'` rows in the last 90 days. The worker reads
/// this on startup to decide whether to run the
/// `eurofxref-hist-90d.xml` bootstrap (ADR-007 + AC-4.3.x):
/// `< 30` triggers a bootstrap, `>= 30` short-circuits.
///
/// The quote filter is intentional: we care about USD coverage (the
/// one quote currency Slice 3 actually uses); a DB carrying only
/// stale EUR/JPY rows would still be cold from the dashboard's
/// perspective.
pub async fn bootstrap_count(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS n
          FROM fx_rates
         WHERE base = 'EUR'
           AND quote = 'USD'
           AND source = 'ecb'
           AND rate_date >= (CURRENT_DATE - INTERVAL '90 days')
        "#,
    )
    .fetch_one(pool)
    .await?;

    let n: i64 = row.try_get("n")?;
    Ok(n.max(0) as u64)
}

fn row_to_fx_rate(row: &sqlx::postgres::PgRow) -> Result<FxRate, sqlx::Error> {
    Ok(FxRate {
        id: row.try_get("id")?,
        base: row.try_get("base")?,
        quote: row.try_get("quote")?,
        rate_date: row.try_get("rate_date")?,
        rate: row.try_get("rate_text")?,
        source: row.try_get("source")?,
        published_at: row.try_get("published_at")?,
    })
}

// ---------------------------------------------------------------------------
// Tests — staleness_for is pure and covered here; SQL round-trips live
// in the integration suite.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_fresh_zero() {
        assert_eq!(staleness_for(0), Staleness::Fresh);
    }

    #[test]
    fn staleness_walkback_one_and_two() {
        assert_eq!(staleness_for(1), Staleness::Walkback);
        assert_eq!(staleness_for(2), Staleness::Walkback);
    }

    #[test]
    fn staleness_stale_three_through_seven() {
        for d in 3..=7 {
            assert_eq!(staleness_for(d), Staleness::Stale, "day {d}");
        }
    }

    #[test]
    fn staleness_eight_plus_is_unavailable_tier() {
        // In practice `lookup_walkback` returns None before we get here
        // (SQL window clamps the day range), but the mapping is defined
        // for defense-in-depth.
        assert_eq!(staleness_for(8), Staleness::Unavailable);
        assert_eq!(staleness_for(365), Staleness::Unavailable);
    }
}
