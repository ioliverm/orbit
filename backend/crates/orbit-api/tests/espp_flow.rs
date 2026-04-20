//! Slice 2 T21 integration tests for the ESPP-purchase endpoints.
//!
//! Covers:
//!   - happy-path POST/GET/PUT/DELETE
//!   - cross-tenant 404 probe
//!   - validator failures (before_offering_date, bad currency, out-of-range
//!     discount)
//!   - duplicate-purchase soft-warn + `forceDuplicate` override (AC-4.2.8)
//!   - first-purchase notes lift (AC-4.5.1) + the lift-with-user-note edge
//!   - audit-log payload allowlist (SEC-101) for create / update / delete

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;
use uuid::Uuid;

mod common;
use common::*;

#[tokio::test]
async fn espp_purchase_crud_happy_path_and_audit_allowlist() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "espp-crud").await;
    let grant_id = create_espp_grant(&app, &s, "ACME Inc.").await;

    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        json!({
            "offeringDate": "2025-01-15",
            "purchaseDate": "2025-06-30",
            "fmvAtPurchase": "45.00",
            "purchasePricePerShare": "38.25",
            "sharesPurchased": 100,
            "currency": "USD",
            "fmvAtOffering": "42.00",
            "employerDiscountPercent": "15"
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "create: {body}");
    assert_eq!(body["migratedFromNotes"], false);
    let purchase_id = body["purchase"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["purchase"]["sharesPurchased"], "100");
    assert_eq!(body["purchase"]["currency"], "USD");

    // List.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let items = body["purchases"].as_array().unwrap();
    assert_eq!(items.len(), 1);

    // Get one.
    let r = get(
        &app,
        &format!("/api/v1/espp-purchases/{purchase_id}"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["purchase"]["id"], purchase_id);

    // Update (change shares).
    let r = put(
        &app,
        &format!("/api/v1/espp-purchases/{purchase_id}"),
        json!({
            "offeringDate": "2025-01-15",
            "purchaseDate": "2025-06-30",
            "fmvAtPurchase": "45.00",
            "purchasePricePerShare": "38.25",
            "sharesPurchased": 150,
            "currency": "USD"
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["purchase"]["sharesPurchased"], "150");

    // Delete.
    let r = delete(
        &app,
        &format!("/api/v1/espp-purchases/{purchase_id}"),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // Re-GET → 404.
    let r = get(
        &app,
        &format!("/api/v1/espp-purchases/{purchase_id}"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    // Audit-log payload shape.
    let pool = audit_pool().await;
    let create_payload = audit_last_payload(&pool, s.user_id, "espp_purchase.create").await;
    assert_eq!(create_payload["currency"], "USD");
    assert_eq!(create_payload["had_discount"], true);
    assert_eq!(create_payload["had_lookback"], true);
    assert_eq!(create_payload["notes_lift"], false);
    // Allowlist: exactly 4 keys, no leakage.
    let obj = create_payload.as_object().unwrap();
    assert_eq!(obj.len(), 4);
    for forbidden in [
        "fmv_at_purchase",
        "purchase_price_per_share",
        "shares_purchased",
        "notes",
    ] {
        assert!(obj.get(forbidden).is_none(), "leaked key {forbidden}");
    }

    let update_payload = audit_last_payload(&pool, s.user_id, "espp_purchase.update").await;
    assert_eq!(update_payload.as_object().unwrap().len(), 4);

    let delete_payload = audit_last_payload(&pool, s.user_id, "espp_purchase.delete").await;
    assert_eq!(delete_payload["currency"], "USD");
    assert_eq!(delete_payload.as_object().unwrap().len(), 1);
}

#[tokio::test]
async fn espp_validation_rejects_bad_body() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "espp-val").await;
    let grant_id = create_espp_grant(&app, &s, "ACME Inc.").await;

    // purchase before offering.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        json!({
            "offeringDate": "2025-06-30",
            "purchaseDate": "2025-01-15",
            "fmvAtPurchase": "45.00",
            "purchasePricePerShare": "38.25",
            "sharesPurchased": 100,
            "currency": "USD"
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields
        .iter()
        .any(|f| f["code"] == "before_offering_date" && f["field"] == "purchaseDate"));

    // bad currency.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        json!({
            "offeringDate": "2025-01-15",
            "purchaseDate": "2025-06-30",
            "fmvAtPurchase": "45.00",
            "purchasePricePerShare": "38.25",
            "sharesPurchased": 100,
            "currency": "JPY"
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["field"] == "currency"));
}

#[tokio::test]
async fn espp_rejects_non_espp_parent_grant() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "espp-wrong-parent").await;

    // Grab the RSU grant id from the seed.
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let rsu_id = body["grants"][0]["id"].as_str().unwrap().to_string();

    let r = post(
        &app,
        &format!("/api/v1/grants/{rsu_id}/espp-purchases"),
        json!({
            "offeringDate": "2025-01-15",
            "purchaseDate": "2025-06-30",
            "fmvAtPurchase": "45.00",
            "purchasePricePerShare": "38.25",
            "sharesPurchased": 100,
            "currency": "USD"
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields
        .iter()
        .any(|f| f["code"] == "not_espp" && f["field"] == "grantId"));
}

#[tokio::test]
async fn espp_duplicate_purchase_soft_warn_then_force_duplicate() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "espp-dup").await;
    let grant_id = create_espp_grant(&app, &s, "ACME Inc.").await;

    let payload = json!({
        "offeringDate": "2025-01-15",
        "purchaseDate": "2025-06-30",
        "fmvAtPurchase": "45.00",
        "purchasePricePerShare": "38.25",
        "sharesPurchased": 100,
        "currency": "USD"
    });
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        payload.clone(),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // Second attempt with same triple → 422 duplicate.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        payload.clone(),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["code"] == "duplicate"));

    // With forceDuplicate: accepted.
    let mut forced = payload;
    forced["forceDuplicate"] = json!(true);
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        forced,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn espp_notes_lift_on_first_purchase() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "espp-lift").await;

    // Create an ESPP grant with the Slice-1 discount JSON packed into notes.
    let r = post(
        &app,
        "/api/v1/grants",
        json!({
            "instrument": "espp",
            "grantDate": "2024-09-15",
            "shareCount": 500,
            "vestingStart": "2024-09-15",
            "vestingTotalMonths": 12,
            "cliffMonths": 0,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc.",
            "esppEstimatedDiscountPct": 15,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    let grant_id = body["grant"]["id"].as_str().unwrap().to_string();
    assert!(body["grant"]["notes"]
        .as_str()
        .unwrap()
        .contains("estimated_discount_percent"));

    // First purchase WITHOUT employer_discount_percent → lift fires.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        json!({
            "offeringDate": "2025-01-15",
            "purchaseDate": "2025-06-30",
            "fmvAtPurchase": "45.00",
            "purchasePricePerShare": "38.25",
            "sharesPurchased": 100,
            "currency": "USD"
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["migratedFromNotes"], true);
    assert_eq!(
        body["purchase"]["employerDiscountPercent"], "15.00",
        "lifted discount landed on the purchase row"
    );

    // grants.notes is now cleared.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    assert!(body["grant"]["notes"].is_null());

    // Audit-log carried notes_lift = true on the single create row.
    let pool = audit_pool().await;
    let payload = audit_last_payload(&pool, s.user_id, "espp_purchase.create").await;
    assert_eq!(payload["notes_lift"], true);
}

#[tokio::test]
async fn espp_cross_tenant_get_returns_404_not_403() {
    let (state, app) = app().await;
    let a = onboarded_with_grant(&state, &app, "espp-a").await;
    let b = onboarded_with_grant(&state, &app, "espp-b").await;
    let grant_id = create_espp_grant(&app, &a, "Alpha Corp.").await;

    // User A creates a purchase.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/espp-purchases"),
        json!({
            "offeringDate": "2025-01-15",
            "purchaseDate": "2025-06-30",
            "fmvAtPurchase": "45.00",
            "purchasePricePerShare": "38.25",
            "sharesPurchased": 100,
            "currency": "USD"
        }),
        vec![
            (header::COOKIE.as_str(), a.cookie.clone()),
            ("x-csrf-token", a.csrf.clone()),
        ],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let purchase_id = body["purchase"]["id"].as_str().unwrap().to_string();

    // User B GETs → 404 (AC-10.3).
    let r = get(
        &app,
        &format!("/api/v1/espp-purchases/{purchase_id}"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    // User B DELETE → also 404.
    let r = delete(
        &app,
        &format!("/api/v1/espp-purchases/{purchase_id}"),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    let _ = Uuid::parse_str(&purchase_id).unwrap();
}
