//! HTTP-level integration tests for the Slice 1 T13b wizard surface
//! (`/consent/disclaimer`, `/residency/*`, `/auth/me` extensions, the
//! onboarding-gate middleware) and the happy-path end-to-end flow
//! (signup → verify → disclaimer → residency → first grant).
//!
//! Feature-gated behind `integration-tests` — mirrors
//! `tests/auth_flow.rs`. A fresh `cargo test --workspace` stays
//! Postgres-free.

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
// Harness (duplicated from auth_flow.rs — kept local per file to avoid
// introducing a shared test-support crate for Slice 1).
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
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap(),
    }
}

async fn app() -> (AppState, axum::Router) {
    let pool = pool_from_env("DATABASE_URL").await;
    let state = make_state(pool);
    (state.clone(), router(state))
}

fn unique_email(tag: &str) -> String {
    format!("orbit-t13b-{tag}-{}@example.test", Uuid::new_v4())
}

fn unique_password() -> String {
    format!("Orbit-T13b-Ok-{}-Z8", Uuid::new_v4())
}

fn unique_ip() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() & 0xff;
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

async fn audit_count(user_id: Uuid, action: &str) -> i64 {
    let pool = pool_from_env("DATABASE_URL_MIGRATE").await;
    sqlx::query_scalar("SELECT COUNT(*)::bigint FROM audit_log WHERE user_id = $1 AND action = $2")
        .bind(user_id)
        .bind(action)
        .fetch_one(&pool)
        .await
        .unwrap_or(0)
}

async fn audit_last_payload(user_id: Uuid, action: &str) -> Value {
    let pool = pool_from_env("DATABASE_URL_MIGRATE").await;
    sqlx::query_scalar(
        "SELECT payload_summary FROM audit_log WHERE user_id = $1 AND action = $2 \
         ORDER BY occurred_at DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(action)
    .fetch_one(&pool)
    .await
    .unwrap_or(Value::Null)
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

/// PUT helper mirroring [`post`]. Exists because the grant.update probe
/// needs a PUT and we keep the per-file harness free of a shared crate.
async fn post_like_put(
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

/// Signup + verify-email and return (user_id, cookie_header, csrf_token).
async fn signup_verified(
    state: &AppState,
    app: &axum::Router,
    tag: &str,
) -> (Uuid, String, String) {
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
    let (status, cookies, _) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let sess = cookie_value(&cookies, "orbit_sess").expect("orbit_sess");
    let csrf = cookie_value(&cookies, "orbit_csrf").expect("orbit_csrf");
    let user_id = find_user(&state.pool, &email).await;
    let cookie_header = format!("orbit_sess={sess}; orbit_csrf={csrf}");
    (user_id, cookie_header, csrf)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn autonomias_list_is_public_and_cacheable() {
    let (_state, app) = app().await;
    let resp = get(&app, "/api/v1/residency/autonomias", vec![]).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cache = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(cache.contains("max-age=3600"), "missing cache-control");
    let (_st, _c, body) = body_json(resp).await;
    let autonomias = body["autonomias"].as_array().expect("array");
    assert!(autonomias.iter().any(|a| a["code"] == "ES-MD"));
    let pv = autonomias.iter().find(|a| a["code"] == "ES-PV").unwrap();
    assert_eq!(pv["foral"], true);
}

#[tokio::test]
async fn happy_path_signup_to_first_grant() {
    let (state, app) = app().await;
    let (user_id, cookie_header, csrf) = signup_verified(&state, &app, "happy").await;

    // /auth/me — disclaimer stage.
    let r = get(
        &app,
        "/api/v1/auth/me",
        vec![(header::COOKIE.as_str(), cookie_header.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    assert_eq!(body["onboardingStage"], "disclaimer");
    assert_eq!(body["disclaimerAccepted"], false);
    assert!(body["residency"].is_null());

    // Disclaimer.
    let r = post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        audit_count(user_id, "dsr.consent.disclaimer_accepted").await,
        1
    );

    // Second POST is a no-op audit-wise (idempotent).
    let r = post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        audit_count(user_id, "dsr.consent.disclaimer_accepted").await,
        1,
        "idempotent: no duplicate audit row"
    );

    // Residency stage.
    let r = get(
        &app,
        "/api/v1/auth/me",
        vec![(header::COOKIE.as_str(), cookie_header.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    assert_eq!(body["onboardingStage"], "residency");
    assert_eq!(body["disclaimerAccepted"], true);

    // Residency create.
    let r = post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-MD",
            "primaryCurrency": "EUR",
            "regimeFlags": []
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["residency"]["subJurisdiction"], "ES-MD");
    assert!(body["residency"]["toDate"].is_null());
    assert_eq!(body["primaryCurrency"], "EUR");
    assert_eq!(audit_count(user_id, "residency.create").await, 1);

    // Residency payload booleans only (SEC-101 / AC-4.1.8).
    let p = audit_last_payload(user_id, "residency.create").await;
    assert_eq!(p["autonomia_changed"], true);
    assert_eq!(p["beckham_changed"], false);
    // EUR is the users default; no change.
    assert_eq!(p["currency_changed"], false);

    // First-grant stage.
    let r = get(
        &app,
        "/api/v1/auth/me",
        vec![(header::COOKIE.as_str(), cookie_header.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    assert_eq!(body["onboardingStage"], "first_grant");
    assert_eq!(body["residency"]["subJurisdiction"], "ES-MD");

    // First grant: RSU 30,000 shares, 4y/1y/monthly, double-trigger.
    let r = post(
        &app,
        "/api/v1/grants",
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
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["grant"]["shareCount"], "30000");
    assert_eq!(body["grant"]["shareCountScaled"], 300_000_000);
    assert_eq!(body["grant"]["doubleTrigger"], true);
    let grant_id = body["grant"]["id"].as_str().unwrap().to_string();
    let events = body["vestingEvents"].as_array().expect("events");
    // 48 months vs 12-month cliff, monthly: cliff event + 36 monthly events.
    assert_eq!(events.len(), 37);
    // No event before 2025-09-15.
    let first_date = events[0]["vestDate"].as_str().unwrap();
    assert_eq!(first_date, "2025-09-15");

    // Audit payload is allowlisted: no share_count, no strike.
    let p = audit_last_payload(user_id, "grant.create").await;
    assert_eq!(p["instrument"], "rsu");
    assert_eq!(p["double_trigger"], true);
    assert_eq!(p["cadence"], "monthly");
    assert!(
        p.get("share_count").is_none() && p.get("shareCount").is_none(),
        "SEC-101: no share count in payload_summary"
    );

    // /auth/me is now `complete`.
    let r = get(
        &app,
        "/api/v1/auth/me",
        vec![(header::COOKIE.as_str(), cookie_header.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    assert_eq!(body["onboardingStage"], "complete");

    // /grants/:id/vesting round-trips the derived events.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting"),
        vec![(header::COOKIE.as_str(), cookie_header)],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let events = body["vestingEvents"].as_array().expect("events");
    assert_eq!(events.len(), 37);
    // Double-trigger + no liquidity event: every past event is
    // time_vested_awaiting_liquidity; upcoming events are `upcoming`.
    for e in events {
        let state = e["state"].as_str().unwrap();
        assert!(
            matches!(state, "upcoming" | "time_vested_awaiting_liquidity"),
            "unexpected state {state}"
        );
    }
}

#[tokio::test]
async fn undisclaimed_user_hitting_grants_is_blocked_with_stage() {
    let (state, app) = app().await;
    let (_uid, cookie_header, csrf) = signup_verified(&state, &app, "gate-disc").await;

    // GET /grants should 403 before disclaimer is accepted.
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), cookie_header.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "onboarding.required");
    assert_eq!(body["error"]["details"]["stage"], "disclaimer");

    // POST /grants (state-changing) also 403 with the same code.
    let r = post(
        &app,
        "/api/v1/grants",
        json!({}),
        vec![
            (header::COOKIE.as_str(), cookie_header),
            ("x-csrf-token", csrf),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "onboarding.required");
    assert_eq!(body["error"]["details"]["stage"], "disclaimer");
}

#[tokio::test]
async fn residency_edit_closes_prior_row_and_opens_new_one() {
    let (state, app) = app().await;
    let (user_id, cookie_header, csrf) = signup_verified(&state, &app, "res-edit").await;

    // Disclaimer first.
    let r = post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // First residency: Madrid.
    let r = post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-MD",
            "primaryCurrency": "EUR",
            "regimeFlags": []
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // Second residency: País Vasco with foral flag.
    let r = post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-PV",
            "primaryCurrency": "EUR",
            "regimeFlags": ["foral_pais_vasco"]
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // Two rows, first one closed with to_date = today, second one open.
    let mpool = pool_from_env("DATABASE_URL_MIGRATE").await;
    let rows: Vec<(String, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT sub_jurisdiction, to_date FROM residency_periods \
         WHERE user_id = $1 ORDER BY from_date ASC, created_at ASC",
    )
    .bind(user_id)
    .fetch_all(&mpool)
    .await
    .expect("rows");
    assert_eq!(rows.len(), 2, "exactly two residency rows");
    assert_eq!(rows[0].0, "ES-MD");
    assert!(rows[0].1.is_some(), "prior row closed on second POST");
    assert_eq!(rows[1].0, "ES-PV");
    assert!(rows[1].1.is_none(), "new row open");
    assert_eq!(audit_count(user_id, "residency.create").await, 2);
}

#[tokio::test]
async fn residency_rejects_bad_inputs() {
    let (state, app) = app().await;
    let (_uid, cookie_header, csrf) = signup_verified(&state, &app, "res-bad").await;

    // disclaimer first
    post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;

    // Unknown autonomía.
    let r = post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-ZZ",
            "primaryCurrency": "EUR",
            "regimeFlags": []
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation");

    // Foral flag mismatch.
    let r = post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-MD",
            "primaryCurrency": "EUR",
            "regimeFlags": ["foral_pais_vasco"]
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header),
            ("x-csrf-token", csrf),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["code"] == "foral_mismatch"));
}

// ---------------------------------------------------------------------------
// T15 — additional acceptance-criteria probes (SEC-101 audit payload-summary
// allowlist, disclaimer payload shape, onboarding-stage transitions).
// ---------------------------------------------------------------------------

/// Helper: fetch the latest `payload_summary` row as an owned JSON object,
/// so the caller can assert the exact key set (SEC-101).
async fn audit_last_payload_keys(user_id: Uuid, action: &str) -> Vec<String> {
    let payload = audit_last_payload(user_id, action).await;
    payload
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// AC G-9: the `dsr.consent.disclaimer_accepted` audit row carries only
/// `{ version }` — no PII, no grant data (SEC-101 / G-26).
#[tokio::test]
async fn disclaimer_audit_payload_is_version_only() {
    let (state, app) = app().await;
    let (user_id, cookie_header, csrf) = signup_verified(&state, &app, "disc-pl").await;

    let r = post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header),
            ("x-csrf-token", csrf),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    let payload = audit_last_payload(user_id, "dsr.consent.disclaimer_accepted").await;
    assert_eq!(payload["version"], "v1-2026-04", "version field present");

    let keys = audit_last_payload_keys(user_id, "dsr.consent.disclaimer_accepted").await;
    assert_eq!(keys, vec!["version".to_string()], "exactly one key allowed");
}

/// AC-G-32 / SEC-101: the `grant.create` audit row carries only
/// `{ instrument, double_trigger, cadence }`. No share count, no strike, no
/// employer name, no ticker, no notes.
#[tokio::test]
async fn grant_create_audit_payload_allowlist_is_strict() {
    let (state, app) = app().await;
    let (user_id, cookie_header, csrf) = signup_verified(&state, &app, "gc-pl").await;

    // Walk through to first-grant stage.
    post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-MD",
            "primaryCurrency": "EUR",
            "regimeFlags": []
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;

    // Create an NSO grant with a strike — the payload MUST not leak strike.
    let r = post(
        &app,
        "/api/v1/grants",
        json!({
            "instrument": "nso",
            "grantDate": "2024-09-15",
            "shareCount": 30000,
            "strikeAmount": "8.00",
            "strikeCurrency": "USD",
            "vestingStart": "2024-09-15",
            "vestingTotalMonths": 48,
            "cliffMonths": 12,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc.",
            "ticker": "ACME",
            "notes": "internal grant note — must not leak"
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header),
            ("x-csrf-token", csrf),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    let payload = audit_last_payload(user_id, "grant.create").await;
    // Whitelisted keys present with the expected values.
    assert_eq!(payload["instrument"], "nso");
    assert_eq!(payload["double_trigger"], false);
    assert_eq!(payload["cadence"], "monthly");

    // Strict key set.
    let mut keys = audit_last_payload_keys(user_id, "grant.create").await;
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "cadence".to_string(),
            "double_trigger".to_string(),
            "instrument".to_string(),
        ],
        "SEC-101 allowlist violated — extra keys in payload_summary"
    );

    // Belt-and-braces negative assertions for the sensitive fields callers
    // most often forget (share_count, strike, employer, ticker, notes).
    for forbidden in [
        "share_count",
        "shareCount",
        "strike_amount",
        "strikeAmount",
        "strike_currency",
        "strikeCurrency",
        "employer_name",
        "employerName",
        "ticker",
        "notes",
    ] {
        assert!(
            payload.get(forbidden).is_none(),
            "SEC-101: forbidden key `{forbidden}` present in grant.create payload"
        );
    }
}

/// T17 O6 (sec-review): the `grant.update` audit row carries exactly
/// `{ instrument, double_trigger, cadence }` — symmetrical to the
/// `grant.create` probe. Catches regressions where an `update` handler
/// accidentally widens the payload (e.g. to include a diff).
#[tokio::test]
async fn grant_update_audit_payload_allowlist_is_strict() {
    let (state, app) = app().await;
    let (user_id, cookie_header, csrf) = signup_verified(&state, &app, "gu-pl").await;

    // Walk through to first-grant stage.
    post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-MD",
            "primaryCurrency": "EUR",
            "regimeFlags": []
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;

    // Create the grant.
    let r = post(
        &app,
        "/api/v1/grants",
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
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    let grant_id = body["grant"]["id"].as_str().unwrap().to_string();

    // Update with a set of noisy fields that, if leaked, would be SEC-101
    // violations (share_count bumped, strike set, notes filled, ticker
    // added). The payload must still be exactly the three allowlisted keys.
    let r = post_like_put(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        json!({
            "instrument": "rsu",
            "grantDate": "2024-09-15",
            "shareCount": 45000,
            "vestingStart": "2024-09-15",
            "vestingTotalMonths": 48,
            "cliffMonths": 12,
            "vestingCadence": "quarterly",
            "doubleTrigger": true,
            "employerName": "ACME Inc.",
            "ticker": "ACME",
            "notes": "post-refresh — must not leak"
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header),
            ("x-csrf-token", csrf),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    let payload = audit_last_payload(user_id, "grant.update").await;
    assert_eq!(payload["instrument"], "rsu");
    assert_eq!(payload["double_trigger"], true);
    assert_eq!(payload["cadence"], "quarterly");

    let mut keys = audit_last_payload_keys(user_id, "grant.update").await;
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "cadence".to_string(),
            "double_trigger".to_string(),
            "instrument".to_string(),
        ],
        "SEC-101 allowlist violated on grant.update"
    );
    for forbidden in [
        "share_count",
        "shareCount",
        "strike_amount",
        "strikeAmount",
        "employer_name",
        "employerName",
        "ticker",
        "notes",
    ] {
        assert!(
            payload.get(forbidden).is_none(),
            "SEC-101: forbidden key `{forbidden}` present in grant.update payload"
        );
    }
}

/// T17 O8 (sec-review): the `residency.create` audit row carries exactly
/// `{ autonomia_changed, beckham_changed, currency_changed }` (booleans
/// only — AC-4.1.8 / SEC-101). Symmetrical to the grant.create probe.
#[tokio::test]
async fn residency_create_audit_payload_allowlist_is_strict() {
    let (state, app) = app().await;
    let (user_id, cookie_header, csrf) = signup_verified(&state, &app, "rc-pl").await;

    // Disclaimer.
    post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;

    // Residency with regime flags + a currency change — guarantees the
    // `*_changed` booleans take both values across a run and makes sure
    // none of the *inputs* (autonomía code, currency code, flag list)
    // leak into the audit payload.
    let r = post(
        &app,
        "/api/v1/residency",
        json!({
            "jurisdiction": "ES",
            "subJurisdiction": "ES-PV",
            "primaryCurrency": "USD",
            "regimeFlags": ["foral_pais_vasco", "beckham_law"]
        }),
        vec![
            (header::COOKIE.as_str(), cookie_header),
            ("x-csrf-token", csrf),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    let payload = audit_last_payload(user_id, "residency.create").await;
    assert_eq!(payload["autonomia_changed"], true);
    assert_eq!(payload["beckham_changed"], true);
    assert_eq!(payload["currency_changed"], true);

    let mut keys = audit_last_payload_keys(user_id, "residency.create").await;
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "autonomia_changed".to_string(),
            "beckham_changed".to_string(),
            "currency_changed".to_string(),
        ],
        "SEC-101 allowlist violated on residency.create"
    );
    for forbidden in [
        "jurisdiction",
        "sub_jurisdiction",
        "subJurisdiction",
        "primary_currency",
        "primaryCurrency",
        "regime_flags",
        "regimeFlags",
    ] {
        assert!(
            payload.get(forbidden).is_none(),
            "SEC-101: forbidden key `{forbidden}` present in residency.create payload"
        );
    }
}

/// AC-G-8: a user past disclaimer but without residency gets 403
/// `onboarding.required` with `stage = residency` when hitting a gated
/// endpoint. Covers the second transition of the onboarding-gate state
/// machine (the first — `disclaimer` — is already asserted by
/// `undisclaimed_user_hitting_grants_is_blocked_with_stage`).
#[tokio::test]
async fn post_disclaimer_pre_residency_user_is_blocked_with_residency_stage() {
    let (state, app) = app().await;
    let (_uid, cookie_header, csrf) = signup_verified(&state, &app, "gate-res").await;

    // Accept the disclaimer only.
    let r = post(
        &app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // GET /grants should 403 with stage=residency (user still hasn't
    // submitted residency).
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), cookie_header.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "onboarding.required");
    assert_eq!(body["error"]["details"]["stage"], "residency");
}
