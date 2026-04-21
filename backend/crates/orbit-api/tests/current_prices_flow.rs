//! Slice 3 T29 — current-price endpoint integration tests.
//!
//! Covers:
//!   - Per-ticker CRUD happy path (list / put / delete)
//!   - Per-grant override CRUD (get / put / delete) + audit allowlist
//!   - Cross-tenant: user B cannot read / edit user A's per-grant override

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;

mod common;
use common::*;

#[tokio::test]
async fn ticker_price_put_list_delete_cycle() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "cp-list").await;

    // PUT — upsert.
    let r = put(
        &app,
        "/api/v1/current-prices/ACME",
        json!({ "price": "45.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ticker"], "ACME");

    // GET — list shows the row.
    let r = get(
        &app,
        "/api/v1/current-prices",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["prices"].as_array().unwrap().len(), 1);

    // DELETE.
    let r = delete(
        &app,
        "/api/v1/current-prices/ACME",
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // List now empty.
    let r = get(
        &app,
        "/api/v1/current-prices",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_status, _c, body) = body_json(r).await;
    assert_eq!(body["prices"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn ticker_price_rejects_negative_price() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "cp-neg").await;
    let r = put(
        &app,
        "/api/v1/current-prices/ACME",
        json!({ "price": "-1.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn grant_override_put_get_delete_audited() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "cp-override").await;

    // Fetch the grant id from the grants list.
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    let grant_id = body["grants"][0]["id"].as_str().unwrap();

    // GET — no override yet.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["override"].is_null());

    // PUT — upsert.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        json!({ "price": "100.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    // Audit: one row with allowlisted keys.
    let audit_pool = audit_pool().await;
    let n = audit_count(
        &audit_pool,
        s.user_id,
        "grant.current_price_override.upsert",
    )
    .await;
    assert_eq!(n, 1);
    let payload = audit_last_payload(
        &audit_pool,
        s.user_id,
        "grant.current_price_override.upsert",
    )
    .await;
    assert!(payload["grant_id"].is_string());
    assert_eq!(payload["had_prior"], false);
    assert_no_forbidden_keys(&payload, "grant.current_price_override.upsert");

    // DELETE.
    let r = delete(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    let n = audit_count(
        &audit_pool,
        s.user_id,
        "grant.current_price_override.delete",
    )
    .await;
    assert_eq!(n, 1);
}

#[tokio::test]
async fn grant_override_cross_tenant_is_404() {
    let (state, app) = app().await;
    let a = onboarded_with_grant(&state, &app, "cp-tenant-a").await;
    let b = onboarded_with_grant(&state, &app, "cp-tenant-b").await;

    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), a.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    let a_grant_id = body["grants"][0]["id"].as_str().unwrap();

    // B cannot GET A's grant-override.
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_grant_id}/current-price-override"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    // B cannot PUT A's grant-override.
    let r = put(
        &app,
        &format!("/api/v1/grants/{a_grant_id}/current-price-override"),
        json!({ "price": "99.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}
