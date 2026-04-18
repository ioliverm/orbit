//! Cross-tenant RLS regression probes (S0-17, SEC-020..024).
//!
//! These tests verify three claims made by `migrations/20260418120000_init.sql`
//! and `orbit_db::Tx::for_user`:
//!
//! 1. With `Tx::for_user(user_a)`, SELECT on an RLS-guarded table returns only
//!    user A's rows. User B's rows are invisible.
//! 2. With `Tx::for_user(user_a)`, UPDATE and DELETE targeting user B's row
//!    affect **zero** rows — the WHERE clause is silently scoped to the
//!    priming GUC.
//! 3. Without `Tx::for_user` (a raw `pool.begin()` that never sets
//!    `app.user_id`), SELECT returns **zero** rows: the policy expression
//!    `user_id = current_setting('app.user_id', true)::uuid` evaluates to NULL
//!    when the GUC is unset, which is not TRUE and therefore filters every
//!    row.
//!
//! # Obvious-on-failure
//!
//! The danger this suite guards against is an RLS misconfiguration that
//! silently returns **all** rows. Each assertion is structured so that
//! "saw rows I should not have seen" produces an explicit panic naming the
//! offending user, not a silent `assert_eq!(0, 0)` masquerade.
//!
//! # Prerequisites (sandbox)
//!
//! These tests require a live Postgres with `migrations/20260418120000_init.sql`
//! applied, and two env vars:
//!
//!   * `DATABASE_URL`          — the `orbit_app` runtime role connection.
//!   * `DATABASE_URL_MIGRATE`  — the `orbit_migrate` schema-owner connection,
//!     used to seed rows for two distinct users. `orbit_migrate` is the table
//!     owner and therefore bypasses the permissive RLS policy (the migration
//!     does not set `FORCE ROW LEVEL SECURITY`). This is the *only* place the
//!     test uses `orbit_migrate`; every actual probe runs through `orbit_app`
//!     via `Tx::for_user`.
//!
//! Run with:
//!
//! ```text
//! cargo test -p orbit-db --features integration-tests -- --nocapture
//! ```
//!
//! Without the feature the file is a single no-op module; the default
//! workspace test run stays Postgres-free.

#![cfg(feature = "integration-tests")]

use orbit_db::Tx;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use sqlx::Row;
use std::str::FromStr;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Connect to Postgres using a URL from the environment. Honors the URL's
/// `sslmode` query parameter as-is (the local dev stack uses `sslmode=require`
/// with a self-signed cert — `VerifyFull` is reserved for `orbit_db::connect`
/// which is production-shaped).
///
/// This helper deliberately does **not** go through `orbit_db::connect`,
/// because the production helper pins `VerifyFull` and would reject the local
/// stack's self-signed cert. The test's correctness does not depend on TLS
/// verification mode — it depends on RLS. Keep these concerns separated.
async fn pool_from_env(var: &str) -> PgPool {
    let url = std::env::var(var)
        .unwrap_or_else(|_| panic!("{var} must be set for orbit-db integration tests"));
    let mut opts =
        PgConnectOptions::from_str(&url).unwrap_or_else(|e| panic!("invalid url in {var}: {e}"));
    // If the URL did not specify sslmode, default to `Require` (not Prefer)
    // so we cannot silently fall back to cleartext.
    if !url.contains("sslmode=") {
        opts = opts.ssl_mode(PgSslMode::Require);
    }
    PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap_or_else(|e| panic!("connect via {var} failed: {e}"))
}

/// Seed one `sessions` row owned by `user_id`. Runs as `orbit_migrate` so it
/// bypasses RLS (table-owner default when `FORCE ROW LEVEL SECURITY` is not
/// set, which it isn't in the Slice 0a init migration). Returns the session
/// row id so the test can target it with surgical UPDATE/DELETE.
async fn seed_session(migrate_pool: &PgPool, user_id: Uuid, tag: &str) -> Uuid {
    // The `sessions` table has several NOT NULL columns. We fill the minimum:
    // UUIDs for the hashes (unique via the table constraint), a non-null
    // family_id and ip_hash, and a short user_agent. `tag` discriminates the
    // two per-test rows so a duplicate-hash collision across tests is
    // impossible.
    let session_id_hash = format!("session_hash_{tag}_{user_id}").into_bytes();
    let refresh_hash = format!("refresh_hash_{tag}_{user_id}").into_bytes();
    let ip_hash = vec![0u8; 32];
    let family_id = Uuid::new_v4();

    let row = sqlx::query(
        r#"
        INSERT INTO sessions (
            user_id, session_id_hash, refresh_token_hash, family_id,
            ip_hash, user_agent
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(&session_id_hash)
    .bind(&refresh_hash)
    .bind(family_id)
    .bind(&ip_hash)
    .bind(format!("rls-test-{tag}"))
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed session for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("sessions.id column missing from RETURNING")
}

/// Ensure a matching `users` row exists for `user_id`. `sessions.user_id` has
/// a FK to `users(id)` with `ON DELETE CASCADE`, so we need a parent row
/// before we can insert. `orbit_migrate` owns the table and can insert.
async fn ensure_user(migrate_pool: &PgPool, user_id: Uuid, tag: &str) {
    // Email must be unique. Use the uuid to guarantee uniqueness across runs.
    let email = format!("rls-{tag}-{user_id}@example.test");
    sqlx::query(
        r#"
        INSERT INTO users (id, email, password_hash)
        VALUES ($1, $2, 'not-a-real-hash-rls-test-only')
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(user_id)
    .bind(email)
    .execute(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("ensure_user {user_id} ({tag}) failed: {e}"));
}

/// Best-effort cleanup — cascades remove the session rows. Failures here do
/// not fail the test (the next run's distinct UUIDs won't collide).
async fn cleanup_user(migrate_pool: &PgPool, user_id: Uuid) {
    let _ = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(migrate_pool)
        .await;
}

// ---------------------------------------------------------------------------
// Probes
// ---------------------------------------------------------------------------

/// Probe 1 — with `Tx::for_user(user_a)` we see user A's row and only that.
#[tokio::test]
async fn for_user_select_returns_only_owners_rows() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_select").await;
    ensure_user(&migrate_pool, user_b, "b_select").await;
    let sid_a = seed_session(&migrate_pool, user_a, "a_select").await;
    let sid_b = seed_session(&migrate_pool, user_b, "b_select").await;

    // The probe itself runs through Tx::for_user(user_a).
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");

    let rows = sqlx::query("SELECT id, user_id FROM sessions WHERE id IN ($1, $2)")
        .bind(sid_a)
        .bind(sid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select under RLS");

    // Must be exactly one row, and it must be user_a's.
    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT under Tx::for_user({user_a}) returned {} rows for (a, b) pair, \
         expected exactly 1. If this is 2, RLS is NOT filtering by app.user_id.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    let seen_user: Uuid = rows[0].try_get("user_id").expect("row user_id");
    assert_eq!(
        seen_id, sid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's session id {seen_id} \
         (expected user A's {sid_a})"
    );
    assert_eq!(
        seen_user, user_a,
        "RLS leak: Tx::for_user({user_a}) returned a row for user {seen_user}"
    );

    tx.rollback().await.expect("rollback");
    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 2 — with `Tx::for_user(user_a)`, an UPDATE targeting user B's row
/// affects zero rows, and likewise for DELETE. User B's row is untouched.
#[tokio::test]
async fn for_user_update_and_delete_cannot_touch_other_tenants_rows() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_mut").await;
    ensure_user(&migrate_pool, user_b, "b_mut").await;
    let _sid_a = seed_session(&migrate_pool, user_a, "a_mut").await;
    let sid_b = seed_session(&migrate_pool, user_b, "b_mut").await;

    // --- UPDATE probe
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");
    let updated =
        sqlx::query("UPDATE sessions SET user_agent = 'cross-tenant-attack' WHERE id = $1")
            .bind(sid_b)
            .execute(tx.as_executor())
            .await
            .expect("cross-tenant UPDATE should not error, only match 0 rows");
    assert_eq!(
        updated.rows_affected(),
        0,
        "RLS leak: UPDATE from Tx::for_user({user_a}) affected {} rows on user B's session {sid_b}. \
         RLS must confine UPDATE to the owner's rows.",
        updated.rows_affected()
    );
    tx.commit().await.expect("commit empty update");

    // --- DELETE probe
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) 2");
    let deleted = sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(sid_b)
        .execute(tx.as_executor())
        .await
        .expect("cross-tenant DELETE should not error, only match 0 rows");
    assert_eq!(
        deleted.rows_affected(),
        0,
        "RLS leak: DELETE from Tx::for_user({user_a}) removed {} rows on user B's session {sid_b}.",
        deleted.rows_affected()
    );
    tx.commit().await.expect("commit empty delete");

    // --- Sanity: user B's row is still intact with its original user_agent,
    // verified via the migrate pool (which bypasses RLS as table owner).
    let surviving = sqlx::query("SELECT user_agent FROM sessions WHERE id = $1")
        .bind(sid_b)
        .fetch_optional(&migrate_pool)
        .await
        .expect("post-check lookup");
    let surviving = surviving
        .expect("user B's session must still exist — DELETE under RLS must not have landed");
    let ua: String = surviving.try_get("user_agent").expect("user_agent");
    assert!(
        ua.starts_with("rls-test-b_mut"),
        "RLS leak: user B's session user_agent was mutated to {ua:?} by a Tx::for_user(user_a) UPDATE"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 3 — a raw `pool.begin()` that never primes `app.user_id` sees zero
/// rows. The policy expression `user_id = current_setting('app.user_id',
/// true)::uuid` evaluates to NULL when the GUC is missing, which is not TRUE,
/// so RLS filters every row out.
///
/// This is the strict test for "silently returns all rows if the GUC wasn't
/// set" — the failure mode we are most worried about. If this assertion ever
/// sees a nonzero row count, the RLS policy has been weakened (e.g. rewritten
/// to accept NULL, or the table has been set to OWNER-of-orbit_app, or
/// BYPASSRLS was granted).
#[tokio::test]
async fn raw_begin_without_priming_sees_no_rows() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_raw").await;
    ensure_user(&migrate_pool, user_b, "b_raw").await;
    let sid_a = seed_session(&migrate_pool, user_a, "a_raw").await;
    let sid_b = seed_session(&migrate_pool, user_b, "b_raw").await;

    // Raw `begin`. We deliberately do NOT route through Tx::for_user here —
    // this probe is the negative case. The `.begin()` call on a pool handle
    // is the primitive the xtask-lint forbids outside `tx.rs`; invoking it
    // here from a dev-deps test file is the only sanctioned test-side usage.
    let mut raw = app_pool.begin().await.expect("raw begin");

    let rows = sqlx::query("SELECT id, user_id FROM sessions WHERE id = $1 OR id = $2")
        .bind(sid_a)
        .bind(sid_b)
        .fetch_all(&mut *raw)
        .await
        .expect("SELECT should not error — just return zero rows");

    assert_eq!(
        rows.len(),
        0,
        "RLS MISCONFIGURED: a raw `pool.begin()` (no app.user_id set) returned {} rows. \
         This means either the policy accepts NULL, or the runtime role has BYPASSRLS, \
         or the table lost ENABLE ROW LEVEL SECURITY. Expected exactly 0.",
        rows.len()
    );

    raw.rollback().await.expect("rollback raw");
    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 4 — defense against a false-green Probe 3. If the migrate pool ever
/// started behaving like orbit_app (e.g. someone repointed DATABASE_URL_MIGRATE
/// at orbit_app), the seed step in Probe 3 would silently insert nothing and
/// Probe 3's assertion `rows.len() == 0` would pass vacuously. This probe
/// verifies that the seed step actually landed rows by reading them back
/// through the migrate pool (owner, bypasses RLS).
#[tokio::test]
async fn seed_rows_are_actually_present_under_migrate_role() {
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_sanity").await;
    ensure_user(&migrate_pool, user_b, "b_sanity").await;
    let sid_a = seed_session(&migrate_pool, user_a, "a_sanity").await;
    let sid_b = seed_session(&migrate_pool, user_b, "b_sanity").await;

    let rows = sqlx::query("SELECT id FROM sessions WHERE id IN ($1, $2)")
        .bind(sid_a)
        .bind(sid_b)
        .fetch_all(&migrate_pool)
        .await
        .expect("select under owner");

    assert_eq!(
        rows.len(),
        2,
        "Harness broken: expected the two seeded sessions to be visible under \
         the orbit_migrate role (table owner bypasses permissive RLS). Saw {}.",
        rows.len()
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}
