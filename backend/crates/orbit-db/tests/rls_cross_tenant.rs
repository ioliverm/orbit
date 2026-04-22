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

// ---------------------------------------------------------------------------
// Slice 1 T12 — cross-tenant probes for residency_periods, grants, vesting_events
// ---------------------------------------------------------------------------
//
// These follow Probe 1's shape: seed one row per user via the migrate pool
// (table-owner bypasses the permissive RLS), then assert that a
// `Tx::for_user(A)` transaction cannot SELECT / UPDATE / DELETE user B's row.
// Each table is exercised at least on SELECT; `vesting_events` additionally
// verifies UPDATE and DELETE are no-ops across tenants, because T13 will
// issue `replace_for_grant` in the hot path and RLS is what keeps that
// operation owner-scoped.

/// Seed a `residency_periods` row for `user_id`. Table owner bypasses RLS.
async fn seed_residency(migrate_pool: &PgPool, user_id: Uuid, tag: &str) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO residency_periods (
            user_id, jurisdiction, sub_jurisdiction, from_date, regime_flags
        )
        VALUES ($1, 'ES', 'ES-MD', CURRENT_DATE, ARRAY[]::text[])
        RETURNING id
        "#,
    )
    .bind(user_id)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed residency for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("residency_periods.id missing from RETURNING")
}

/// Seed a `grants` row for `user_id`. RSU to avoid the strike CHECK.
async fn seed_grant(migrate_pool: &PgPool, user_id: Uuid, tag: &str) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO grants (
            user_id, instrument, grant_date, share_count,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            employer_name
        )
        VALUES ($1, 'rsu', DATE '2024-09-15', 1000,
                DATE '2024-09-15', 48, 12, 'monthly',
                $2)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(format!("ACME-{tag}"))
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed grant for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("grants.id missing from RETURNING")
}

/// Seed a single `vesting_events` row tied to `grant_id` / `user_id`.
async fn seed_vesting_event(
    migrate_pool: &PgPool,
    user_id: Uuid,
    grant_id: Uuid,
    tag: &str,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO vesting_events (
            user_id, grant_id, vest_date,
            shares_vested_this_event, cumulative_shares_vested, state
        )
        VALUES ($1, $2, DATE '2025-09-15',
                250, 250, 'vested')
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed vesting_event for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("vesting_events.id missing from RETURNING")
}

/// Probe 5 — a `Tx::for_user(user_a)` SELECT on `grants` returns only user
/// A's row; user B's row is invisible. Covers AC-7.3 / SEC-023 for the
/// grants surface.
#[tokio::test]
async fn grants_cross_tenant_select_is_isolated() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_grants").await;
    ensure_user(&migrate_pool, user_b, "b_grants").await;
    let gid_a = seed_grant(&migrate_pool, user_a, "a_grants").await;
    let gid_b = seed_grant(&migrate_pool, user_b, "b_grants").await;

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");

    let rows = sqlx::query("SELECT id, user_id FROM grants WHERE id IN ($1, $2)")
        .bind(gid_a)
        .bind(gid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select grants under RLS");

    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on grants under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    let seen_user: Uuid = rows[0].try_get("user_id").expect("row user_id");
    assert_eq!(
        seen_id, gid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's grant id {seen_id} (expected {gid_a})"
    );
    assert_eq!(
        seen_user, user_a,
        "RLS leak: Tx::for_user({user_a}) returned a grants row for user {seen_user}"
    );

    tx.rollback().await.expect("rollback");
    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 6 — a `Tx::for_user(user_a)` SELECT on `residency_periods` returns
/// only user A's row.
#[tokio::test]
async fn residency_periods_cross_tenant_select_is_isolated() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_residency").await;
    ensure_user(&migrate_pool, user_b, "b_residency").await;
    let rid_a = seed_residency(&migrate_pool, user_a, "a_residency").await;
    let rid_b = seed_residency(&migrate_pool, user_b, "b_residency").await;

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");

    let rows = sqlx::query("SELECT id, user_id FROM residency_periods WHERE id IN ($1, $2)")
        .bind(rid_a)
        .bind(rid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select residency_periods under RLS");

    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on residency_periods under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    let seen_user: Uuid = rows[0].try_get("user_id").expect("row user_id");
    assert_eq!(
        seen_id, rid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's residency id {seen_id} (expected {rid_a})"
    );
    assert_eq!(
        seen_user, user_a,
        "RLS leak: Tx::for_user({user_a}) returned a residency row for user {seen_user}"
    );

    tx.rollback().await.expect("rollback");
    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 7 — `vesting_events` isolation. The handler's hot path
/// (`replace_for_grant`) runs DELETE + INSERT under a per-user tx; if RLS
/// ever loosened, a Tx::for_user(B) could wipe user A's events. Verify
/// SELECT / UPDATE / DELETE are all owner-scoped.
#[tokio::test]
async fn vesting_events_cross_tenant_mutation_cannot_touch_other_tenants_rows() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_vesting").await;
    ensure_user(&migrate_pool, user_b, "b_vesting").await;
    let gid_a = seed_grant(&migrate_pool, user_a, "a_vesting").await;
    let gid_b = seed_grant(&migrate_pool, user_b, "b_vesting").await;
    let eid_a = seed_vesting_event(&migrate_pool, user_a, gid_a, "a_vesting").await;
    let eid_b = seed_vesting_event(&migrate_pool, user_b, gid_b, "b_vesting").await;

    // --- SELECT probe
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) select");
    let rows = sqlx::query("SELECT id, user_id FROM vesting_events WHERE id IN ($1, $2)")
        .bind(eid_a)
        .bind(eid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select vesting_events under RLS");
    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on vesting_events under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    assert_eq!(
        seen_id, eid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's vesting_event id {seen_id}"
    );
    tx.rollback().await.expect("rollback select probe");

    // --- UPDATE probe (target user B's row)
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) update");
    let updated = sqlx::query("UPDATE vesting_events SET state = 'upcoming' WHERE id = $1")
        .bind(eid_b)
        .execute(tx.as_executor())
        .await
        .expect("cross-tenant UPDATE should return 0 rows, not error");
    assert_eq!(
        updated.rows_affected(),
        0,
        "RLS leak: UPDATE from Tx::for_user({user_a}) affected {} rows on user B's \
         vesting_event {eid_b}.",
        updated.rows_affected()
    );
    tx.commit().await.expect("commit empty update");

    // --- DELETE probe (target user B's row)
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) delete");
    let deleted = sqlx::query("DELETE FROM vesting_events WHERE id = $1")
        .bind(eid_b)
        .execute(tx.as_executor())
        .await
        .expect("cross-tenant DELETE should return 0 rows, not error");
    assert_eq!(
        deleted.rows_affected(),
        0,
        "RLS leak: DELETE from Tx::for_user({user_a}) removed {} rows on user B's \
         vesting_event {eid_b}.",
        deleted.rows_affected()
    );
    tx.commit().await.expect("commit empty delete");

    // Sanity: user B's vesting_event is still present and unmutated. Read via
    // the migrate pool (owner, bypasses RLS).
    let surviving = sqlx::query("SELECT state FROM vesting_events WHERE id = $1")
        .bind(eid_b)
        .fetch_optional(&migrate_pool)
        .await
        .expect("post-check lookup");
    let surviving =
        surviving.expect("user B's vesting_event must still exist after cross-tenant DELETE");
    let state: String = surviving.try_get("state").expect("state");
    assert_eq!(
        state, "vested",
        "RLS leak: user B's vesting_event.state was mutated to {state:?} by \
         a Tx::for_user(user_a) UPDATE"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

// ---------------------------------------------------------------------------
// Slice 2 T20 — cross-tenant probes for espp_purchases, art_7p_trips,
// modelo_720_user_inputs, and the new session-revoke path. Same shape as
// the Slice-1 probes above: seed one row per user via the migrate pool,
// then assert Tx::for_user(A) cannot SELECT / UPDATE / DELETE user B's row.
// Additionally, the notes-lift helper has its own probe (Probe 12).
// ---------------------------------------------------------------------------

/// Seed an ESPP `grants` row for `user_id`. Unlike `seed_grant` (RSU), the
/// ESPP instrument is the one `espp_purchases` accepts per the Slice-2
/// trigger.
async fn seed_espp_grant(
    migrate_pool: &PgPool,
    user_id: Uuid,
    tag: &str,
    notes: Option<&str>,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO grants (
            user_id, instrument, grant_date, share_count,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            employer_name, notes
        )
        VALUES ($1, 'espp', DATE '2024-09-15', 1000,
                DATE '2024-09-15', 24, 0, 'monthly',
                $2, $3)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(format!("ACME-{tag}"))
    .bind(notes)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed espp grant for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("grants.id missing from RETURNING")
}

/// Seed an `espp_purchases` row tied to `grant_id` for `user_id`. Table
/// owner bypasses the permissive RLS; the grant-instrument trigger
/// nonetheless fires — `grant_id` must point at an ESPP grant.
async fn seed_espp_purchase(
    migrate_pool: &PgPool,
    user_id: Uuid,
    grant_id: Uuid,
    tag: &str,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO espp_purchases (
            user_id, grant_id, offering_date, purchase_date,
            fmv_at_purchase, purchase_price_per_share,
            shares_purchased, currency
        )
        VALUES ($1, $2, DATE '2025-03-01', DATE '2025-09-01',
                30.00, 25.50, 100, 'USD')
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed espp purchase for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("espp_purchases.id missing from RETURNING")
}

/// Seed an `art_7p_trips` row for `user_id`.
async fn seed_art_7p_trip(migrate_pool: &PgPool, user_id: Uuid, tag: &str) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO art_7p_trips (
            user_id, destination_country, from_date, to_date,
            employer_paid, purpose, eligibility_criteria
        )
        VALUES ($1, 'FR', DATE '2026-03-10', DATE '2026-03-17',
                true, $2, '{}'::jsonb)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(format!("trip-{tag}"))
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed art_7p_trip for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("art_7p_trips.id missing from RETURNING")
}

/// Seed a `modelo_720_user_inputs` row for `user_id` × `category`. The
/// partial unique index requires at most one `to_date IS NULL` row per
/// (user, category), so the `tag` merely discriminates logs.
async fn seed_m720_input(migrate_pool: &PgPool, user_id: Uuid, category: &str, tag: &str) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO modelo_720_user_inputs (
            user_id, category, amount_eur, reference_date, from_date, to_date
        )
        VALUES ($1, $2, 123456.78, CURRENT_DATE, CURRENT_DATE, NULL)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(category)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed m720 input for {user_id} ({tag}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("modelo_720_user_inputs.id missing from RETURNING")
}

/// Probe 8 — `espp_purchases` isolation. Seed one ESPP grant per user, one
/// purchase per grant, then verify `Tx::for_user(A)` cannot SEE user B's
/// purchase row.
#[tokio::test]
async fn espp_purchases_cross_tenant_select_is_isolated() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_espp").await;
    ensure_user(&migrate_pool, user_b, "b_espp").await;
    let ga = seed_espp_grant(&migrate_pool, user_a, "a_espp", None).await;
    let gb = seed_espp_grant(&migrate_pool, user_b, "b_espp", None).await;
    let pa = seed_espp_purchase(&migrate_pool, user_a, ga, "a_espp").await;
    let pb = seed_espp_purchase(&migrate_pool, user_b, gb, "b_espp").await;

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");

    let rows = sqlx::query("SELECT id, user_id FROM espp_purchases WHERE id IN ($1, $2)")
        .bind(pa)
        .bind(pb)
        .fetch_all(tx.as_executor())
        .await
        .expect("select espp_purchases under RLS");

    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on espp_purchases under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    let seen_user: Uuid = rows[0].try_get("user_id").expect("row user_id");
    assert_eq!(
        seen_id, pa,
        "RLS leak: Tx::for_user({user_a}) returned user B's purchase id {seen_id} (expected {pa})"
    );
    assert_eq!(
        seen_user, user_a,
        "RLS leak: Tx::for_user({user_a}) returned an espp_purchases row for user {seen_user}"
    );

    tx.rollback().await.expect("rollback");
    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 9 — `art_7p_trips` isolation. Same shape as Probe 8.
#[tokio::test]
async fn art_7p_trips_cross_tenant_select_is_isolated() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_trip").await;
    ensure_user(&migrate_pool, user_b, "b_trip").await;
    let tid_a = seed_art_7p_trip(&migrate_pool, user_a, "a_trip").await;
    let tid_b = seed_art_7p_trip(&migrate_pool, user_b, "b_trip").await;

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");

    let rows = sqlx::query("SELECT id, user_id FROM art_7p_trips WHERE id IN ($1, $2)")
        .bind(tid_a)
        .bind(tid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select art_7p_trips under RLS");

    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on art_7p_trips under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    let seen_user: Uuid = rows[0].try_get("user_id").expect("row user_id");
    assert_eq!(
        seen_id, tid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's trip id {seen_id} (expected {tid_a})"
    );
    assert_eq!(
        seen_user, user_a,
        "RLS leak: Tx::for_user({user_a}) returned an art_7p_trips row for user {seen_user}"
    );

    tx.rollback().await.expect("rollback");
    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 10 — `modelo_720_user_inputs` cross-tenant mutation is blocked.
/// The close-and-create helper runs UPDATE + INSERT; if RLS ever loosened,
/// a Tx::for_user(B) could close user A's open row. RLS USING gates the
/// UPDATE's row-set to zero matches; RLS WITH CHECK gates a sneaky
/// successor INSERT pointing at another user.
#[tokio::test]
async fn modelo_720_inputs_cross_tenant_mutation_cannot_touch_other_tenants_rows() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_m720").await;
    ensure_user(&migrate_pool, user_b, "b_m720").await;
    let rid_a = seed_m720_input(&migrate_pool, user_a, "bank_accounts", "a_m720").await;
    let rid_b = seed_m720_input(&migrate_pool, user_b, "bank_accounts", "b_m720").await;

    // --- SELECT probe
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) select");
    let rows = sqlx::query("SELECT id, user_id FROM modelo_720_user_inputs WHERE id IN ($1, $2)")
        .bind(rid_a)
        .bind(rid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select m720 under RLS");
    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on modelo_720_user_inputs under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    assert_eq!(
        seen_id, rid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's m720 row {seen_id}"
    );
    tx.rollback().await.expect("rollback select probe");

    // --- UPDATE probe (cross-tenant UPDATE of the open-row's to_date)
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) update");
    let updated =
        sqlx::query("UPDATE modelo_720_user_inputs SET to_date = CURRENT_DATE WHERE id = $1")
            .bind(rid_b)
            .execute(tx.as_executor())
            .await
            .expect("cross-tenant UPDATE should return 0 rows, not error");
    assert_eq!(
        updated.rows_affected(),
        0,
        "RLS leak: UPDATE from Tx::for_user({user_a}) affected {} rows on user B's m720 row {rid_b}.",
        updated.rows_affected()
    );
    tx.commit().await.expect("commit empty update");

    // --- DELETE probe
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) delete");
    let deleted = sqlx::query("DELETE FROM modelo_720_user_inputs WHERE id = $1")
        .bind(rid_b)
        .execute(tx.as_executor())
        .await
        .expect("cross-tenant DELETE should return 0 rows, not error");
    assert_eq!(
        deleted.rows_affected(),
        0,
        "RLS leak: DELETE from Tx::for_user({user_a}) removed {} rows on user B's m720 row {rid_b}.",
        deleted.rows_affected()
    );
    tx.commit().await.expect("commit empty delete");

    // Sanity: user B's row is still open (to_date IS NULL) and untouched.
    let surviving = sqlx::query("SELECT to_date FROM modelo_720_user_inputs WHERE id = $1")
        .bind(rid_b)
        .fetch_optional(&migrate_pool)
        .await
        .expect("post-check lookup");
    let surviving =
        surviving.expect("user B's m720 row must still exist after cross-tenant DELETE");
    let to_date: Option<chrono::NaiveDate> = surviving.try_get("to_date").expect("to_date");
    assert!(
        to_date.is_none(),
        "RLS leak: user B's m720 row to_date was mutated to {to_date:?} by a Tx::for_user(user_a) UPDATE"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 11 — session revoke cannot cross tenants. Exercises the Slice-2
/// `sessions_mgmt::revoke_other` and `revoke_all_others` helpers: a
/// Tx::for_user(A) call targeting user B's session must return NotFound
/// (single revoke) or skip user B's row entirely (bulk revoke), and user
/// B's session must remain active.
#[tokio::test]
async fn sessions_revoke_other_cannot_cross_tenant() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_rev").await;
    ensure_user(&migrate_pool, user_b, "b_rev").await;
    // User A's own current session + a target session belonging to user B.
    let _sid_a_current = seed_session(&migrate_pool, user_a, "a_rev_current").await;
    let sid_b_target = seed_session(&migrate_pool, user_b, "b_rev_target").await;

    // Call the revoke helper as user A targeting user B's session id. Pass
    // a distinct current_session_id (not the target, not nil) so we
    // exercise the cross-tenant RLS guard rather than the self-revoke
    // early return.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");
    let outcome =
        orbit_db::sessions_mgmt::revoke_other(&mut tx, user_a, sid_b_target, Uuid::new_v4())
            .await
            .expect("revoke_other cross-tenant call");
    assert_eq!(
        outcome,
        orbit_db::sessions_mgmt::RevokeOtherOutcome::NotFound,
        "RLS leak: revoke_other({user_a}, target=user B's {sid_b_target}) reported {outcome:?}; \
         expected NotFound (0 rows updated under RLS)."
    );
    tx.commit().await.expect("commit empty revoke");

    // Sanity: user B's session is still active (revoked_at IS NULL).
    let row = sqlx::query("SELECT revoked_at FROM sessions WHERE id = $1")
        .bind(sid_b_target)
        .fetch_one(&migrate_pool)
        .await
        .expect("post-check lookup");
    let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("revoked_at").expect("revoked_at");
    assert!(
        revoked_at.is_none(),
        "RLS leak: user B's session {sid_b_target} was revoked by a Tx::for_user(user_a) call; \
         revoked_at = {revoked_at:?}"
    );

    // Also verify revoke_all_others from user A's tx does not touch user
    // B's session. Pass nil as the current-session-id so the predicate
    // `id <> $2` matches every row in user A's RLS scope; the count must
    // include only user A's own seeded session (1), never user B's.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) bulk");
    let count = orbit_db::sessions_mgmt::revoke_all_others(&mut tx, user_a, Uuid::nil())
        .await
        .expect("revoke_all_others");
    assert_eq!(
        count, 1,
        "revoke_all_others under Tx::for_user({user_a}) revoked {count} sessions; expected 1. \
         If this is 2, user B's session leaked into the RLS scope."
    );
    tx.commit().await.expect("commit bulk revoke");

    let row = sqlx::query("SELECT revoked_at FROM sessions WHERE id = $1")
        .bind(sid_b_target)
        .fetch_one(&migrate_pool)
        .await
        .expect("post-check lookup 2");
    let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("revoked_at").expect("revoked_at");
    assert!(
        revoked_at.is_none(),
        "RLS leak: user B's session {sid_b_target} was revoked by \
         Tx::for_user(user_a)::revoke_all_others; revoked_at = {revoked_at:?}"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 12 — ESPP notes-lift on first purchase. Seed a Slice-1 ESPP grant
/// with `grants.notes = '{"estimated_discount_percent": 15}'`; call
/// [`orbit_db::espp_purchases::migrate_notes_on_first_purchase`]; assert the
/// helper returns `Some(NotesMigration { lifted_discount_percent = "15.00",
/// preserved_user_note = None })` and `grants.notes` is now NULL.
///
/// This is the integration-side check for AC-4.5.1 / ADR-016 §2. The parser
/// half is already covered by unit tests in `espp_purchases.rs`; this probe
/// closes the SQL-round-trip half (RLS scoping, UPDATE visibility,
/// transaction boundary).
#[tokio::test]
async fn espp_notes_lift_first_purchase_rewrites_grant_notes() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_lift").await;
    let grant_id = seed_espp_grant(
        &migrate_pool,
        user_a,
        "a_lift",
        Some(r#"{"estimated_discount_percent":15}"#),
    )
    .await;

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");
    let migration =
        orbit_db::espp_purchases::migrate_notes_on_first_purchase(&mut tx, user_a, grant_id)
            .await
            .expect("migrate_notes_on_first_purchase");
    tx.commit().await.expect("commit lift");

    let migration = migration.expect("Slice-1 JSON should parse and trigger the lift");
    assert_eq!(
        migration.lifted_discount_percent, "15.00",
        "lifted_discount_percent should normalize to 15.00 (NUMERIC(5,2) shape)"
    );
    assert_eq!(
        migration.preserved_user_note, None,
        "no `note` key in the source JSON → preserved_user_note must be None"
    );

    // grants.notes should now be NULL. Read via the migrate pool (owner,
    // bypasses RLS) so a misconfigured RLS policy cannot mask a NOT-NULL
    // surviving payload.
    let row = sqlx::query("SELECT notes FROM grants WHERE id = $1")
        .bind(grant_id)
        .fetch_one(&migrate_pool)
        .await
        .expect("post-lift grants lookup");
    let notes: Option<String> = row.try_get("notes").expect("notes column");
    assert_eq!(
        notes, None,
        "grants.notes must be NULL after the lift; got {notes:?}"
    );

    cleanup_user(&migrate_pool, user_a).await;
}

// ---------------------------------------------------------------------------
// Slice 3 T28 — cross-tenant probes for ticker_current_prices,
// grant_current_price_overrides, and the vesting-event override surface.
// fx_rates is shared reference data — NOT RLS-scoped — so it has no probe
// (ADR-017 §1 "Why `fx_rates` is NOT RLS-scoped").
// ---------------------------------------------------------------------------

/// Seed a `ticker_current_prices` row via the migrate pool (owner
/// bypasses the permissive RLS).
async fn seed_ticker_price(
    migrate_pool: &PgPool,
    user_id: Uuid,
    ticker: &str,
    price: &str,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO ticker_current_prices (user_id, ticker, price, currency)
        VALUES ($1, $2, $3::numeric, 'USD')
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(ticker)
    .bind(price)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed ticker price for {user_id} ({ticker}) failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("ticker_current_prices.id missing from RETURNING")
}

/// Seed a `grant_current_price_overrides` row for `grant_id`.
async fn seed_grant_price_override(
    migrate_pool: &PgPool,
    user_id: Uuid,
    grant_id: Uuid,
    price: &str,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO grant_current_price_overrides (user_id, grant_id, price, currency)
        VALUES ($1, $2, $3::numeric, 'USD')
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .bind(price)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed grant price override for {user_id} failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("grant_current_price_overrides.id missing from RETURNING")
}

/// Probe 13 — `ticker_current_prices` isolation. User A upserts a row;
/// user B's tx cannot SELECT it.
#[tokio::test]
async fn ticker_current_prices_cross_tenant_select_is_isolated() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_tp").await;
    ensure_user(&migrate_pool, user_b, "b_tp").await;
    let pid_a = seed_ticker_price(&migrate_pool, user_a, "ACME", "42.00").await;
    let pid_b = seed_ticker_price(&migrate_pool, user_b, "ACME", "99.99").await;

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a)");

    let rows = sqlx::query("SELECT id, user_id FROM ticker_current_prices WHERE id IN ($1, $2)")
        .bind(pid_a)
        .bind(pid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select ticker_current_prices under RLS");

    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on ticker_current_prices under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    let seen_user: Uuid = rows[0].try_get("user_id").expect("row user_id");
    assert_eq!(
        seen_id, pid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's ticker_current_prices id {seen_id} \
         (expected {pid_a})"
    );
    assert_eq!(
        seen_user, user_a,
        "RLS leak: Tx::for_user({user_a}) returned a ticker_current_prices row for user {seen_user}"
    );

    tx.rollback().await.expect("rollback");
    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Probe 14 — `grant_current_price_overrides` mutation isolation. User A
/// has an override on A's grant; user B's tx cannot UPDATE/DELETE it.
#[tokio::test]
async fn grant_current_price_overrides_cross_tenant_mutation_cannot_touch_other_tenants_rows() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_gpo").await;
    ensure_user(&migrate_pool, user_b, "b_gpo").await;
    let ga = seed_grant(&migrate_pool, user_a, "a_gpo").await;
    let gb = seed_grant(&migrate_pool, user_b, "b_gpo").await;
    let oid_a = seed_grant_price_override(&migrate_pool, user_a, ga, "42.00").await;
    let oid_b = seed_grant_price_override(&migrate_pool, user_b, gb, "99.99").await;

    // --- SELECT probe
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) select");
    let rows =
        sqlx::query("SELECT id, user_id FROM grant_current_price_overrides WHERE id IN ($1, $2)")
            .bind(oid_a)
            .bind(oid_b)
            .fetch_all(tx.as_executor())
            .await
            .expect("select grant_current_price_overrides under RLS");
    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on grant_current_price_overrides under Tx::for_user({user_a}) returned \
         {} rows for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    assert_eq!(
        seen_id, oid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's override id {seen_id}"
    );
    tx.rollback().await.expect("rollback select probe");

    // --- UPDATE probe (target user B's override)
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) update");
    let updated =
        sqlx::query("UPDATE grant_current_price_overrides SET price = 1.00::numeric WHERE id = $1")
            .bind(oid_b)
            .execute(tx.as_executor())
            .await
            .expect("cross-tenant UPDATE should return 0 rows, not error");
    assert_eq!(
        updated.rows_affected(),
        0,
        "RLS leak: UPDATE from Tx::for_user({user_a}) affected {} rows on user B's override {oid_b}.",
        updated.rows_affected()
    );
    tx.commit().await.expect("commit empty update");

    // --- DELETE probe (target user B's override)
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) delete");
    let deleted = sqlx::query("DELETE FROM grant_current_price_overrides WHERE id = $1")
        .bind(oid_b)
        .execute(tx.as_executor())
        .await
        .expect("cross-tenant DELETE should return 0 rows, not error");
    assert_eq!(
        deleted.rows_affected(),
        0,
        "RLS leak: DELETE from Tx::for_user({user_a}) removed {} rows on user B's override {oid_b}.",
        deleted.rows_affected()
    );
    tx.commit().await.expect("commit empty delete");

    // Sanity: user B's override is still present with its original price.
    let surviving = sqlx::query(
        "SELECT price::text AS price_text FROM grant_current_price_overrides WHERE id = $1",
    )
    .bind(oid_b)
    .fetch_optional(&migrate_pool)
    .await
    .expect("post-check lookup");
    let surviving = surviving.expect(
        "user B's grant_current_price_overrides row must still exist after cross-tenant DELETE",
    );
    let price: String = surviving.try_get("price_text").expect("price");
    assert!(
        price.starts_with("99.99"),
        "RLS leak: user B's grant_current_price_overrides.price mutated to {price:?} by a \
         Tx::for_user(user_a) UPDATE"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Seed a vesting_events row with an active FMV override for
/// `user_id`/`grant_id`, bypassing the permissive RLS via the migrate
/// pool. Mirrors the slice-3 "user has already saved a manual override"
/// state used by the override probe.
async fn seed_overridden_vesting_event(
    migrate_pool: &PgPool,
    user_id: Uuid,
    grant_id: Uuid,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO vesting_events (
            user_id, grant_id, vest_date,
            shares_vested_this_event, cumulative_shares_vested, state,
            fmv_at_vest, fmv_currency, is_user_override, overridden_at
        )
        VALUES ($1, $2, DATE '2025-09-15',
                250, 250, 'vested',
                42.00::numeric, 'USD', true, now())
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed overridden vesting_event for {user_id} failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("vesting_events.id missing from RETURNING")
}

/// Probe 15 — vesting-event override is isolated across tenants.
/// User A creates a grant + an override; user B's tx calling
/// `apply_override` on A's event returns `Conflict` (RLS denies the
/// UPDATE's row-set), and user B's tx calling SELECT sees zero rows.
#[tokio::test]
async fn vesting_events_override_cross_tenant_is_isolated() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_vo").await;
    ensure_user(&migrate_pool, user_b, "b_vo").await;
    let ga = seed_grant(&migrate_pool, user_a, "a_vo").await;
    let eid_a = seed_overridden_vesting_event(&migrate_pool, user_a, ga).await;

    // Read the current `updated_at` (as the owner) so we can pass a
    // realistic-looking token to apply_override. The point of the
    // probe is that RLS gates the UPDATE's row set to zero regardless
    // of whether the token matches.
    let row = sqlx::query("SELECT updated_at FROM vesting_events WHERE id = $1")
        .bind(eid_a)
        .fetch_one(&migrate_pool)
        .await
        .expect("read updated_at");
    let updated_at: chrono::DateTime<chrono::Utc> = row.try_get("updated_at").expect("updated_at");

    // --- SELECT probe: user B sees zero rows for A's event.
    let mut tx = Tx::for_user(&app_pool, user_b)
        .await
        .expect("Tx::for_user(user_b) select");
    let rows = sqlx::query("SELECT id FROM vesting_events WHERE id = $1")
        .bind(eid_a)
        .fetch_all(tx.as_executor())
        .await
        .expect("select vesting_events under RLS as user_b");
    assert_eq!(
        rows.len(),
        0,
        "RLS leak: Tx::for_user({user_b}) saw {} rows for user A's vesting_event {eid_a}; \
         expected 0.",
        rows.len()
    );
    tx.rollback().await.expect("rollback select");

    // --- apply_override probe: from user B's tx, target user A's event.
    // RLS USING on UPDATE matches zero rows → OverrideOutcome::Conflict.
    let mut tx = Tx::for_user(&app_pool, user_b)
        .await
        .expect("Tx::for_user(user_b) override");
    let patch = orbit_db::vesting_events::VestingEventOverridePatch {
        fmv_at_vest: Some(Some("1.00".to_string())),
        fmv_currency: Some(Some("USD".to_string())),
        ..Default::default()
    };
    let outcome =
        orbit_db::vesting_events::apply_override(&mut tx, user_b, eid_a, &patch, updated_at)
            .await
            .expect("apply_override call");
    assert_eq!(
        outcome,
        orbit_db::vesting_events::OverrideOutcome::Conflict,
        "RLS leak: apply_override from Tx::for_user({user_b}) on user A's event {eid_a} \
         returned {outcome:?}; expected Conflict (0 rows matched under RLS)."
    );
    tx.commit().await.expect("commit empty override");

    // Sanity: user A's event is still present with its original FMV
    // (read via the migrate pool — owner bypasses RLS).
    let surviving = sqlx::query(
        "SELECT fmv_at_vest::text AS fmv_text, is_user_override FROM vesting_events WHERE id = $1",
    )
    .bind(eid_a)
    .fetch_one(&migrate_pool)
    .await
    .expect("post-check lookup");
    let fmv: String = surviving.try_get("fmv_text").expect("fmv_text");
    let flag: bool = surviving
        .try_get("is_user_override")
        .expect("is_user_override");
    assert!(
        fmv.starts_with("42."),
        "RLS leak: user A's vesting_event.fmv_at_vest mutated to {fmv:?} by a Tx::for_user(user_b) \
         apply_override call"
    );
    assert!(
        flag,
        "user A's is_user_override flag must still be true after the cross-tenant no-op"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

// ---------------------------------------------------------------------------
// Slice 3 T28 — apply_override + clear_override round-trip unit test.
// Validates that the override columns advance together and that
// clear_override preserves FMV per AC-8.7.1.
// ---------------------------------------------------------------------------

/// Probe 16 — override round-trip. Apply an override (vest_date +
/// shares + FMV), verify the four override columns advance; call
/// clear_override and verify per AC-8.7.1 that (a) vest_date and
/// shares revert to the algorithm's output, (b) FMV is preserved, and
/// (c) is_user_override stays `true` because the FMV itself is still
/// a manual edit.
///
/// Matches the T28 task's "apply + verify flags, clear + verify FMV
/// preservation" ask but follows AC-8.7.1 (d) precisely: when the row
/// still carries an FMV after the clear, the override flag stays true.
#[tokio::test]
async fn vesting_events_override_round_trip_preserves_fmv() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_rt").await;
    // seed_grant creates an RSU grant with vesting_start = 2024-09-15,
    // 48 months, 12-month cliff, monthly, 1000 shares. Algorithm output
    // for the 2025-09-15 cliff row: 250 shares (12/48 * 1000).
    let grant_id = seed_grant(&migrate_pool, user_a, "a_rt").await;
    // Seed the cliff row with the algorithmic values so apply_override
    // has a row to mutate.
    let event_id = seed_vesting_event(&migrate_pool, user_a, grant_id, "a_rt").await;

    // Read initial updated_at.
    let row = sqlx::query("SELECT updated_at FROM vesting_events WHERE id = $1")
        .bind(event_id)
        .fetch_one(&migrate_pool)
        .await
        .expect("read initial updated_at");
    let initial_updated_at: chrono::DateTime<chrono::Utc> =
        row.try_get("updated_at").expect("updated_at");

    // --- apply_override: change vest_date + shares + fmv.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) apply");
    let patch = orbit_db::vesting_events::VestingEventOverridePatch {
        vest_date: Some(chrono::NaiveDate::from_ymd_opt(2025, 10, 1).unwrap()),
        shares_vested_this_event: Some(300 * orbit_core::SHARES_SCALE),
        fmv_at_vest: Some(Some("42.5000".to_string())),
        fmv_currency: Some(Some("USD".to_string())),
    };
    let outcome = orbit_db::vesting_events::apply_override(
        &mut tx,
        user_a,
        event_id,
        &patch,
        initial_updated_at,
    )
    .await
    .expect("apply_override call");
    tx.commit().await.expect("commit apply");

    let applied = match outcome {
        orbit_db::vesting_events::OverrideOutcome::Applied(r) => r,
        orbit_db::vesting_events::OverrideOutcome::Conflict => {
            panic!("apply_override returned Conflict on fresh token; expected Applied")
        }
    };
    assert!(
        applied.is_user_override,
        "is_user_override must be true after apply_override"
    );
    assert!(
        applied.overridden_at.is_some(),
        "overridden_at must be non-null after apply_override"
    );
    assert!(
        applied.updated_at > initial_updated_at,
        "updated_at must advance after apply_override: {:?} vs initial {:?}",
        applied.updated_at,
        initial_updated_at
    );
    assert_eq!(
        applied.vest_date,
        chrono::NaiveDate::from_ymd_opt(2025, 10, 1).unwrap(),
        "vest_date must reflect the patch"
    );
    assert_eq!(
        applied.shares_vested_this_event,
        300 * orbit_core::SHARES_SCALE,
        "shares_vested_this_event must reflect the patch"
    );
    assert!(
        applied
            .fmv_at_vest
            .as_deref()
            .map(|s| s.starts_with("42.5"))
            .unwrap_or(false),
        "fmv_at_vest must be 42.5000 (got {:?})",
        applied.fmv_at_vest
    );
    assert_eq!(applied.fmv_currency.as_deref(), Some("USD"));

    // --- clear_override: revert date + shares; preserve FMV.
    // `today = 2030-01-01` pins every derived row to a state that isn't
    // Upcoming; the 2025-09-15 cliff row is what the algorithm outputs.
    // `applied.updated_at` is the fresh OCC cookie after the preceding
    // apply_override — clear_override requires it per AC-10.5.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) clear");
    let today = chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap();
    let clear_outcome = orbit_db::vesting_events::clear_override(
        &mut tx,
        user_a,
        event_id,
        today,
        applied.updated_at,
    )
    .await
    .expect("clear_override call");
    tx.commit().await.expect("commit clear");

    let cleared = match clear_outcome {
        orbit_db::vesting_events::ClearOutcome::Cleared(r) => r,
        other => panic!("clear_override returned {other:?}; expected Cleared"),
    };

    // vest_date + shares revert to the algorithm output.
    assert_eq!(
        cleared.vest_date,
        chrono::NaiveDate::from_ymd_opt(2025, 9, 15).unwrap(),
        "clear_override must revert vest_date to the algorithm output (2025-09-15 cliff)"
    );
    assert_eq!(
        cleared.shares_vested_this_event,
        250 * orbit_core::SHARES_SCALE,
        "clear_override must revert shares to the algorithm output (250 at cliff)"
    );

    // FMV is preserved per AC-8.7.1 (b).
    assert!(
        cleared
            .fmv_at_vest
            .as_deref()
            .map(|s| s.starts_with("42.5"))
            .unwrap_or(false),
        "FMV must be preserved after clear_override (got {:?})",
        cleared.fmv_at_vest
    );
    assert_eq!(cleared.fmv_currency.as_deref(), Some("USD"));

    // Per AC-8.7.1 (d): the row still carries an FMV, so the override
    // flag stays true and overridden_at stays non-null (coherence
    // CHECK requires it).
    assert!(
        cleared.is_user_override,
        "is_user_override must remain true when FMV is still set (AC-8.7.1 d)"
    );
    assert!(
        cleared.overridden_at.is_some(),
        "overridden_at must remain non-null while is_user_override is true \
         (override_flag_coherent CHECK)"
    );

    cleanup_user(&migrate_pool, user_a).await;
}

/// Probe 17 — clear_override on a row with no FMV drops the override
/// flag cleanly. Covers the AC-8.7.1 (c) branch of the biconditional.
#[tokio::test]
async fn vesting_events_clear_override_without_fmv_resets_flags() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_rt_nofmv").await;
    let grant_id = seed_grant(&migrate_pool, user_a, "a_rt_nofmv").await;
    let event_id = seed_vesting_event(&migrate_pool, user_a, grant_id, "a_rt_nofmv").await;

    // Read initial updated_at.
    let row = sqlx::query("SELECT updated_at FROM vesting_events WHERE id = $1")
        .bind(event_id)
        .fetch_one(&migrate_pool)
        .await
        .expect("read initial updated_at");
    let initial_updated_at: chrono::DateTime<chrono::Utc> =
        row.try_get("updated_at").expect("updated_at");

    // Apply an override that changes vest_date + shares but leaves FMV unset.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user apply");
    let patch = orbit_db::vesting_events::VestingEventOverridePatch {
        vest_date: Some(chrono::NaiveDate::from_ymd_opt(2025, 10, 1).unwrap()),
        shares_vested_this_event: Some(300 * orbit_core::SHARES_SCALE),
        fmv_at_vest: None,
        fmv_currency: None,
    };
    let outcome = orbit_db::vesting_events::apply_override(
        &mut tx,
        user_a,
        event_id,
        &patch,
        initial_updated_at,
    )
    .await
    .expect("apply_override");
    tx.commit().await.expect("commit apply");
    let applied = match outcome {
        orbit_db::vesting_events::OverrideOutcome::Applied(r) => r,
        other => panic!("expected Applied, got {other:?}"),
    };
    assert!(applied.fmv_at_vest.is_none());
    assert!(applied.is_user_override);

    // Now clear; expect full reset since FMV is NULL. `applied.updated_at`
    // is the fresh OCC cookie the clear call requires per AC-10.5.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user clear");
    let today = chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap();
    let clear_outcome = orbit_db::vesting_events::clear_override(
        &mut tx,
        user_a,
        event_id,
        today,
        applied.updated_at,
    )
    .await
    .expect("clear_override");
    tx.commit().await.expect("commit clear");

    let cleared = match clear_outcome {
        orbit_db::vesting_events::ClearOutcome::Cleared(r) => r,
        other => panic!("expected Cleared, got {other:?}"),
    };
    assert!(
        !cleared.is_user_override,
        "is_user_override must reset to false when no FMV remains (AC-8.7.1 c)"
    );
    assert!(
        cleared.overridden_at.is_none(),
        "overridden_at must reset to NULL when no FMV remains (override_flag_coherent CHECK)"
    );
    assert!(
        cleared.fmv_at_vest.is_none(),
        "fmv_at_vest must remain NULL"
    );

    cleanup_user(&migrate_pool, user_a).await;
}

// ---------------------------------------------------------------------------
// Slice 3 T33 Sec-S2 — `Tx::system` primes `app.user_id` to the nil UUID
// so any accidental SELECT from an RLS-scoped table fails closed (zero
// rows) rather than raising `invalid input syntax for type uuid`.
// ---------------------------------------------------------------------------

/// Probe — `Tx::system` primes `app.user_id` to the nil UUID (string).
#[tokio::test]
async fn tx_system_primes_app_user_id_to_nil_uuid() {
    let app_pool = pool_from_env("DATABASE_URL").await;

    let mut tx = Tx::system(&app_pool).await.expect("Tx::system");
    let row = sqlx::query("SELECT current_setting('app.user_id', true) AS v")
        .fetch_one(tx.as_executor())
        .await
        .expect("read guc");
    let v: String = row.try_get("v").expect("get v");
    assert_eq!(
        v,
        Uuid::nil().to_string(),
        "Tx::system must prime app.user_id to the nil UUID so RLS fails closed"
    );
    tx.rollback().await.expect("rollback");
}

/// Probe — `Tx::system` SELECT on an RLS-scoped table returns zero rows
/// (not an `invalid input syntax for type uuid` error), even when seeded
/// rows exist for real users. This is the fail-closed guarantee that
/// keeps the escape hatch safe against accidental misuse.
#[tokio::test]
async fn tx_system_select_on_rls_table_returns_zero_rows() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_sys_nil").await;
    let sid_a = seed_session(&migrate_pool, user_a, "a_sys_nil").await;

    let mut tx = Tx::system(&app_pool).await.expect("Tx::system");
    let rows = sqlx::query("SELECT id FROM sessions WHERE id = $1")
        .bind(sid_a)
        .fetch_all(tx.as_executor())
        .await
        .expect(
            "SELECT under Tx::system must succeed (nil-UUID prime keeps \
             the RLS uuid-cast from raising) and simply return zero rows",
        );

    assert_eq!(
        rows.len(),
        0,
        "Tx::system must see zero RLS-scoped rows: any other count means \
         the nil-UUID prime landed on a real user's rows or RLS is misconfigured"
    );

    tx.rollback().await.expect("rollback");
    cleanup_user(&migrate_pool, user_a).await;
}

// ---------------------------------------------------------------------------
// Slice 3b T37 — cross-tenant probes for user_tax_preferences + the
// sell-to-cover override surface on vesting_events. Unit tests covering
// the four UpsertOutcome branches, resolve_at_date, the sell-to-cover
// round-trip, and the extended clear_override behavior also live here
// (they need a live Postgres so the integration-tests gate is correct).
// ---------------------------------------------------------------------------

/// Seed a `user_tax_preferences` row for `user_id` via the migrate pool
/// (owner bypasses the permissive RLS).
async fn seed_user_tax_pref(
    migrate_pool: &PgPool,
    user_id: Uuid,
    country: &str,
    percent: Option<&str>,
    enabled: bool,
    from_date: chrono::NaiveDate,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO user_tax_preferences (
            user_id, country_iso2, rendimiento_del_trabajo_percent,
            sell_to_cover_enabled, from_date, to_date
        )
        VALUES ($1, $2, $3::numeric, $4, $5, NULL)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(country)
    .bind(percent)
    .bind(enabled)
    .bind(from_date)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed user_tax_preferences for {user_id} failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("user_tax_preferences.id missing from RETURNING")
}

/// Probe — `user_tax_preferences` isolation. User A saves a row; user
/// B's tx cannot SELECT, UPDATE, or DELETE it.
#[tokio::test]
async fn user_tax_preferences_cross_tenant_select_is_isolated() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_utp").await;
    ensure_user(&migrate_pool, user_b, "b_utp").await;
    let today = chrono::NaiveDate::from_ymd_opt(2026, 4, 19).unwrap();
    let rid_a = seed_user_tax_pref(&migrate_pool, user_a, "ES", Some("0.4500"), true, today).await;
    let rid_b = seed_user_tax_pref(&migrate_pool, user_b, "PT", None, false, today).await;

    // --- SELECT probe under user A: sees only A's row.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) select");
    let rows = sqlx::query("SELECT id, user_id FROM user_tax_preferences WHERE id IN ($1, $2)")
        .bind(rid_a)
        .bind(rid_b)
        .fetch_all(tx.as_executor())
        .await
        .expect("select user_tax_preferences under RLS");
    assert_eq!(
        rows.len(),
        1,
        "RLS leak: SELECT on user_tax_preferences under Tx::for_user({user_a}) returned {} rows \
         for (a, b) pair, expected exactly 1.",
        rows.len()
    );
    let seen_id: Uuid = rows[0].try_get("id").expect("row id");
    let seen_user: Uuid = rows[0].try_get("user_id").expect("row user_id");
    assert_eq!(
        seen_id, rid_a,
        "RLS leak: Tx::for_user({user_a}) returned user B's user_tax_preferences id {seen_id}"
    );
    assert_eq!(
        seen_user, user_a,
        "RLS leak: Tx::for_user({user_a}) returned a user_tax_preferences row for user {seen_user}"
    );
    tx.rollback().await.expect("rollback select probe");

    // --- repo current() under user B: does NOT see user A's row.
    let mut tx = Tx::for_user(&app_pool, user_b)
        .await
        .expect("Tx::for_user(user_b) repo");
    let seen_by_b = orbit_db::user_tax_preferences::current(&mut tx, user_a)
        .await
        .expect("current()");
    assert!(
        seen_by_b.is_none(),
        "RLS leak: user B's Tx saw user A's user_tax_preferences via current(): {seen_by_b:?}"
    );
    tx.rollback().await.expect("rollback repo probe");

    // --- UPDATE probe (cross-tenant UPDATE of the open-row's to_date).
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) update");
    let updated =
        sqlx::query("UPDATE user_tax_preferences SET to_date = CURRENT_DATE WHERE id = $1")
            .bind(rid_b)
            .execute(tx.as_executor())
            .await
            .expect("cross-tenant UPDATE should return 0 rows, not error");
    assert_eq!(
        updated.rows_affected(),
        0,
        "RLS leak: UPDATE from Tx::for_user({user_a}) affected {} rows on user B's \
         user_tax_preferences row {rid_b}.",
        updated.rows_affected()
    );
    tx.commit().await.expect("commit empty update");

    // --- DELETE probe.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) delete");
    let deleted = sqlx::query("DELETE FROM user_tax_preferences WHERE id = $1")
        .bind(rid_b)
        .execute(tx.as_executor())
        .await
        .expect("cross-tenant DELETE should return 0 rows, not error");
    assert_eq!(
        deleted.rows_affected(),
        0,
        "RLS leak: DELETE from Tx::for_user({user_a}) removed {} rows on user B's \
         user_tax_preferences row {rid_b}.",
        deleted.rows_affected()
    );
    tx.commit().await.expect("commit empty delete");

    // Sanity: user B's row still exists with its original payload.
    let surviving = sqlx::query(
        "SELECT country_iso2, sell_to_cover_enabled, to_date \
           FROM user_tax_preferences WHERE id = $1",
    )
    .bind(rid_b)
    .fetch_optional(&migrate_pool)
    .await
    .expect("post-check lookup");
    let surviving = surviving
        .expect("user B's user_tax_preferences row must still exist after cross-tenant DELETE");
    let country: String = surviving.try_get("country_iso2").expect("country_iso2");
    let to_date: Option<chrono::NaiveDate> = surviving.try_get("to_date").expect("to_date");
    assert_eq!(country, "PT");
    assert!(
        to_date.is_none(),
        "RLS leak: user B's user_tax_preferences row to_date was mutated by cross-tenant UPDATE"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

/// Seed a vesting_events row with a sell-to-cover override already
/// applied for `user_id`/`grant_id`. Used by the cross-tenant probe
/// against [`apply_sell_to_cover_override`].
async fn seed_sell_to_cover_vesting_event(
    migrate_pool: &PgPool,
    user_id: Uuid,
    grant_id: Uuid,
) -> Uuid {
    let row = sqlx::query(
        r#"
        INSERT INTO vesting_events (
            user_id, grant_id, vest_date,
            shares_vested_this_event, cumulative_shares_vested, state,
            fmv_at_vest, fmv_currency,
            is_user_override, overridden_at,
            tax_withholding_percent, share_sell_price, share_sell_currency,
            is_sell_to_cover_override, sell_to_cover_overridden_at
        )
        VALUES ($1, $2, DATE '2025-09-15',
                250, 250, 'vested',
                42.00::numeric, 'USD',
                true, now(),
                0.4500::numeric, 42.25::numeric, 'USD',
                true, now())
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_one(migrate_pool)
    .await
    .unwrap_or_else(|e| panic!("seed sell-to-cover vesting_event for {user_id} failed: {e}"));

    row.try_get::<Uuid, _>("id")
        .expect("vesting_events.id missing from RETURNING")
}

/// Probe — `apply_sell_to_cover_override` from Tx::for_user(B) on
/// user A's event is RLS-filtered. The UPDATE matches zero rows, and
/// the helper returns `SellToCoverOutcome::NotFound` (user B's tx
/// cannot even SEE the row, so the post-update existence check also
/// returns None — NotFound is the correct branch). User A's row is
/// unmutated.
#[tokio::test]
async fn vesting_events_sell_to_cover_override_cross_tenant_cannot_apply() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    let user_b = Uuid::new_v4();

    ensure_user(&migrate_pool, user_a, "a_s2c").await;
    ensure_user(&migrate_pool, user_b, "b_s2c").await;
    let ga = seed_grant(&migrate_pool, user_a, "a_s2c").await;
    let eid_a = seed_sell_to_cover_vesting_event(&migrate_pool, user_a, ga).await;

    // Read the owner's updated_at token (so the probe is not masked by
    // an OCC mismatch — the point is that RLS alone gates the UPDATE's
    // row-set to zero).
    let row = sqlx::query("SELECT updated_at FROM vesting_events WHERE id = $1")
        .bind(eid_a)
        .fetch_one(&migrate_pool)
        .await
        .expect("read updated_at");
    let updated_at: chrono::DateTime<chrono::Utc> = row.try_get("updated_at").expect("updated_at");

    // --- apply_sell_to_cover_override from user B's tx, targeting A's event.
    let mut tx = Tx::for_user(&app_pool, user_b)
        .await
        .expect("Tx::for_user(user_b)");
    let patch = orbit_db::vesting_events::SellToCoverOverridePatch {
        tax_withholding_percent: Some(Some("0.9999".to_string())),
        share_sell_price: Some(Some("1.00".to_string())),
        share_sell_currency: Some(Some("USD".to_string())),
    };
    let outcome = orbit_db::vesting_events::apply_sell_to_cover_override(
        &mut tx, user_b, eid_a, &patch, updated_at,
    )
    .await
    .expect("apply_sell_to_cover_override call");
    assert_eq!(
        outcome,
        orbit_db::vesting_events::SellToCoverOutcome::NotFound,
        "RLS leak: apply_sell_to_cover_override from Tx::for_user({user_b}) on user A's \
         event {eid_a} returned {outcome:?}; expected NotFound (0 rows matched under RLS \
         + post-update existence check also RLS-filtered)."
    );
    tx.commit().await.expect("commit empty override");

    // --- clear_sell_to_cover_override from user B's tx, targeting A's event.
    let mut tx = Tx::for_user(&app_pool, user_b)
        .await
        .expect("Tx::for_user(user_b) clear");
    let clear_outcome =
        orbit_db::vesting_events::clear_sell_to_cover_override(&mut tx, user_b, eid_a, updated_at)
            .await
            .expect("clear_sell_to_cover_override call");
    assert_eq!(
        clear_outcome,
        orbit_db::vesting_events::SellToCoverClearOutcome::NotFound,
        "RLS leak: clear_sell_to_cover_override from Tx::for_user({user_b}) on user A's \
         event {eid_a} returned {clear_outcome:?}; expected NotFound."
    );
    tx.commit().await.expect("commit empty clear");

    // Sanity: user A's row still carries the original sell-to-cover
    // triplet and flags (read via the migrate pool — owner bypasses RLS).
    let surviving = sqlx::query(
        "SELECT tax_withholding_percent::text AS pct, share_sell_price::text AS price, \
                share_sell_currency, is_sell_to_cover_override \
           FROM vesting_events WHERE id = $1",
    )
    .bind(eid_a)
    .fetch_one(&migrate_pool)
    .await
    .expect("post-check lookup");
    let pct: String = surviving.try_get("pct").expect("pct");
    let price: String = surviving.try_get("price").expect("price");
    let cur: String = surviving.try_get("share_sell_currency").expect("cur");
    let flag: bool = surviving
        .try_get("is_sell_to_cover_override")
        .expect("is_sell_to_cover_override");
    assert!(
        pct.starts_with("0.45"),
        "RLS leak: user A's tax_withholding_percent mutated to {pct:?} by cross-tenant call"
    );
    assert!(
        price.starts_with("42.25"),
        "RLS leak: user A's share_sell_price mutated to {price:?} by cross-tenant call"
    );
    assert_eq!(cur, "USD");
    assert!(
        flag,
        "user A's is_sell_to_cover_override flag must still be true after the cross-tenant no-op"
    );

    cleanup_user(&migrate_pool, user_a).await;
    cleanup_user(&migrate_pool, user_b).await;
}

// ---------------------------------------------------------------------------
// Slice 3b T37 — user_tax_preferences::create_or_upsert_same_day +
// resolve_at_date round-trip unit tests.
// ---------------------------------------------------------------------------

/// Probe — `create_or_upsert_same_day` covers every UpsertOutcome
/// branch in one round-trip. Assert the partial UNIQUE holds (one
/// open row at a time).
#[tokio::test]
async fn user_tax_preferences_upsert_covers_every_outcome() {
    use orbit_db::user_tax_preferences as utp;

    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_utp_outcomes").await;

    // ---------- 1. Inserted (no prior row).
    let day1 = chrono::NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) insert");
    let form = utp::UserTaxPreferenceUpsertForm {
        country_iso2: "ES".into(),
        rendimiento_del_trabajo_percent: Some("0.4500".into()),
        sell_to_cover_enabled: true,
        today: day1,
    };
    let outcome = utp::create_or_upsert_same_day(&mut tx, user_a, &form)
        .await
        .expect("upsert inserted");
    let inserted_row = match outcome {
        utp::UpsertOutcome::Inserted(r) => r,
        other => panic!("expected Inserted, got {other:?}"),
    };
    assert_eq!(inserted_row.country_iso2, "ES");
    assert_eq!(
        inserted_row.rendimiento_del_trabajo_percent.as_deref(),
        Some("0.4500")
    );
    assert!(inserted_row.sell_to_cover_enabled);
    assert_eq!(inserted_row.from_date, day1);
    assert!(inserted_row.to_date.is_none());
    tx.commit().await.expect("commit insert");

    // ---------- 2. NoOp (identical same-day save).
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) noop");
    let outcome = utp::create_or_upsert_same_day(&mut tx, user_a, &form)
        .await
        .expect("upsert noop");
    let noop_row = match outcome {
        utp::UpsertOutcome::NoOp(r) => r,
        other => panic!("expected NoOp, got {other:?}"),
    };
    assert_eq!(
        noop_row.id, inserted_row.id,
        "NoOp must return the existing row id unchanged"
    );
    tx.commit().await.expect("commit noop");

    // ---------- 3. UpdatedSameDay (same-day, different percent).
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) same-day");
    let form_changed = utp::UserTaxPreferenceUpsertForm {
        country_iso2: "ES".into(),
        rendimiento_del_trabajo_percent: Some("0.4700".into()),
        sell_to_cover_enabled: true,
        today: day1,
    };
    let outcome = utp::create_or_upsert_same_day(&mut tx, user_a, &form_changed)
        .await
        .expect("upsert same-day");
    let same_day_row = match outcome {
        utp::UpsertOutcome::UpdatedSameDay(r) => r,
        other => panic!("expected UpdatedSameDay, got {other:?}"),
    };
    assert_eq!(
        same_day_row.id, inserted_row.id,
        "UpdatedSameDay must update the existing row in place (same id)"
    );
    assert_eq!(
        same_day_row.rendimiento_del_trabajo_percent.as_deref(),
        Some("0.4700")
    );
    assert_eq!(same_day_row.from_date, day1);
    tx.commit().await.expect("commit same-day");

    // ---------- 4. ClosedAndCreated (different day, different value).
    let day2 = chrono::NaiveDate::from_ymd_opt(2026, 2, 5).unwrap();
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) close-and-create");
    let form_day2 = utp::UserTaxPreferenceUpsertForm {
        country_iso2: "PT".into(),
        rendimiento_del_trabajo_percent: None,
        sell_to_cover_enabled: false,
        today: day2,
    };
    let outcome = utp::create_or_upsert_same_day(&mut tx, user_a, &form_day2)
        .await
        .expect("upsert close-and-create");
    let day2_row = match outcome {
        utp::UpsertOutcome::ClosedAndCreated(r) => r,
        other => panic!("expected ClosedAndCreated, got {other:?}"),
    };
    assert_ne!(
        day2_row.id, inserted_row.id,
        "ClosedAndCreated must INSERT a new row with a fresh id"
    );
    assert_eq!(day2_row.country_iso2, "PT");
    assert!(day2_row.rendimiento_del_trabajo_percent.is_none());
    assert!(!day2_row.sell_to_cover_enabled);
    assert_eq!(day2_row.from_date, day2);
    assert!(day2_row.to_date.is_none());
    tx.commit().await.expect("commit close-and-create");

    // ---------- Invariant: partial UNIQUE on (user_id) WHERE to_date IS NULL
    // holds. Only one open row may exist at a time.
    let open_count_row = sqlx::query(
        "SELECT COUNT(*) AS n FROM user_tax_preferences \
         WHERE user_id = $1 AND to_date IS NULL",
    )
    .bind(user_a)
    .fetch_one(&migrate_pool)
    .await
    .expect("count open rows");
    let open_count: i64 = open_count_row.try_get("n").expect("n");
    assert_eq!(
        open_count, 1,
        "partial UNIQUE invariant violated: user {user_a} has {open_count} open rows"
    );

    // The prior row now has `to_date = day2` (closed on close-and-create).
    let prior_row = sqlx::query("SELECT to_date FROM user_tax_preferences WHERE id = $1")
        .bind(inserted_row.id)
        .fetch_one(&migrate_pool)
        .await
        .expect("read prior row");
    let prior_to_date: Option<chrono::NaiveDate> = prior_row.try_get("to_date").expect("to_date");
    assert_eq!(
        prior_to_date,
        Some(day2),
        "prior open row should have been closed with to_date = day2"
    );

    cleanup_user(&migrate_pool, user_a).await;
}

/// Probe — `resolve_at_date` returns the correct row for points
/// inside open periods, closed periods, and before the earliest
/// `from_date`.
#[tokio::test]
async fn user_tax_preferences_resolve_at_date_covers_every_branch() {
    use orbit_db::user_tax_preferences as utp;

    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_utp_resolve").await;

    let day1 = chrono::NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
    let day2 = chrono::NaiveDate::from_ymd_opt(2026, 2, 5).unwrap();

    // Seed a closed row [day1, day2) and an open row [day2, ∞).
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) seed 1");
    let form1 = utp::UserTaxPreferenceUpsertForm {
        country_iso2: "ES".into(),
        rendimiento_del_trabajo_percent: Some("0.4500".into()),
        sell_to_cover_enabled: true,
        today: day1,
    };
    let _ = utp::create_or_upsert_same_day(&mut tx, user_a, &form1)
        .await
        .expect("seed day1");
    tx.commit().await.expect("commit seed 1");

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) seed 2");
    let form2 = utp::UserTaxPreferenceUpsertForm {
        country_iso2: "PT".into(),
        rendimiento_del_trabajo_percent: None,
        sell_to_cover_enabled: false,
        today: day2,
    };
    let _ = utp::create_or_upsert_same_day(&mut tx, user_a, &form2)
        .await
        .expect("seed day2");
    tx.commit().await.expect("commit seed 2");

    // Case A — date inside the open period: resolves to the day-2 row.
    let inside_open = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) resolve A");
    let row = utp::resolve_at_date(&mut tx, user_a, inside_open)
        .await
        .expect("resolve inside open period")
        .expect("open-period row must exist");
    assert_eq!(row.country_iso2, "PT", "inside open period must be PT");
    assert_eq!(row.from_date, day2);
    assert!(row.to_date.is_none());
    tx.commit().await.expect("commit A");

    // Case B — date inside the closed period: resolves to the day-1 row.
    let inside_closed = chrono::NaiveDate::from_ymd_opt(2026, 1, 20).unwrap();
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) resolve B");
    let row = utp::resolve_at_date(&mut tx, user_a, inside_closed)
        .await
        .expect("resolve inside closed period")
        .expect("closed-period row must exist");
    assert_eq!(row.country_iso2, "ES", "inside closed period must be ES");
    assert_eq!(row.from_date, day1);
    assert_eq!(row.to_date, Some(day2));
    tx.commit().await.expect("commit B");

    // Case C — date before the earliest from_date: resolves to None.
    let before_anything = chrono::NaiveDate::from_ymd_opt(2025, 12, 1).unwrap();
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) resolve C");
    let row = utp::resolve_at_date(&mut tx, user_a, before_anything)
        .await
        .expect("resolve before-anything");
    assert!(
        row.is_none(),
        "resolve_at_date before earliest from_date must return None; got {row:?}"
    );
    tx.commit().await.expect("commit C");

    cleanup_user(&migrate_pool, user_a).await;
}

// ---------------------------------------------------------------------------
// Slice 3b T37 — apply/clear_sell_to_cover_override + extended
// clear_override round-trip unit tests.
// ---------------------------------------------------------------------------

/// Probe — `apply_sell_to_cover_override` round-trip. Apply the
/// triplet + verify the flags; then `clear_sell_to_cover_override`
/// and verify the triplet is nulled and the flags are false, BUT the
/// Slice-3 FMV-track columns are preserved verbatim (AC-7.5.2).
#[tokio::test]
async fn vesting_events_sell_to_cover_round_trip_preserves_fmv() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_s2c_rt").await;
    let grant_id = seed_grant(&migrate_pool, user_a, "a_s2c_rt").await;
    let event_id = seed_vesting_event(&migrate_pool, user_a, grant_id, "a_s2c_rt").await;

    // Pre-stage: apply a Slice-3 FMV override so the row has FMV +
    // is_user_override=true going into the sell-to-cover path. The
    // narrow clear later must preserve all of these.
    let row0 = sqlx::query("SELECT updated_at FROM vesting_events WHERE id = $1")
        .bind(event_id)
        .fetch_one(&migrate_pool)
        .await
        .expect("read updated_at (initial)");
    let ua0: chrono::DateTime<chrono::Utc> = row0.try_get("updated_at").expect("updated_at");

    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) fmv");
    let fmv_patch = orbit_db::vesting_events::VestingEventOverridePatch {
        fmv_at_vest: Some(Some("42.0000".to_string())),
        fmv_currency: Some(Some("USD".to_string())),
        ..Default::default()
    };
    let fmv_outcome =
        orbit_db::vesting_events::apply_override(&mut tx, user_a, event_id, &fmv_patch, ua0)
            .await
            .expect("apply_override fmv");
    tx.commit().await.expect("commit fmv");
    let after_fmv = match fmv_outcome {
        orbit_db::vesting_events::OverrideOutcome::Applied(r) => r,
        other => panic!("expected Applied, got {other:?}"),
    };

    // --- apply_sell_to_cover_override on the FMV-bearing row.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) s2c apply");
    let s2c_patch = orbit_db::vesting_events::SellToCoverOverridePatch {
        tax_withholding_percent: Some(Some("0.4500".to_string())),
        share_sell_price: Some(Some("42.2500".to_string())),
        share_sell_currency: Some(Some("USD".to_string())),
    };
    let s2c_outcome = orbit_db::vesting_events::apply_sell_to_cover_override(
        &mut tx,
        user_a,
        event_id,
        &s2c_patch,
        after_fmv.updated_at,
    )
    .await
    .expect("apply_sell_to_cover_override");
    tx.commit().await.expect("commit s2c apply");
    let after_s2c = match s2c_outcome {
        orbit_db::vesting_events::SellToCoverOutcome::Applied(r) => r,
        other => panic!("expected Applied, got {other:?}"),
    };

    assert!(
        after_s2c.is_sell_to_cover_override,
        "is_sell_to_cover_override must be true after apply"
    );
    assert!(
        after_s2c.sell_to_cover_overridden_at.is_some(),
        "sell_to_cover_overridden_at must be non-null after apply"
    );
    assert!(
        after_s2c
            .tax_withholding_percent
            .as_deref()
            .map(|s| s.starts_with("0.45"))
            .unwrap_or(false),
        "tax_withholding_percent must be 0.4500 (got {:?})",
        after_s2c.tax_withholding_percent
    );
    assert!(
        after_s2c
            .share_sell_price
            .as_deref()
            .map(|s| s.starts_with("42.25"))
            .unwrap_or(false),
        "share_sell_price must be 42.2500 (got {:?})",
        after_s2c.share_sell_price
    );
    assert_eq!(after_s2c.share_sell_currency.as_deref(), Some("USD"));
    assert!(
        after_s2c.updated_at > after_fmv.updated_at,
        "updated_at must advance through the apply path"
    );
    // Slice-3 track preserved.
    assert!(after_s2c.is_user_override);
    assert_eq!(after_s2c.fmv_currency.as_deref(), Some("USD"));

    // --- clear_sell_to_cover_override (narrow revert).
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) s2c clear");
    let clear_outcome = orbit_db::vesting_events::clear_sell_to_cover_override(
        &mut tx,
        user_a,
        event_id,
        after_s2c.updated_at,
    )
    .await
    .expect("clear_sell_to_cover_override");
    tx.commit().await.expect("commit s2c clear");
    let cleared = match clear_outcome {
        orbit_db::vesting_events::SellToCoverClearOutcome::Cleared(r) => r,
        other => panic!("expected Cleared, got {other:?}"),
    };

    // Triplet cleared + flag false + timestamp null.
    assert!(cleared.tax_withholding_percent.is_none());
    assert!(cleared.share_sell_price.is_none());
    assert!(cleared.share_sell_currency.is_none());
    assert!(!cleared.is_sell_to_cover_override);
    assert!(cleared.sell_to_cover_overridden_at.is_none());
    // Slice-3 FMV-track columns preserved (AC-7.5.2).
    assert!(
        cleared
            .fmv_at_vest
            .as_deref()
            .map(|s| s.starts_with("42."))
            .unwrap_or(false),
        "fmv_at_vest must be preserved by narrow clear (got {:?})",
        cleared.fmv_at_vest
    );
    assert_eq!(cleared.fmv_currency.as_deref(), Some("USD"));
    assert!(
        cleared.is_user_override,
        "is_user_override must be preserved by narrow clear"
    );
    assert!(
        cleared.overridden_at.is_some(),
        "overridden_at must be preserved by narrow clear"
    );

    cleanup_user(&migrate_pool, user_a).await;
}

/// Probe — extended `clear_override` behaviour per ADR-018 §2:
/// apply both a Slice-3 FMV override and a Slice-3b sell-to-cover
/// override, then `clear_override` once. Verify BOTH tracks reset:
/// FMV columns NULL, triplet NULL, both flag pairs false/NULL, AND
/// vest_date + shares reverted to the algorithmic output.
#[tokio::test]
async fn vesting_events_clear_override_resets_both_tracks() {
    let app_pool = pool_from_env("DATABASE_URL").await;
    let migrate_pool = pool_from_env("DATABASE_URL_MIGRATE").await;

    let user_a = Uuid::new_v4();
    ensure_user(&migrate_pool, user_a, "a_clear_both").await;
    // seed_grant: RSU, 1000 shares / 48 mo / 12 mo cliff / monthly.
    // Algorithmic output for the 2025-09-15 cliff row: 250 shares.
    let grant_id = seed_grant(&migrate_pool, user_a, "a_clear_both").await;
    let event_id = seed_vesting_event(&migrate_pool, user_a, grant_id, "a_clear_both").await;

    // Read initial updated_at.
    let row0 = sqlx::query("SELECT updated_at FROM vesting_events WHERE id = $1")
        .bind(event_id)
        .fetch_one(&migrate_pool)
        .await
        .expect("read initial updated_at");
    let ua0: chrono::DateTime<chrono::Utc> = row0.try_get("updated_at").expect("updated_at");

    // --- Apply FMV override (vest_date + shares + fmv) via apply_override.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) fmv");
    let fmv_patch = orbit_db::vesting_events::VestingEventOverridePatch {
        vest_date: Some(chrono::NaiveDate::from_ymd_opt(2025, 10, 1).unwrap()),
        shares_vested_this_event: Some(300 * orbit_core::SHARES_SCALE),
        fmv_at_vest: Some(Some("42.0000".to_string())),
        fmv_currency: Some(Some("USD".to_string())),
    };
    let fmv_outcome =
        orbit_db::vesting_events::apply_override(&mut tx, user_a, event_id, &fmv_patch, ua0)
            .await
            .expect("apply_override fmv");
    tx.commit().await.expect("commit fmv");
    let after_fmv = match fmv_outcome {
        orbit_db::vesting_events::OverrideOutcome::Applied(r) => r,
        other => panic!("expected Applied, got {other:?}"),
    };

    // --- Apply sell-to-cover override on top.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) s2c");
    let s2c_patch = orbit_db::vesting_events::SellToCoverOverridePatch {
        tax_withholding_percent: Some(Some("0.4500".to_string())),
        share_sell_price: Some(Some("42.2500".to_string())),
        share_sell_currency: Some(Some("USD".to_string())),
    };
    let s2c_outcome = orbit_db::vesting_events::apply_sell_to_cover_override(
        &mut tx,
        user_a,
        event_id,
        &s2c_patch,
        after_fmv.updated_at,
    )
    .await
    .expect("apply_sell_to_cover_override");
    tx.commit().await.expect("commit s2c");
    let after_s2c = match s2c_outcome {
        orbit_db::vesting_events::SellToCoverOutcome::Applied(r) => r,
        other => panic!("expected Applied, got {other:?}"),
    };
    // Pre-condition: both flags true; both tracks populated.
    assert!(after_s2c.is_user_override);
    assert!(after_s2c.is_sell_to_cover_override);
    assert!(after_s2c.fmv_at_vest.is_some());
    assert!(after_s2c.tax_withholding_percent.is_some());

    // --- clear_override: the extended Slice-3b behaviour must reset
    // both tracks AND revert vest_date + shares to the algorithm.
    let mut tx = Tx::for_user(&app_pool, user_a)
        .await
        .expect("Tx::for_user(user_a) clear");
    // `today` far in the future so every derived row is non-Upcoming.
    let today = chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap();
    let clear_outcome = orbit_db::vesting_events::clear_override(
        &mut tx,
        user_a,
        event_id,
        today,
        after_s2c.updated_at,
    )
    .await
    .expect("clear_override");
    tx.commit().await.expect("commit clear");
    let cleared = match clear_outcome {
        orbit_db::vesting_events::ClearOutcome::Cleared(r) => r,
        other => panic!("expected Cleared, got {other:?}"),
    };

    // vest_date + shares reverted to the algorithm output (2025-09-15
    // cliff, 250 shares — same as the Slice-3 round-trip probe).
    assert_eq!(
        cleared.vest_date,
        chrono::NaiveDate::from_ymd_opt(2025, 9, 15).unwrap(),
        "clear_override must revert vest_date to the algorithm output"
    );
    assert_eq!(
        cleared.shares_vested_this_event,
        250 * orbit_core::SHARES_SCALE,
        "clear_override must revert shares to the algorithm output"
    );
    // Slice-3 FMV track reset (ADR-018 §2 supersede).
    assert!(
        cleared.fmv_at_vest.is_none(),
        "fmv_at_vest must be NULL after Slice-3b full clear (got {:?})",
        cleared.fmv_at_vest
    );
    assert!(
        cleared.fmv_currency.is_none(),
        "fmv_currency must be NULL after Slice-3b full clear"
    );
    assert!(
        !cleared.is_user_override,
        "is_user_override must be false after Slice-3b full clear"
    );
    assert!(
        cleared.overridden_at.is_none(),
        "overridden_at must be NULL after Slice-3b full clear"
    );
    // Slice-3b sell-to-cover track reset.
    assert!(cleared.tax_withholding_percent.is_none());
    assert!(cleared.share_sell_price.is_none());
    assert!(cleared.share_sell_currency.is_none());
    assert!(
        !cleared.is_sell_to_cover_override,
        "is_sell_to_cover_override must be false after clear_override"
    );
    assert!(
        cleared.sell_to_cover_overridden_at.is_none(),
        "sell_to_cover_overridden_at must be NULL after clear_override"
    );

    cleanup_user(&migrate_pool, user_a).await;
}
