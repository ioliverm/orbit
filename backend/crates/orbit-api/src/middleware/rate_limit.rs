//! Per-key token-bucket rate limit backed by `rate_limit_buckets` (SEC-160).
//!
//! Each limiter is identified by a deterministic string key — typical
//! shapes: `"signin:ip:<hmac-hex>"` or `"signin:account:<uuid>"`. The
//! update is a single UPSERT inside a transaction so two concurrent
//! requests refill-then-consume atomically. The "leaky bucket" refill rate
//! is `capacity / period_secs`; the bucket is consumed one token per
//! request.
//!
//! This module exposes a pure helper used by the auth handlers rather than
//! an axum layer, because the bucket keys vary per endpoint (IP vs
//! account) and some endpoints consume two buckets per request.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

/// One limiter instance.
#[derive(Debug, Clone, Copy)]
pub struct Limiter {
    /// Maximum tokens the bucket holds (also the refill ceiling).
    pub capacity: f64,
    /// Refill period in seconds. Together with `capacity` this gives the
    /// refill rate.
    pub period_secs: u64,
}

impl Limiter {
    fn refill_per_second(self) -> f64 {
        self.capacity / (self.period_secs as f64)
    }
}

/// Outcome of a [`try_consume`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    RateLimited { retry_after_secs: u64 },
}

/// Atomically consume one token from the bucket at `key`. Returns
/// [`Decision::RateLimited`] with a `Retry-After` suggestion if the bucket
/// is empty.
pub async fn try_consume(
    pool: &PgPool,
    key: &str,
    limiter: Limiter,
) -> Result<Decision, sqlx::Error> {
    // One UPSERT-SELECT-UPSERT cycle inside a single tx. We use `pool.begin()`
    // here because the rate limit path runs before any user is known —
    // `Tx::for_user` is inapplicable. `rate_limit_buckets` is not RLS-scoped
    // (it's an internal/abuse-control table, per §4 Internal class).
    let mut tx = pool.begin().await?;

    let now: DateTime<Utc> = Utc::now();

    let existing = sqlx::query(
        r#"SELECT tokens, last_refilled_at FROM rate_limit_buckets WHERE key = $1 FOR UPDATE"#,
    )
    .bind(key)
    .fetch_optional(&mut *tx)
    .await?;

    let (tokens, last_refilled_at) = match existing {
        Some(row) => {
            let prev_tokens: f64 = row.try_get("tokens")?;
            let prev_at: DateTime<Utc> = row.try_get("last_refilled_at")?;
            (prev_tokens, prev_at)
        }
        None => (limiter.capacity, now),
    };

    // Refill since last seen.
    let elapsed = (now - last_refilled_at).num_milliseconds().max(0) as f64 / 1000.0;
    let refilled = (tokens + elapsed * limiter.refill_per_second()).min(limiter.capacity);

    if refilled < 1.0 {
        // Not enough budget. Compute retry-after: time until one full token.
        let missing = 1.0 - refilled;
        let secs = (missing / limiter.refill_per_second()).ceil() as u64;
        // Persist the updated last_refilled_at so the next request accounts
        // for this refill.
        sqlx::query(
            r#"
            INSERT INTO rate_limit_buckets (key, tokens, last_refilled_at)
            VALUES ($1, $2, $3)
            ON CONFLICT (key) DO UPDATE
              SET tokens = EXCLUDED.tokens,
                  last_refilled_at = EXCLUDED.last_refilled_at
            "#,
        )
        .bind(key)
        .bind(refilled)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Decision::RateLimited {
            retry_after_secs: secs.max(1),
        });
    }

    let new_tokens = refilled - 1.0;
    sqlx::query(
        r#"
        INSERT INTO rate_limit_buckets (key, tokens, last_refilled_at)
        VALUES ($1, $2, $3)
        ON CONFLICT (key) DO UPDATE
          SET tokens = EXCLUDED.tokens,
              last_refilled_at = EXCLUDED.last_refilled_at
        "#,
    )
    .bind(key)
    .bind(new_tokens)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Decision::Allowed)
}

/// Peek at the bucket at `key` without consuming a token. Returns
/// [`Decision::RateLimited`] iff the (refilled) bucket holds less than one
/// token. Used by the captcha check in the signin handler — the signin
/// attempt itself must not spend a failure budget token; only an actual
/// login.failure does.
///
/// The read path persists the refilled balance so that subsequent peeks
/// account for elapsed time even if no failure lands between them. That
/// mirrors [`try_consume`]'s behaviour on the RateLimited branch.
pub async fn peek(pool: &PgPool, key: &str, limiter: Limiter) -> Result<Decision, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let now: DateTime<Utc> = Utc::now();

    let existing = sqlx::query(
        r#"SELECT tokens, last_refilled_at FROM rate_limit_buckets WHERE key = $1 FOR UPDATE"#,
    )
    .bind(key)
    .fetch_optional(&mut *tx)
    .await?;

    let (tokens, last_refilled_at) = match existing {
        Some(row) => {
            let prev_tokens: f64 = row.try_get("tokens")?;
            let prev_at: DateTime<Utc> = row.try_get("last_refilled_at")?;
            (prev_tokens, prev_at)
        }
        None => {
            // No row yet — a fresh bucket is full. Nothing to persist.
            tx.commit().await?;
            return Ok(Decision::Allowed);
        }
    };

    let elapsed = (now - last_refilled_at).num_milliseconds().max(0) as f64 / 1000.0;
    let refilled = (tokens + elapsed * limiter.refill_per_second()).min(limiter.capacity);

    sqlx::query(
        r#"
        INSERT INTO rate_limit_buckets (key, tokens, last_refilled_at)
        VALUES ($1, $2, $3)
        ON CONFLICT (key) DO UPDATE
          SET tokens = EXCLUDED.tokens,
              last_refilled_at = EXCLUDED.last_refilled_at
        "#,
    )
    .bind(key)
    .bind(refilled)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    if refilled < 1.0 {
        let missing = 1.0 - refilled;
        let secs = (missing / limiter.refill_per_second()).ceil() as u64;
        Ok(Decision::RateLimited {
            retry_after_secs: secs.max(1),
        })
    } else {
        Ok(Decision::Allowed)
    }
}
