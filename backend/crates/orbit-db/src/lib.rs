//! Postgres pool + migration entrypoint for Orbit. Slice 0a scaffold.
//!
//! Traces to:
//!   - ADR-013 (migration tool: `sqlx-cli`, path: `migrations/`).
//!   - ADR-014 §1 (authoritative DDL).
//!   - ADR-015 (Slice 0a scope: `orbit_support` role lives in the init migration).
//!   - Security checklist S0-16 / S0-23 (`Tx::for_user` is the only
//!     query-handle API; owned by T7).
//!
//! This crate intentionally ships a minimal surface in Slice 0a:
//!
//!   * [`connect`] — opens a TLS-verified [`PgPool`].
//!   * [`migrate`] — runs the embedded migrations against that pool.
//!   * [`Tx::for_user`] — declared as a stub; the real implementation lands
//!     with the auth layer (T7) per SEC-022.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use uuid::Uuid;

/// Errors produced by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Failed to parse the provided `DATABASE_URL`.
    #[error("invalid database url: {0}")]
    InvalidUrl(#[source] sqlx::Error),
    /// Failed to establish a TLS-verified connection to Postgres.
    #[error("connect failed: {0}")]
    Connect(#[source] sqlx::Error),
    /// Migration runner failed.
    #[error("migrate failed: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

/// Open a TLS-verified connection pool to Postgres.
///
/// Enforces `sslmode=verify-full` unless the caller's URL already requests
/// `verify-full` or stricter. The connection fails fast if the server rejects
/// TLS (S0-16 companion on the client side; full enforcement still lives in
/// `pg_hba.conf` on the server).
///
/// The caller is responsible for keeping the pool alive for the process
/// lifetime; handlers acquire scoped transactions via `Tx::for_user` (T7).
pub async fn connect(database_url: &str) -> Result<PgPool, Error> {
    let options = PgConnectOptions::from_str(database_url)
        .map_err(Error::InvalidUrl)?
        // sqlx's default for `postgres://` URLs is `Prefer`, which accepts a
        // cleartext fallback; that is not acceptable for Orbit. Force
        // verify-full so the client validates the server's certificate chain
        // against the configured root store. The connection will fail fast
        // if the server rejects TLS.
        .ssl_mode(PgSslMode::VerifyFull);

    PgPoolOptions::new()
        .max_connections(16)
        .connect_with(options)
        .await
        .map_err(Error::Connect)
}

/// Run the embedded migrations against `pool`.
///
/// The binary's `orbit migrate` subcommand is the sole caller of this helper.
pub async fn migrate(pool: &PgPool) -> Result<(), Error> {
    sqlx::migrate!("../../../migrations")
        .run(pool)
        .await
        .map_err(Error::Migrate)
}

/// A per-user transaction handle.
///
/// Per SEC-022 / S0-23, this is the **only** query-handle API exposed to
/// handler code. Acquiring it runs `SET LOCAL app.user_id = $1` inside a
/// transaction so that the RLS `tenant_isolation` policies on every
/// user-scoped table resolve to the caller's rows.
///
/// The real implementation is owned by T7 (auth layer); this scaffold is a
/// compile-time placeholder so downstream crates can reference the type.
pub struct Tx<'a> {
    // PhantomData keeps the lifetime attached; the real impl will hold a
    // `sqlx::Transaction<'a, sqlx::Postgres>`.
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Tx<'a> {
    /// Open a transaction scoped to `user_id`.
    ///
    /// **Not implemented in Slice 0a.** The full implementation lands with
    /// the auth layer (T7) per SEC-022. Do not call this from Slice 0a code.
    #[allow(clippy::missing_errors_doc)]
    pub async fn for_user(_pool: &PgPool, _user_id: Uuid) -> Result<Tx<'a>, Error> {
        todo!("Tx::for_user is owned by T7 (auth layer) — SEC-022")
    }
}
