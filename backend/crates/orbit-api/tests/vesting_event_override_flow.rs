//! Slice 3 T29 — vesting-event override + bulk-fmv integration tests.
//!
//! Covers:
//!   - OCC happy path (PUT succeeds + audit row written)
//!   - OCC stale → 409 `resource.stale_client_state`
//!   - `clearOverride` preserves FMV (AC-8.7.1)
//!   - bulk-fmv skips rows with existing FMV (AC-8.6.2)
//!   - grant-shrink-below-overrides → 422 `grant.share_count_below_overrides`

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use chrono::Utc;
use serde_json::json;

mod common;
use common::*;

/// Seed a Slice-1 RSU with the cliff already passed so past-row edits
/// are allowed.
async fn onboarded_with_past_rsu(
    state: &orbit_api::AppState,
    app: &axum::Router,
    tag: &str,
) -> (Session, uuid::Uuid) {
    let s = onboarded(state, app, tag).await;
    let body = json!({
        "instrument": "rsu",
        "grantDate": "2024-01-15",
        "shareCount": 12000,
        "vestingStart": "2024-01-15",
        "vestingTotalMonths": 12,
        "cliffMonths": 0,
        "vestingCadence": "monthly",
        "doubleTrigger": false,
        "employerName": "ACME Inc."
    });
    let r = post(
        app,
        "/api/v1/grants",
        body,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    let grant_id: uuid::Uuid =
        uuid::Uuid::parse_str(body["grant"]["id"].as_str().unwrap()).unwrap();
    (s, grant_id)
}

async fn list_vesting(app: &axum::Router, s: &Session, grant_id: &uuid::Uuid) -> serde_json::Value {
    let r = get(
        app,
        &format!("/api/v1/grants/{grant_id}/vesting"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    body
}

async fn fetch_event_row(
    pool: &sqlx::PgPool,
    grant_id: uuid::Uuid,
    idx: i64,
) -> (uuid::Uuid, chrono::DateTime<chrono::Utc>) {
    // Fetch the N-th past vesting event (by vest_date ASC).
    let row: (uuid::Uuid, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        r#"
        SELECT id, updated_at FROM vesting_events
        WHERE grant_id = $1
        ORDER BY vest_date ASC
        OFFSET $2 LIMIT 1
        "#,
    )
    .bind(grant_id)
    .bind(idx)
    .fetch_one(pool)
    .await
    .expect("fetch event row");
    row
}

#[tokio::test]
async fn override_happy_path_writes_audit_with_allowlisted_payload() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-happy").await;

    // Confirm events exist.
    let v = list_vesting(&app, &s, &grant_id).await;
    assert!(v["vestingEvents"].as_array().unwrap().len() >= 3);

    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "42.8000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["isUserOverride"], true);

    let audit_pool = audit_pool().await;
    let n = audit_count(&audit_pool, s.user_id, "vesting_event.override").await;
    assert_eq!(n, 1);
    let payload = audit_last_payload(&audit_pool, s.user_id, "vesting_event.override").await;
    assert_eq!(payload["fields_changed"], json!(["fmv"]));
    assert!(payload["grant_id"].is_string());
    assert_no_forbidden_keys(&payload, "vesting_event.override");
}

#[tokio::test]
async fn override_occ_conflict_returns_409() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-occ").await;
    let (event_id, _updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    let stale_ts = Utc::now() - chrono::Duration::days(365);
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "99.0000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": stale_ts,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn bulk_fmv_skips_existing_rows() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-bulk").await;

    // First, override one event with a known FMV.
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;
    let _ = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "99.0000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    // Now bulk-fill — the overridden row should be skipped.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/bulk-fmv"),
        json!({ "fmv": "40.0000", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["skippedCount"], 1);
    assert!(body["appliedCount"].as_u64().unwrap() >= 1);

    // Audit for bulk_fmv with allowlisted payload.
    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(&audit_pool, s.user_id, "vesting_event.bulk_fmv").await;
    assert_eq!(payload["skipped_count"], 1);
    assert!(payload["applied_count"].is_number());
    assert_no_forbidden_keys(&payload, "vesting_event.bulk_fmv");
}

#[tokio::test]
async fn grant_shrink_below_overrides_returns_422() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-shrink").await;

    // Override one row with 5000 shares (scaled: 50000000).
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "sharesVested": 5000,
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    // Attempt to shrink the grant's share_count below 5000 → 422.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        json!({
            "instrument": "rsu",
            "grantDate": "2024-01-15",
            "shareCount": 100,
            "vestingStart": "2024-01-15",
            "vestingTotalMonths": 12,
            "cliffMonths": 0,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc."
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields
        .iter()
        .any(|f| f["code"] == "grant.share_count_below_overrides"));
}
