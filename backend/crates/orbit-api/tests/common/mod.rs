//! Shared integration-test harness for Slice-2 T21 flow tests.
//!
//! Duplicating the harness across files (as Slice-1 did with
//! `wizard_flow.rs`, `auth_flow.rs`, `grants_flow.rs`) is workable but
//! grew noisy once Slice-2 added five new surfaces. This module lives
//! under `tests/common/` so `cargo test` does NOT compile it as a
//! standalone integration binary (it would fail — no test functions);
//! each flow file imports it via `mod common;`.

#![cfg(feature = "integration-tests")]
#![allow(dead_code)]

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt as _;
use orbit_api::{router, AppState};
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use std::str::FromStr;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Pool + state
// ---------------------------------------------------------------------------

pub async fn pool_from_env(var: &str) -> PgPool {
    let url = std::env::var(var)
        .unwrap_or_else(|_| panic!("{var} must be set for orbit-api integration tests"));
    let opts =
        PgConnectOptions::from_str(&url).unwrap_or_else(|e| panic!("invalid url in {var}: {e}"));
    PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap_or_else(|e| panic!("connect via {var} failed: {e}"))
}

pub fn make_state(pool: PgPool) -> AppState {
    AppState {
        pool,
        ip_hash_key: Arc::new([7u8; 32]),
        cookie_secure: false,
        cors_origin: "http://localhost:5173".into(),
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap(),
    }
}

pub async fn app() -> (AppState, axum::Router) {
    let pool = pool_from_env("DATABASE_URL").await;
    let state = make_state(pool);
    (state.clone(), router(state))
}

// ---------------------------------------------------------------------------
// Identifier helpers (unique per test invocation)
// ---------------------------------------------------------------------------

pub fn unique_email(tag: &str) -> String {
    format!("orbit-t21-{tag}-{}@example.test", Uuid::new_v4())
}

pub fn unique_password() -> String {
    format!("Orbit-T21-{}-Z8", Uuid::new_v4())
}

pub fn unique_ip() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() & 0xff;
    format!("10.{pid}.{}.{}", (n >> 8) & 0xff, n & 0xff)
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

pub async fn body_json(
    resp: axum::http::Response<Body>,
) -> (StatusCode, Vec<axum::http::HeaderValue>, Value) {
    let status = resp.status();
    let cookies: Vec<_> = resp
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect();
    let body_bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json = if body_bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body_bytes).unwrap_or(Value::Null)
    };
    (status, cookies, json)
}

pub fn cookie_value(headers: &[axum::http::HeaderValue], name: &str) -> Option<String> {
    for h in headers {
        let s = h.to_str().ok()?;
        if let Some(rest) = s.strip_prefix(&format!("{name}=")) {
            let v = rest.split(';').next().unwrap_or("");
            return Some(v.to_string());
        }
    }
    None
}

pub async fn post(
    app: &axum::Router,
    path: &str,
    body: Value,
    extra_headers: Vec<(&str, String)>,
) -> axum::http::Response<Body> {
    let has_xff = extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("x-forwarded-for"));
    let mut req = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    for (k, v) in extra_headers {
        req = req.header(k, v);
    }
    if !has_xff {
        req = req.header("x-forwarded-for", unique_ip());
    }
    app.clone()
        .oneshot(
            req.body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .expect("request ran")
}

pub async fn put(
    app: &axum::Router,
    path: &str,
    body: Value,
    extra_headers: Vec<(&str, String)>,
) -> axum::http::Response<Body> {
    let has_xff = extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("x-forwarded-for"));
    let mut req = Request::builder()
        .method("PUT")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    for (k, v) in extra_headers {
        req = req.header(k, v);
    }
    if !has_xff {
        req = req.header("x-forwarded-for", unique_ip());
    }
    app.clone()
        .oneshot(
            req.body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .expect("request ran")
}

pub async fn delete(
    app: &axum::Router,
    path: &str,
    extra_headers: Vec<(&str, String)>,
) -> axum::http::Response<Body> {
    let has_xff = extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("x-forwarded-for"));
    let mut req = Request::builder().method("DELETE").uri(path);
    for (k, v) in extra_headers {
        req = req.header(k, v);
    }
    if !has_xff {
        req = req.header("x-forwarded-for", unique_ip());
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .expect("request ran")
}

pub async fn get(
    app: &axum::Router,
    path: &str,
    extra_headers: Vec<(&str, String)>,
) -> axum::http::Response<Body> {
    let has_xff = extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("x-forwarded-for"));
    let mut req = Request::builder().method("GET").uri(path);
    for (k, v) in extra_headers {
        req = req.header(k, v);
    }
    if !has_xff {
        req = req.header("x-forwarded-for", unique_ip());
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .expect("request ran")
}

// ---------------------------------------------------------------------------
// User seeding (signup → verify → disclaimer → residency → first grant)
// ---------------------------------------------------------------------------

async fn take_raw_verification_token(pool: &PgPool, email: &str) -> String {
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .expect("user exists");
    let raw = Uuid::new_v4().to_string().replace('-', "");
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, raw.as_bytes());
    let hash: [u8; 32] = sha2::Digest::finalize(hasher).into();
    let mut tx = orbit_db::Tx::for_user(pool, user_id)
        .await
        .expect("tx for_user");
    sqlx::query(
        r#"
        INSERT INTO email_verifications (user_id, token_hash, expires_at)
        VALUES ($1, $2, now() + INTERVAL '24 hours')
        "#,
    )
    .bind(user_id)
    .bind(&hash[..])
    .execute(tx.as_executor())
    .await
    .expect("insert verification");
    tx.commit().await.expect("commit");
    raw
}

async fn find_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .expect("user row")
}

pub struct Session {
    pub user_id: Uuid,
    pub cookie: String,
    pub csrf: String,
}

/// Signs up a new user, verifies their email, and returns the session
/// cookie + CSRF token. No onboarding steps completed yet — the caller
/// drives disclaimer / residency / first-grant as needed.
pub async fn signup_verified(state: &AppState, app: &axum::Router, tag: &str) -> Session {
    let email = unique_email(tag);
    let password = unique_password();
    let r = post(
        app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let token = take_raw_verification_token(&state.pool, &email).await;
    let r = post(
        app,
        "/api/v1/auth/verify-email",
        json!({ "token": token }),
        vec![],
    )
    .await;
    let (_st, cookies, _) = body_json(r).await;
    let sess = cookie_value(&cookies, "orbit_sess").unwrap();
    let csrf = cookie_value(&cookies, "orbit_csrf").unwrap();
    let cookie = format!("orbit_sess={sess}; orbit_csrf={csrf}");
    let user_id = find_user(&state.pool, &email).await;
    Session {
        user_id,
        cookie,
        csrf,
    }
}

/// Full onboarding: signup + verify + disclaimer + residency. Returns a
/// Session whose caller can POST `/api/v1/grants` to finish onboarding
/// (or call [`onboarded_with_grant`] to get a complete user in one go).
pub async fn onboarded(state: &AppState, app: &axum::Router, tag: &str) -> Session {
    let s = signup_verified(state, app, tag).await;
    post(
        app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    post(
        app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-MD",
            "primaryCurrency": "EUR",
            "regimeFlags": []
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    s
}

pub fn basic_rsu_body() -> Value {
    json!({
        "instrument": "rsu",
        "grantDate": "2024-09-15",
        "shareCount": 30000,
        "vestingStart": "2024-09-15",
        "vestingTotalMonths": 48,
        "cliffMonths": 12,
        "vestingCadence": "monthly",
        "doubleTrigger": true,
        "employerName": "ACME Inc."
    })
}

pub fn espp_grant_body(employer: &str) -> Value {
    json!({
        "instrument": "espp",
        "grantDate": "2024-09-15",
        "shareCount": 500,
        "vestingStart": "2024-09-15",
        "vestingTotalMonths": 12,
        "cliffMonths": 0,
        "vestingCadence": "monthly",
        "doubleTrigger": false,
        "employerName": employer,
    })
}

/// Onboarded with an RSU grant committed, bumping the user to
/// `complete`. Use for tests that operate past the onboarding gate.
pub async fn onboarded_with_grant(state: &AppState, app: &axum::Router, tag: &str) -> Session {
    let s = onboarded(state, app, tag).await;
    let r = post(
        app,
        "/api/v1/grants",
        basic_rsu_body(),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    s
}

pub async fn create_espp_grant(app: &axum::Router, s: &Session, employer: &str) -> Uuid {
    let r = post(
        app,
        "/api/v1/grants",
        espp_grant_body(employer),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "espp grant create: {body}");
    Uuid::parse_str(body["grant"]["id"].as_str().unwrap()).unwrap()
}

// ---------------------------------------------------------------------------
// Audit-log probes
// ---------------------------------------------------------------------------

pub async fn audit_count(pool: &PgPool, user_id: Uuid, action: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*)::bigint FROM audit_log WHERE user_id = $1 AND action = $2")
        .bind(user_id)
        .bind(action)
        .fetch_one(pool)
        .await
        .unwrap_or(0)
}

pub async fn audit_last_payload(pool: &PgPool, user_id: Uuid, action: &str) -> Value {
    sqlx::query_scalar(
        "SELECT payload_summary FROM audit_log WHERE user_id = $1 AND action = $2 \
         ORDER BY occurred_at DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(action)
    .fetch_one(pool)
    .await
    .unwrap_or(Value::Null)
}

/// Audit-log reads are not RLS-scoped — the test pool (role: `orbit_app`)
/// cannot SELECT from `audit_log` in production, but during integration
/// tests we either use the migrate role or accept empty reads. When
/// `DATABASE_URL_MIGRATE` is set (our CI convention), route probes
/// through it; else fall back to the writable pool.
pub async fn audit_pool() -> PgPool {
    if std::env::var("DATABASE_URL_MIGRATE").is_ok() {
        pool_from_env("DATABASE_URL_MIGRATE").await
    } else {
        pool_from_env("DATABASE_URL").await
    }
}
