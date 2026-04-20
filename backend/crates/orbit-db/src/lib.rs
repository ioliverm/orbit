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

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};

pub mod grants;
pub mod residency;
mod tx;
pub mod vesting_events;

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

/// Open a connection pool to Postgres using the sslmode declared in the URL.
///
/// The caller is responsible for choosing the right TLS posture:
///   - `sslmode=verify-full` — production (0b): Let's Encrypt over real DNS,
///     client verifies the server's certificate chain against the system
///     trust store. This is what `.env` must use in 0b.
///   - `sslmode=require` — local dev (0a): TLS is on the wire but the client
///     does not verify the chain. This is the documented 0a posture in
///     `.env.example` because rustls rejects the two-tier dev PKI under
///     verify-full even though the chain is structurally valid.
///   - `sslmode=disable` / `prefer` — NEVER. The backend must reject these at
///     boot; a separate runtime config loader enforces that.
///
/// The connection fails fast if the server rejects the requested mode.
///
/// The caller is responsible for keeping the pool alive for the process
/// lifetime; handlers acquire scoped transactions via `Tx::for_user` (T7).
pub async fn connect(database_url: &str) -> Result<PgPool, Error> {
    let options = PgConnectOptions::from_str(database_url).map_err(Error::InvalidUrl)?;

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
