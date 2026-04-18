//! Postgres pool + migration entrypoint for Orbit. Slice 0a scaffold.
//!
//! Traces to:
//!   - ADR-013 (migration tool: `sqlx-cli`, path: `migrations/`).
//!   - ADR-014 §1 (authoritative DDL).
//!   - ADR-015 (Slice 0a scope: `orbit_support` role lives in the init migration).
//!   - Security checklist S0-16 / S0-23 (`Tx::for_user` is the only
//!     query-handle API).
//!
//! This crate intentionally ships a minimal surface in Slice 0a:
//!
//!   * [`connect`] — opens a TLS-verified [`PgPool`].
//!   * [`migrate`] — runs the embedded migrations against that pool.
//!   * [`Tx::for_user`] — per-user transaction that primes the
//!     `app.user_id` GUC used by RLS policies. Defined in [`tx`].

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};

mod tx;

pub use tx::Tx;

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
    /// Failed to begin a transaction or prime `app.user_id` inside it
    /// (SEC-022 / S0-23). If this fires, handler code must NOT proceed —
    /// the RLS policies would treat the caller as unauthenticated.
    #[error("tx setup failed: {0}")]
    Tx(#[source] sqlx::Error),
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
