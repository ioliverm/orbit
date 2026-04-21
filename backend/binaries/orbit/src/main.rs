//! `orbit` — single binary with `api`, `migrate`, `worker` subcommands per
//! ADR-013 §"Single binary with subcommands".
//!
//! Slice 1 T13a scope:
//!   - `orbit api`: boots axum on `APP_BIND_ADDR`.
//!   - `orbit migrate`: runs sqlx migrations.
//!   - `orbit worker`: stub. Real wiring ships with Slice 3+.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "orbit", version, about = "Orbit single binary")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Boot the axum API server.
    Api {
        /// Address to bind. Overrides `APP_BIND_ADDR`.
        #[arg(long, env = "APP_BIND_ADDR", default_value = "127.0.0.1:8080")]
        bind: String,
        /// Postgres connection URL (orbit_app role).
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        /// 64-hex-char HMAC key for `audit_log.ip_hash` (SEC-054).
        #[arg(long, env = "APP_IP_HASH_KEY_HEX")]
        ip_hash_key_hex: String,
        /// Emit `Secure` on session cookies. Defaults to `true` — the dev
        /// environment must explicitly set `APP_COOKIE_SECURE=false` so
        /// cookies flow over `http://localhost:*` (see `.env.example`).
        #[arg(long, env = "APP_COOKIE_SECURE", default_value_t = true)]
        cookie_secure: bool,
        /// Same-origin SPA origin for CORS.
        #[arg(long, env = "APP_CORS_ORIGIN", default_value = "http://localhost:5173")]
        cors_origin: String,
    },
    /// Run sqlx migrations against `DATABASE_URL_MIGRATE`.
    Migrate {
        /// Postgres connection URL (orbit_migrate role).
        #[arg(long, env = "DATABASE_URL_MIGRATE")]
        database_url_migrate: String,
    },
    /// Boot the background worker (Slice 3+: scheduled ECB FX fetch).
    Worker {
        /// Postgres connection URL (orbit_app role).
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
        /// When set, run a single fetch and exit (for dev + CI smoke).
        /// Accepts `fx` (daily) or `bootstrap` (90-day history).
        #[arg(long, value_parser = ["fx", "bootstrap"])]
        once: Option<String>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Api {
            bind,
            database_url,
            ip_hash_key_hex,
            cookie_secure,
            cors_origin,
        } => {
            run_api(
                bind,
                database_url,
                ip_hash_key_hex,
                cookie_secure,
                cors_origin,
            )
            .await
        }
        Command::Migrate {
            database_url_migrate,
        } => run_migrate(database_url_migrate).await,
        Command::Worker { database_url, once } => run_worker(database_url, once).await,
    }
}

async fn run_api(
    bind: String,
    database_url: String,
    ip_hash_key_hex: String,
    cookie_secure: bool,
    cors_origin: String,
) -> ExitCode {
    let addr: SocketAddr = match bind.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("orbit api: invalid --bind {bind:?}: {e}");
            return ExitCode::from(2);
        }
    };

    let ip_hash_key = match decode_hash_key(&ip_hash_key_hex) {
        Ok(k) => Arc::new(k),
        Err(e) => {
            eprintln!("orbit api: invalid APP_IP_HASH_KEY_HEX: {e}");
            return ExitCode::from(2);
        }
    };

    let pool = match orbit_db::connect(&database_url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("orbit api: database connect failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let http = reqwest_client();

    let state = orbit_api::AppState {
        pool,
        ip_hash_key,
        cookie_secure,
        cors_origin,
        http,
    };

    let router = orbit_api::router(state);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("orbit api: bind {addr} failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    orbit_log::event!(
        orbit_log::Level::Info,
        "api.listening",
        port = addr.port() as u64
    );

    if let Err(e) = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        eprintln!("orbit api: serve error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn run_migrate(url: String) -> ExitCode {
    let pool = match orbit_db::connect(&url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("orbit migrate: connect failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    match orbit_db::migrate(&pool).await {
        Ok(()) => {
            orbit_log::event!(orbit_log::Level::Info, "migrate.ok");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("orbit migrate: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run_worker(database_url: String, once: Option<String>) -> ExitCode {
    let pool = match orbit_db::connect(&database_url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("orbit worker: database connect failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // The ECB worker needs a separate reqwest client from the API's
    // HIBP client because the timeout posture differs: 5 s here per
    // ADR-007 vs 500 ms for HIBP.
    //
    // `redirect(Policy::none())` — ECB publishes at a stable
    // `https://www.ecb.europa.eu/stats/eurofxref/...` URL, so a redirect
    // is either a site reshuffle (tracked manually) or an attempt to
    // pivot the fetch somewhere unexpected. Fail closed rather than
    // follow. The response-size cap lives in `fx::fetch_xml` so that
    // bootstrap + daily share the same gate.
    let http = match reqwest::Client::builder()
        .user_agent("orbit-worker/0.0.0")
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("orbit worker: reqwest client build: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(kind_str) = once {
        let kind = match kind_str.as_str() {
            "fx" => orbit_worker::FetchKind::Daily,
            "bootstrap" => orbit_worker::FetchKind::Bootstrap,
            other => {
                eprintln!("orbit worker: unknown --once value {other:?}");
                return ExitCode::from(2);
            }
        };
        match orbit_worker::run_once(&pool, &http, kind).await {
            Ok(outcome) => {
                eprintln!(
                    "orbit worker --once {}: rows_inserted={} oldest={:?} newest={:?}",
                    kind_str, outcome.rows_inserted, outcome.oldest_date, outcome.newest_date
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("orbit worker --once {kind_str}: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        let (tx, rx) = tokio::sync::watch::channel(false);
        // Wire Ctrl-C to flip the shutdown signal.
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                let _ = tx.send(true);
            }
        });
        match orbit_worker::run_scheduled(pool, http, rx).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("orbit worker: scheduler error: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

fn decode_hash_key(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s).map_err(|e| format!("hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn reqwest_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("orbit/0.0.0")
        .timeout(std::time::Duration::from_millis(500))
        .connect_timeout(std::time::Duration::from_millis(500))
        .build()
        .expect("reqwest client build")
}
