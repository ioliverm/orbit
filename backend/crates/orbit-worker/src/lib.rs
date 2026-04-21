//! `orbit-worker` — background jobs.
//!
//! Slice 3 T29 lands the crate's first real job: the ECB FX fetcher. ADR-017
//! §4 authors the shape; this crate wires the work into two entry points:
//!
//!   * [`run_scheduled`] — long-running scheduler. Computes the next
//!     17:00-Europe/Madrid tick, sleeps until it, then runs the daily
//!     fetch. On startup, runs the 90-day bootstrap iff the DB is cold
//!     (fewer than 30 USD rows in the last 90 days per ADR-017 §4).
//!   * [`run_once`] — ad-hoc. Runs exactly one fetch (daily or bootstrap)
//!     and returns. Used by the `orbit worker --once fx|bootstrap`
//!     CLI and by CI for deterministic smoke tests.
//!
//! All outbound HTTP goes through the caller-supplied `reqwest::Client`
//! (timeouts pinned at the client level — 5 s per ADR-007). SQL writes
//! go through `orbit_db::Tx` so the fetch + `fx.fetch_success` audit row
//! land atomically (SEC-101 inline-with-mutation posture; ADR-017 §4).
//!
//! # Log allowlist (SEC-050)
//!
//! Every log line goes through `orbit_log::event!`. The new Slice-3
//! event names:
//!
//!   * `fx.fetch_start`
//!   * `fx.fetch_success`
//!   * `fx.fetch_failure`
//!   * `fx.fetch_persistent_failure`
//!   * `fx.bootstrap_start`
//!   * `fx.bootstrap_success`
//!   * `fx.scheduler_tick`

pub mod fx;
pub mod scheduler;

use chrono::Utc;
use orbit_db::Tx;
use sqlx::PgPool;

pub use fx::{FetchError, FetchKind, FetchOutcome};

/// Outbound egress and run-once ad-hoc path for both daily + bootstrap.
///
/// Returns the number of rows inserted into `fx_rates` and a summary
/// of the fetch for the CLI to print on `--once`.
pub async fn run_once(
    pool: &PgPool,
    http: &reqwest::Client,
    kind: FetchKind,
) -> Result<FetchOutcome, FetchError> {
    match kind {
        FetchKind::Daily => fx::run_daily_with_retry(pool, http).await,
        FetchKind::Bootstrap => fx::run_bootstrap(pool, http).await,
    }
}

/// Long-running scheduler. Boots with a bootstrap-if-cold check, then
/// enters the daily loop.
///
/// The caller is responsible for wiring shutdown (a `tokio::select!`
/// against its own signal channel); when `shutdown` fires, the loop
/// exits gracefully on the next tick.
pub async fn run_scheduled(
    pool: PgPool,
    http: reqwest::Client,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), FetchError> {
    // 1. Bootstrap if cold. ADR-017 §4 — threshold is `< 30` USD rows
    //    in the last 90 days.
    let bootstrap_count = orbit_db::fx_rates::bootstrap_count(&pool)
        .await
        .map_err(FetchError::Db)?;
    if bootstrap_count < 30 {
        orbit_log::event!(
            orbit_log::Level::Info,
            "fx.bootstrap_start",
            existing_rows = bootstrap_count
        );
        // A bootstrap failure is logged but does NOT stop the scheduler
        // — the daily fetch is still useful.
        if let Err(e) = fx::run_bootstrap(&pool, &http).await {
            orbit_log::event!(
                orbit_log::Level::Warn,
                "fx.fetch_persistent_failure",
                kind = "bootstrap",
                run_count = 1u64
            );
            record_failure_audit(&pool, FetchKind::Bootstrap, &e).await;
        }
    }

    // 2. Daily scheduler loop.
    loop {
        // Check shutdown before sleeping.
        if *shutdown.borrow() {
            return Ok(());
        }

        let sleep = scheduler::sleep_until_next_tick(Utc::now());
        tokio::select! {
            _ = sleep => {},
            _ = wait_shutdown(shutdown.clone()) => {
                return Ok(());
            }
        }

        orbit_log::event!(orbit_log::Level::Info, "fx.scheduler_tick");

        match fx::run_daily_with_retry(&pool, &http).await {
            Ok(_) => {}
            Err(e) => {
                record_failure_audit(&pool, FetchKind::Daily, &e).await;
            }
        }
    }
}

async fn wait_shutdown(mut rx: tokio::sync::watch::Receiver<bool>) {
    let _ = rx.changed().await;
}

/// Best-effort audit-row insert for a fetch failure. Runs against a
/// system pseudo-tx (no user scoping; fx_rates is reference data).
/// Errors are swallowed because (a) the fetch already failed and
/// (b) losing the audit row is strictly less bad than losing the
/// scheduler on a DB blip.
async fn record_failure_audit(pool: &PgPool, kind: FetchKind, err: &FetchError) {
    let reason = err.classify();
    let kind_str = kind.as_str();
    let minute = Utc::now().format("%H:%M").to_string();
    let payload = serde_json::json!({
        "reason": reason,
        "kind": kind_str,
        "attempted_at_minute": minute,
    });
    let _ = sqlx::query(
        r#"
        INSERT INTO audit_log (
            user_id, actor_kind, action, target_kind, target_id,
            ip_hash, payload_summary
        )
        VALUES (NULL, 'system', $1, 'fx_rates', NULL, NULL, $2)
        "#,
    )
    .bind(match kind {
        FetchKind::Daily => "fx.fetch_failure",
        FetchKind::Bootstrap => "fx.bootstrap_failure",
    })
    .bind(payload)
    .execute(pool)
    .await;
}

/// Helper for audit writes that DO ride inside a tx (success path). See
/// `fx::run_daily_with_retry` for the caller.
pub(crate) async fn record_success_audit_in_tx(
    tx: &mut Tx<'_>,
    kind: FetchKind,
    rows_inserted: u64,
    rate_date: Option<chrono::NaiveDate>,
    span_days: Option<i64>,
) -> Result<(), sqlx::Error> {
    let action = match kind {
        FetchKind::Daily => "fx.fetch_success",
        FetchKind::Bootstrap => "fx.bootstrap_success",
    };
    let mut payload = serde_json::json!({
        "kind": kind.as_str(),
        "quote_currencies": ["USD"],
        "rows_inserted": rows_inserted,
    });
    if let Some(d) = rate_date {
        payload["publication_date"] = serde_json::json!(d.format("%Y-%m-%d").to_string());
    }
    if let Some(s) = span_days {
        payload["span_days"] = serde_json::json!(s);
    }
    if matches!(kind, FetchKind::Bootstrap) {
        payload["historical_file"] = serde_json::json!("eurofxref-hist-90d");
    }

    sqlx::query(
        r#"
        INSERT INTO audit_log (
            user_id, actor_kind, action, target_kind, target_id,
            ip_hash, payload_summary
        )
        VALUES (NULL, 'system', $1, 'fx_rates', NULL, NULL, $2)
        "#,
    )
    .bind(action)
    .bind(payload)
    .execute(tx.as_executor())
    .await?;
    Ok(())
}
