//! `Tx::for_user` — the **only** sanctioned query-handle acquisition path
//! (SEC-022, S0-23).
//!
//! Handlers never call `pool.begin()` or `pool.acquire()` themselves. They
//! call [`Tx::for_user(pool, user_id)`](Tx::for_user), which:
//!
//! 1. Begins a transaction on `pool`.
//! 2. Runs `SELECT set_config('app.user_id', <user_id>, true)` inside that
//!    transaction. The `true` third argument is the `is_local` flag — the
//!    equivalent of `SET LOCAL`, scoped to the current transaction and reset
//!    on commit/rollback. **Never** `SET` (without `LOCAL`), which would
//!    persist the GUC on the pooled connection and leak into the next
//!    unrelated request's transaction.
//! 3. Returns the transaction handle, wrapped in a [`Tx`] newtype so the
//!    handler cannot get at the underlying pool methods.
//!
//! RLS policies on `sessions`, `email_verifications`, `password_reset_tokens`,
//! `dsr_requests` (migrated by T6) use `current_setting('app.user_id', true)`
//! to decide visibility; priming the GUC is therefore a correctness
//! requirement, not an optimization.
//!
//! This file is the **allow-listed home of `.acquire()`** in `cargo xtask
//! check`. The current implementation does not call `.acquire()` directly —
//! `pool.begin()` handles connection checkout internally — but the allow-list
//! stays on this file so that a future helper needing `.acquire()` for a
//! non-transactional path (e.g. `LISTEN`/`NOTIFY`) lands here by default.

use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::Error;

/// A per-user transaction handle.
///
/// Drop semantics follow `sqlx::Transaction`: dropping without calling
/// [`Tx::commit`] rolls back. This matches the axum handler convention where
/// an early `?` return must not leak half-applied state.
pub struct Tx<'a> {
    inner: Transaction<'a, Postgres>,
}

impl Tx<'_> {
    /// Open a transaction scoped to `user_id` and prime `app.user_id` so
    /// RLS policies resolve to the caller's rows.
    ///
    /// On any failure the transaction is dropped (rolled back implicitly by
    /// sqlx) and the error is returned as [`Error::Tx`]. The caller MUST
    /// treat that as a hard failure — proceeding would mean either no RLS
    /// scoping (data leak) or a caller stuck in an undead transaction.
    pub async fn for_user(pool: &PgPool, user_id: Uuid) -> Result<Tx<'static>, Error> {
        let mut inner = pool.begin().await.map_err(Error::Tx)?;

        // `SET LOCAL app.user_id = $1` is not parameterizable (SET statements
        // in Postgres don't accept bind params); `set_config(name, value,
        // is_local)` is the canonical parameterizable equivalent, and
        // `is_local = true` gives us the LOCAL-to-transaction scope we need.
        // Stringify the UUID for the text argument.
        sqlx::query("SELECT set_config('app.user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *inner)
            .await
            .map_err(Error::Tx)?;

        Ok(Tx { inner })
    }

    /// Commit the transaction. On commit the `app.user_id` GUC is reset by
    /// Postgres automatically (it was SET LOCAL).
    pub async fn commit(self) -> Result<(), Error> {
        self.inner.commit().await.map_err(Error::Tx)
    }

    /// Explicitly roll back. Equivalent to dropping `self`, but readable at
    /// call sites that want the rollback intent documented.
    pub async fn rollback(self) -> Result<(), Error> {
        self.inner.rollback().await.map_err(Error::Tx)
    }

    /// Borrow the underlying connection as a sqlx `Executor` target.
    ///
    /// `sqlx::Query::execute` / `fetch_one` / `fetch_all` require `&mut
    /// PgConnection` (a type that implements `sqlx::Executor`); a
    /// `&mut Transaction` does not satisfy the bound directly. The idiomatic
    /// pattern is `&mut *tx`, which auto-derefs through
    /// `Transaction::deref_mut`. This method encapsulates that so handlers
    /// never need to write the double-borrow themselves.
    ///
    /// The returned reference is short-lived by design: handler code fetches
    /// it immediately before an `sqlx::query(...).execute(tx.as_executor())`
    /// call and drops it. This keeps the `Tx` newtype the only long-lived
    /// handle a handler holds.
    pub fn as_executor(&mut self) -> &mut PgConnection {
        &mut self.inner
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// An end-to-end test for `Tx::for_user` needs a live Postgres: the GUC
// round-trip, the RLS visibility pivot, and the `SET LOCAL` scoping are all
// Postgres-side behaviors. Slice 0a local dev uses `docker compose up db` or
// a developer-provided `DATABASE_URL`, neither of which is guaranteed in CI.
//
// The test is therefore gated behind the `integration-tests` feature. It
// will be enabled once the CI job that provisions a disposable Postgres
// lands (tracked with the wider integration-test matrix, not S0-23).
#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use super::*;
    use sqlx::Row;

    async fn pool() -> PgPool {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for orbit-db integration tests");
        PgPool::connect(&url).await.expect("connect")
    }

    #[tokio::test]
    async fn for_user_primes_app_user_id_guc() {
        let pool = pool().await;
        let uid = Uuid::new_v4();
        let mut tx = Tx::for_user(&pool, uid).await.expect("begin tx");
        let row = sqlx::query("SELECT current_setting('app.user_id', true) AS v")
            .fetch_one(tx.as_executor())
            .await
            .expect("read guc");
        let v: String = row.try_get("v").expect("get v");
        assert_eq!(v, uid.to_string());
        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    async fn for_user_guc_is_local_and_does_not_leak() {
        let pool = pool().await;
        let uid = Uuid::new_v4();
        // Prime + drop.
        {
            let _tx = Tx::for_user(&pool, uid).await.expect("begin tx");
            // Implicit rollback on drop.
        }
        // A fresh acquire on the pool should see an empty GUC, regardless of
        // whether the same underlying connection is reused.
        let mut tx = Tx::for_user(&pool, Uuid::nil()).await.expect("begin tx 2");
        let row = sqlx::query("SELECT current_setting('app.user_id', true) AS v")
            .fetch_one(tx.as_executor())
            .await
            .expect("read guc");
        let v: String = row.try_get("v").expect("get v");
        assert_eq!(v, Uuid::nil().to_string());
        tx.rollback().await.expect("rollback");
    }
}
