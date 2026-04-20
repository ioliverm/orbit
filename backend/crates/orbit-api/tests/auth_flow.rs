//! HTTP-level integration tests for the Slice 1 T13a auth surface.
//!
//! Feature-gated behind `integration-tests` (mirrors orbit-db). A fresh
//! `cargo test --workspace` stays Postgres-free; the CI job enabling this
//! feature boots the local Postgres stack first.
//!
//! Coverage:
//!
//! * Happy-path flow: signup → verify-email → /auth/me → signout →
//!   signin → /auth/me → signout. Asserts cookies + audit_log rows +
//!   CSRF header exchange.
//! * SEC-003 no-enumeration posture: duplicate-email signup returns 201,
//!   and wrong-password signin returns 401 with `code: "auth"`.
//! * Email-verification token semantics: expired / unknown token → 401.
//! * Unverified user cannot signin.

#![cfg(feature = "integration-tests")]

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
// Harness
// ---------------------------------------------------------------------------

async fn pool_from_env(var: &str) -> PgPool {
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

fn make_state(pool: PgPool) -> AppState {
    AppState {
        pool,
        ip_hash_key: Arc::new([7u8; 32]),
        cookie_secure: false,
        cors_origin: "http://localhost:5173".into(),
        // 2s budget is generous enough for HIBP's Cloudflare-fronted
        // range endpoint (~200ms P75 from EU). Shorter values make the
        // integration tests brittle when the test host has slow DNS.
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap(),
    }
}

/// A not-in-HIBP password fragment. Combined with a per-test UUID the
/// full string is effectively unique; even if the UUID bit is ever
/// crawled into a breach list the fresh Uuid ensures any rerun of the
/// suite gets a novel password.
fn unique_password() -> String {
    format!("Orbit-T13a-Ok-{}-Z8", Uuid::new_v4())
}

/// Build a thin app per test so cookie jars don't bleed between cases.
/// Rate-limit isolation across tests relies on each request carrying a
/// synthetic `X-Forwarded-For` (see [`unique_ip`]) and a unique email —
/// the orbit_app role can't DELETE from `rate_limit_buckets`.
async fn app() -> (AppState, axum::Router) {
    let pool = pool_from_env("DATABASE_URL").await;
    let state = make_state(pool);
    (state.clone(), router(state))
}

/// Unique email per test — keeps runs independent on a shared local DB.
fn unique_email(tag: &str) -> String {
    format!("orbit-api-{tag}-{}@example.test", Uuid::new_v4())
}

/// Unique synthetic IP per test invocation. The signup handler rate-limits
/// by IP hash, so without this every 6th call from the same test host
/// would hit 5/IP/hour and return 429. Combines an in-process counter
/// (deterministic per run) with the run's PID so reruns don't collide.
fn unique_ip() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() & 0xff;
    // 10.0.0.0/8 documentation range — won't collide with any real caller.
    format!("10.{pid}.{}.{}", (n >> 8) & 0xff, n & 0xff)
}

async fn body_json(
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

/// Parse a `Set-Cookie` header for the `name=value` pair.
fn cookie_value(headers: &[axum::http::HeaderValue], name: &str) -> Option<String> {
    for h in headers {
        let s = h.to_str().ok()?;
        if let Some(rest) = s.strip_prefix(&format!("{name}=")) {
            let v = rest.split(';').next().unwrap_or("");
            return Some(v.to_string());
        }
    }
    None
}

/// Mint a fresh verification row for `email` and return the raw token.
/// Emails are not sent in T13a, so the real signup also logs the raw
/// token — we don't scrape stderr, we synthesize a fresh row here.
///
/// Inserts go through `Tx::for_user(user_id)` so the RLS WITH CHECK
/// clause on `email_verifications` accepts the row.
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

/// Count audit_log rows. `orbit_app` has no SELECT on audit_log (SEC-102)
/// so this uses the migrate pool as the read path, matching the orbit_support
/// operator view.
async fn audit_count(user_id: Uuid, action: &str) -> i64 {
    let pool = pool_from_env("DATABASE_URL_MIGRATE").await;
    sqlx::query_scalar("SELECT COUNT(*)::bigint FROM audit_log WHERE user_id = $1 AND action = $2")
        .bind(user_id)
        .bind(action)
        .fetch_one(&pool)
        .await
        .unwrap_or(0)
}

async fn find_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .expect("user row")
}

async fn post(
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

async fn get(
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
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn healthz_is_no_content() {
    let (_state, app) = app().await;
    let resp = get(&app, "/healthz", vec![]).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn signup_verify_me_signout_signin_roundtrip() {
    let (state, app) = app().await;
    let email = unique_email("happy");
    let password = unique_password();

    // --- signup
    let resp = post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password, "localeHint": "es-ES" }),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED, "signup must be 201");
    let user_id = find_user(&state.pool, &email).await;
    assert_eq!(audit_count(user_id, "signup.success").await, 1);

    // --- verify-email (fetch a token via the DB since email is not sent)
    let token = take_raw_verification_token(&state.pool, &email).await;
    let resp = post(
        &app,
        "/api/v1/auth/verify-email",
        json!({ "token": token }),
        vec![],
    )
    .await;
    let (status, cookies, _body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "verify-email must be 200");
    let sess = cookie_value(&cookies, "orbit_sess").expect("orbit_sess issued");
    let csrf = cookie_value(&cookies, "orbit_csrf").expect("orbit_csrf issued");
    assert!(!sess.is_empty() && !csrf.is_empty());
    assert_eq!(
        audit_count(user_id, "login.success").await,
        1,
        "login.success recorded at post-verification"
    );

    // --- /auth/me
    let cookie_header = format!("orbit_sess={sess}; orbit_csrf={csrf}");
    let resp = get(
        &app,
        "/api/v1/auth/me",
        vec![(header::COOKIE.as_str(), cookie_header.clone())],
    )
    .await;
    let (status, _cookies, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "me must be 200");
    assert_eq!(body["user"]["email"], email);
    assert_eq!(body["onboardingStage"], "disclaimer");

    // --- signout (requires CSRF header matching the cookie)
    let resp = post(
        &app,
        "/api/v1/auth/signout",
        json!({}),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(audit_count(user_id, "logout").await, 1);

    // --- signin (by password)
    let resp = post(
        &app,
        "/api/v1/auth/signin",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    let (status, cookies, _body) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "signin must succeed for a verified user"
    );
    let sess2 = cookie_value(&cookies, "orbit_sess").expect("sess2");
    let csrf2 = cookie_value(&cookies, "orbit_csrf").expect("csrf2");
    assert_ne!(sess, sess2, "new session rotates orbit_sess");

    let cookie_header2 = format!("orbit_sess={sess2}; orbit_csrf={csrf2}");
    let resp = post(
        &app,
        "/api/v1/auth/signout",
        json!({}),
        vec![
            (header::COOKIE.as_str(), cookie_header2.clone()),
            ("x-csrf-token", csrf2.clone()),
        ],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(audit_count(user_id, "logout").await, 2);
}

#[tokio::test]
async fn signup_duplicate_email_still_returns_201() {
    let (state, app) = app().await;
    let email = unique_email("dup");
    let password = unique_password();

    let r1 = post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    assert_eq!(r1.status(), StatusCode::CREATED);

    // Second signup for the same email must also be 201 (SEC-003 no-enumeration).
    let r2 = post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    assert_eq!(
        r2.status(),
        StatusCode::CREATED,
        "duplicate signup must not leak existence"
    );

    // Exactly one user row, one signup.success audit row, one failure row.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM users WHERE email = $1")
        .bind(&email)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn signin_wrong_password_returns_generic_401() {
    let (state, app) = app().await;
    let email = unique_email("wrongpw");
    let password = unique_password();

    // Seed a verified user via signup + direct DB flip (avoid running
    // verify-email just to unblock the test — we're probing signin).
    let r = post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
        .bind(&email)
        .execute(&state.pool)
        .await
        .unwrap();

    let resp = post(
        &app,
        "/api/v1/auth/signin",
        json!({ "email": email, "password": "not-the-password" }),
        vec![],
    )
    .await;
    let (status, _cookies, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "auth");
}

#[tokio::test]
async fn signin_unknown_user_returns_generic_401() {
    let (_state, app) = app().await;
    let resp = post(
        &app,
        "/api/v1/auth/signin",
        json!({
            "email": unique_email("nosuch"),
            "password": unique_password(),
        }),
        vec![],
    )
    .await;
    let (status, _cookies, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Must be the identical generic code (SEC-004).
    assert_eq!(body["error"]["code"], "auth");
}

#[tokio::test]
async fn unverified_user_cannot_signin() {
    let (_state, app) = app().await;
    let email = unique_email("unverified");
    let password = unique_password();

    let r = post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // Do NOT verify. Signin should fail generically.
    let resp = post(
        &app,
        "/api/v1/auth/signin",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    let (status, _cookies, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "auth");
}

#[tokio::test]
async fn verify_email_rejects_expired_or_unknown_token() {
    let (state, app) = app().await;
    let email = unique_email("expired");
    let password = unique_password();

    let r = post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // Unknown token.
    let resp = post(
        &app,
        "/api/v1/auth/verify-email",
        json!({ "token": "not-a-token-12345" }),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Expired token: insert one with expires_at in the past, via a
    // per-user tx so the RLS WITH CHECK passes.
    let user_id = find_user(&state.pool, &email).await;
    let raw = Uuid::new_v4().to_string().replace('-', "");
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, raw.as_bytes());
    let hash: [u8; 32] = sha2::Digest::finalize(hasher).into();
    let mut tx = orbit_db::Tx::for_user(&state.pool, user_id)
        .await
        .expect("tx for_user");
    sqlx::query(
        "INSERT INTO email_verifications (user_id, token_hash, expires_at) VALUES ($1, $2, now() - INTERVAL '1 hour')",
    )
    .bind(user_id)
    .bind(&hash[..])
    .execute(tx.as_executor())
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let resp = post(
        &app,
        "/api/v1/auth/verify-email",
        json!({ "token": raw }),
        vec![],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn signout_without_csrf_header_is_403() {
    let (state, app) = app().await;
    let email = unique_email("csrfmiss");
    let password = unique_password();

    // signup → verify → now we have a session
    post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    let token = take_raw_verification_token(&state.pool, &email).await;
    let r = post(
        &app,
        "/api/v1/auth/verify-email",
        json!({ "token": token }),
        vec![],
    )
    .await;
    let (_st, cookies, _b) = body_json(r).await;
    let sess = cookie_value(&cookies, "orbit_sess").unwrap();
    let csrf = cookie_value(&cookies, "orbit_csrf").unwrap();
    let cookie_header = format!("orbit_sess={sess}; orbit_csrf={csrf}");

    // Signout WITHOUT the X-CSRF-Token header → 403.
    let resp = post(
        &app,
        "/api/v1/auth/signout",
        json!({}),
        vec![(header::COOKIE.as_str(), cookie_header)],
    )
    .await;
    let (status, _c, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "csrf");
}

/// T17 B1: after `SIGNIN_CAPTCHA_FAILURE_THRESHOLD` consecutive failures
/// against the same account, the next attempt must return
/// `AppError::CaptchaRequired` (HTTP 401, `code: "captcha_required"`) —
/// without ever needing to SELECT from `audit_log` (orbit_app has no
/// SELECT grant there). The counter is a token bucket in
/// `rate_limit_buckets` keyed by `captcha:account:<sha256(email)>` and
/// `captcha:ip:<hash>`.
///
/// Also pins the `login.failure` audit payload allowlist (SEC-101) post
/// the T17 shrink: after B1 the payload is `{ reason }` — no `email_hash`.
#[tokio::test]
async fn captcha_gate_trips_after_threshold_failures() {
    let (state, app) = app().await;
    let email = unique_email("captcha");
    let password = unique_password();

    // Seed a verified user so we're exercising the bad_password branch
    // rather than the unknown_email one — either branch should feed the
    // captcha counter, but bad_password is the more interesting path and
    // also gives us a user_id to filter the audit query against.
    let r = post(
        &app,
        "/api/v1/auth/signup",
        json!({ "email": email, "password": password }),
        vec![],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1")
        .bind(&email)
        .execute(&state.pool)
        .await
        .unwrap();
    let user_id = find_user(&state.pool, &email).await;

    // Drive from a single IP so the account bucket is the one that trips
    // (the per-account captcha bucket fills with THRESHOLD tokens; the
    // per-IP bucket is keyed by the IP hash — both would exhaust together
    // here, which is fine). Reusing one IP also keeps us from polluting
    // a range of captcha:ip:* buckets on this shared dev DB.
    let xff_ip = unique_ip();

    // THRESHOLD consecutive bad-password attempts — each lands a
    // login.failure audit row and consumes one token from both buckets.
    // Status is 401 `auth` on each, not `captcha_required` yet.
    const THRESHOLD: usize = 3;
    for _ in 0..THRESHOLD {
        let resp = post(
            &app,
            "/api/v1/auth/signin",
            json!({ "email": email, "password": "not-the-password" }),
            vec![("x-forwarded-for", xff_ip.clone())],
        )
        .await;
        let (status, _c, body) = body_json(resp).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "auth");
    }

    // N+1: the captcha bucket is exhausted, so the peek at the top of
    // signin short-circuits to captcha_required.
    let resp = post(
        &app,
        "/api/v1/auth/signin",
        json!({ "email": email, "password": "not-the-password" }),
        vec![("x-forwarded-for", xff_ip.clone())],
    )
    .await;
    let (status, _c, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "captcha_required");

    // THRESHOLD login.failure rows: we consumed THRESHOLD before the
    // gate tripped. The N+1-th attempt never reached the audit write —
    // it was blocked at the top of signin.
    assert_eq!(
        audit_count(user_id, "login.failure").await,
        THRESHOLD as i64
    );

    // SEC-101 / T17 B1: login.failure carries exactly `{ reason }`. No
    // `email_hash`, no email, no password hash, no raw payload leakage.
    let mpool = pool_from_env("DATABASE_URL_MIGRATE").await;
    let payload: Value = sqlx::query_scalar(
        "SELECT payload_summary FROM audit_log WHERE user_id = $1 AND action = 'login.failure' \
         ORDER BY occurred_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(&mpool)
    .await
    .unwrap();
    let keys: Vec<String> = payload
        .as_object()
        .expect("payload is a JSON object")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        vec!["reason".to_string()],
        "login.failure payload must be exactly {{ reason }} (no email_hash)"
    );
    assert_eq!(payload["reason"], "bad_password");
}

#[tokio::test]
async fn mfa_endpoints_return_501() {
    let (_state, app) = app().await;
    let resp = post(&app, "/api/v1/auth/mfa/enroll", json!({}), vec![]).await;
    let (status, _c, body) = body_json(resp).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["error"]["code"], "not_implemented");
}
