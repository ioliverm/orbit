//! HTTP-level integration tests for the Slice 1 T13b grants surface.
//!
//! Covers: CRUD validation, cross-tenant 404, and the grant-detail vesting
//! round-trip. Shares the happy-path scaffolding with `wizard_flow.rs`.

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
    format!("orbit-t13b-grants-{tag}-{}@example.test", Uuid::new_v4())
}

fn unique_password() -> String {
    format!("Orbit-T13b-Grants-{}-Z8", Uuid::new_v4())
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

async fn put(
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

async fn delete(
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

/// Signup, verify-email, accept disclaimer, set residency. Returns
/// (user_id, cookie_header, csrf) with onboarding_stage == first_grant.
async fn onboarded(state: &AppState, app: &axum::Router, tag: &str) -> (Uuid, String, String) {
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
    let cookie_header = format!("orbit_sess={sess}; orbit_csrf={csrf}");
    let user_id = find_user(&state.pool, &email).await;

    post(
        app,
        "/api/v1/consent/disclaimer",
        json!({ "version": "v1-2026-04" }),
        vec![
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
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
            (header::COOKIE.as_str(), cookie_header.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;

    (user_id, cookie_header, csrf)
}

fn basic_rsu_body() -> Value {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grants_crud_round_trip_and_vesting_read() {
    let (state, app) = app().await;
    let (_uid, cookie, csrf) = onboarded(&state, &app, "crud").await;

    // Create.
    let r = post(
        &app,
        "/api/v1/grants",
        basic_rsu_body(),
        vec![
            (header::COOKIE.as_str(), cookie.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    let grant_id = body["grant"]["id"].as_str().unwrap().to_string();

    // List includes it.
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let grants = body["grants"].as_array().unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0]["id"], grant_id);

    // Get one.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        vec![(header::COOKIE.as_str(), cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["grant"]["instrument"], "rsu");

    // Update: change grant_date; events should be recomputed.
    let mut update_body = basic_rsu_body();
    update_body["grantDate"] = json!("2024-08-15");
    update_body["vestingStart"] = json!("2024-08-15");
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        update_body,
        vec![
            (header::COOKIE.as_str(), cookie.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["grant"]["grantDate"], "2024-08-15");
    let first_vest = body["vestingEvents"][0]["vestDate"].as_str().unwrap();
    assert_eq!(first_vest, "2025-08-15", "cliff shifted with vesting_start");

    // /grants/:id/vesting read.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting"),
        vec![(header::COOKIE.as_str(), cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let events = body["vestingEvents"].as_array().unwrap();
    assert_eq!(events.len(), 37);

    // Delete.
    let r = delete(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        vec![
            (header::COOKIE.as_str(), cookie.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // GET now 404s.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        vec![(header::COOKIE.as_str(), cookie)],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cross_tenant_get_returns_404_not_403() {
    let (state, app) = app().await;
    let (_a_uid, a_cookie, a_csrf) = onboarded(&state, &app, "tenant-a").await;
    let (_b_uid, b_cookie, _b_csrf) = onboarded(&state, &app, "tenant-b").await;

    // User A creates a grant.
    let r = post(
        &app,
        "/api/v1/grants",
        basic_rsu_body(),
        vec![
            (header::COOKIE.as_str(), a_cookie.clone()),
            ("x-csrf-token", a_csrf.clone()),
        ],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let a_grant_id = body["grant"]["id"].as_str().unwrap().to_string();

    // User B GETs user A's grant: 404, not 403 (AC-7.3 / SEC-023).
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_grant_id}"),
        vec![(header::COOKIE.as_str(), b_cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    // Cross-tenant vesting read: same 404.
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_grant_id}/vesting"),
        vec![(header::COOKIE.as_str(), b_cookie)],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn validation_rejects_bad_inputs() {
    let (state, app) = app().await;
    let (_uid, cookie, csrf) = onboarded(&state, &app, "val").await;

    // cliff > total.
    let mut body = basic_rsu_body();
    body["cliffMonths"] = json!(49);
    let r = post(
        &app,
        "/api/v1/grants",
        body,
        vec![
            (header::COOKIE.as_str(), cookie.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["code"] == "cliff_exceeds_vesting"));

    // share_count == 0.
    let mut body = basic_rsu_body();
    body["shareCount"] = json!(0);
    let r = post(
        &app,
        "/api/v1/grants",
        body,
        vec![
            (header::COOKIE.as_str(), cookie.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["code"] == "must_be_positive"));

    // NSO without strike.
    let r = post(
        &app,
        "/api/v1/grants",
        json!({
            "instrument": "nso",
            "grantDate": "2024-09-15",
            "shareCount": 10000,
            "vestingStart": "2024-09-15",
            "vestingTotalMonths": 48,
            "cliffMonths": 0,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc."
        }),
        vec![
            (header::COOKIE.as_str(), cookie.clone()),
            ("x-csrf-token", csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields
        .iter()
        .any(|f| f["field"] == "strikeAmount" && f["code"] == "required_for_options"));

    // RSU with strike sent → accepted, strike dropped silently.
    let r = post(
        &app,
        "/api/v1/grants",
        json!({
            "instrument": "rsu",
            "grantDate": "2024-09-15",
            "shareCount": 10000,
            "strikeAmount": "8.00",
            "strikeCurrency": "USD",
            "vestingStart": "2024-09-15",
            "vestingTotalMonths": 48,
            "cliffMonths": 12,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc."
        }),
        vec![(header::COOKIE.as_str(), cookie), ("x-csrf-token", csrf)],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body["grant"]["strikeAmount"].is_null());
}
