//! Slice 2 T23 — cross-tenant sweep.
//!
//! Two users A and B each reach `complete` with their own Slice-2 state.
//! From B's session, every Slice-2 GET endpoint that accepts an id from
//! A's state must return 404 (direct-lookup paths) or omit A's rows from
//! the response (collection paths). This is the inverse of the
//! per-feature cross-tenant probes in `espp_flow.rs`, `trips_flow.rs`,
//! etc. — one file that sweeps *every* Slice-2 GET in one place so a new
//! endpoint added without an ownership filter trips this sweep before it
//! trips any feature-specific test.
//!
//! RLS scoping (ADR-014 §6): every SELECT runs under the
//! `orbit_app` role with `app.user_id = auth.user_id` set by the tx
//! opener. A row belonging to user A is simply invisible to user B; the
//! repo layer surfaces this as `Ok(None)` / empty vec, which the
//! handlers translate to 404 or `{ items: [] }` as appropriate.

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;

mod common;
use common::*;

#[tokio::test]
async fn user_b_never_sees_user_a_slice_2_state_on_any_get_endpoint() {
    let (state, app) = app().await;

    // User A: RSU + ESPP grants, one ESPP purchase, one trip, one M720
    // upsert.
    let a = onboarded_with_grant(&state, &app, "xt-a").await;
    let a_espp_grant = create_espp_grant(&app, &a, "Alpha Inc.").await;

    let r = post(
        &app,
        &format!("/api/v1/grants/{a_espp_grant}/espp-purchases"),
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
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "A espp purchase: {body}");
    let a_purchase_id = body["purchase"]["id"].as_str().unwrap().to_string();

    let r = post(
        &app,
        "/api/v1/trips",
        json!({
            "destinationCountry": "US",
            "fromDate": "2026-03-01",
            "toDate": "2026-04-15",
            "employerPaid": true,
            "purpose": "Cross-tenant probe trip for A",
            "eligibilityCriteria": {
                "services_outside_spain": true,
                "non_spanish_employer": true,
                "not_tax_haven": true,
                "no_double_exemption": true,
                "within_annual_cap": true
            }
        }),
        vec![
            (header::COOKIE.as_str(), a.cookie.clone()),
            ("x-csrf-token", a.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "A trip: {body}");
    let a_trip_id = body["trip"]["id"].as_str().unwrap().to_string();

    let r = post(
        &app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": "bank_accounts", "totalEur": "12345.00" }),
        vec![
            (header::COOKIE.as_str(), a.cookie.clone()),
            ("x-csrf-token", a.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // A's grant id for the direct-lookup ESPP list path.
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), a.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let a_grant_ids: Vec<String> = body["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap().to_string())
        .collect();
    assert!(!a_grant_ids.is_empty());
    let a_rsu_grant_id = a_grant_ids
        .iter()
        .find(|id| **id != a_espp_grant.to_string())
        .cloned()
        .unwrap();

    // User B: full Slice-2 state of their own so every collection
    // endpoint has SOMETHING to return — this catches a bug where the
    // handler returns global data rather than an empty list.
    let b = onboarded_with_grant(&state, &app, "xt-b").await;
    let _b_espp_grant = create_espp_grant(&app, &b, "Bravo Inc.").await;
    let r = post(
        &app,
        "/api/v1/trips",
        json!({
            "destinationCountry": "DE",
            "fromDate": "2026-02-01",
            "toDate": "2026-02-10",
            "employerPaid": false,
            "purpose": "B's own trip",
            "eligibilityCriteria": {
                "services_outside_spain": false,
                "non_spanish_employer": false,
                "not_tax_haven": true,
                "no_double_exemption": true,
                "within_annual_cap": true
            }
        }),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let r = post(
        &app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": "real_estate", "totalEur": "777.00" }),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // ---------- Direct-lookup-by-id endpoints: 404 on A's ids ----------

    // /api/v1/grants/:A-grant-id — the RSU one.
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_rsu_grant_id}"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "B must not see A's grant"
    );

    // /api/v1/grants/:A-espp-grant-id/espp-purchases — list path anchored
    // on A's grant. 404 (the grant itself is not owned by B; see
    // `espp_purchases::list_for_grant` guard).
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_espp_grant}/espp-purchases"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "B must not enumerate purchases under A's grant"
    );

    // /api/v1/espp-purchases/:A-purchase-id
    let r = get(
        &app,
        &format!("/api/v1/espp-purchases/{a_purchase_id}"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "B must not see A's purchase"
    );

    // /api/v1/trips/:A-trip-id
    let r = get(
        &app,
        &format!("/api/v1/trips/{a_trip_id}"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND, "B must not see A's trip");

    // ---------- Collection endpoints: A's rows are absent ----------

    // /api/v1/trips — B's list must not contain A's trip id.
    let r = get(
        &app,
        "/api/v1/trips",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let trips = body["trips"].as_array().unwrap();
    for row in trips {
        assert_ne!(
            row["id"].as_str().unwrap(),
            a_trip_id,
            "A's trip leaked into B's /trips list"
        );
    }

    // /api/v1/modelo-720-inputs/current?category=bank_accounts — B never
    // saved one, so `current` is null (A's bank_accounts row is
    // invisible). If the RLS scope leaked, B would see A's 12345.00.
    let r = get(
        &app,
        "/api/v1/modelo-720-inputs/current?category=bank_accounts",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["current"].is_null(),
        "B must not see A's bank_accounts m720 value: {body}"
    );

    // /api/v1/modelo-720-inputs?category=bank_accounts — B has no
    // history for this category. A's row must not appear.
    let r = get(
        &app,
        "/api/v1/modelo-720-inputs?category=bank_accounts",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["history"].as_array().unwrap().len(),
        0,
        "A's bank_accounts history leaked into B's view: {body}"
    );

    // /api/v1/auth/sessions — B sees only B's own session(s).
    let r = get(
        &app,
        "/api/v1/auth/sessions",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty());
    // Every row must be marked isCurrent for B's cookie (since B has one
    // session). If A's session id somehow leaked, the row would have
    // `isCurrent: false`.
    assert!(
        sessions.iter().all(|r| r["isCurrent"] == true),
        "B sees a non-current session from A? {body}"
    );

    // /api/v1/dashboard/stacked — B's dashboard only has B's employer
    // ("Bravo Inc." + "ACME Inc." from seed). A's grants must not be in
    // the per-employer curves or the grant_ids list.
    let r = get(
        &app,
        "/api/v1/dashboard/stacked",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let by_employer = body["byEmployer"].as_array().unwrap();
    for es in by_employer {
        let employer = es["employerName"].as_str().unwrap();
        assert_ne!(
            employer, "Alpha Inc.",
            "B sees A's employer on stacked dashboard"
        );
        for gid in es["grantIds"].as_array().unwrap() {
            let gid = gid.as_str().unwrap();
            assert!(
                !a_grant_ids.iter().any(|a_id| a_id == gid),
                "B's stacked dashboard contains A's grant id {gid}"
            );
            assert_ne!(gid, a_espp_grant.to_string());
        }
    }

    // ---------- PUT / DELETE cross-tenant also 404 (defense-in-depth) ----------
    let r = put(
        &app,
        &format!("/api/v1/trips/{a_trip_id}"),
        json!({
            "destinationCountry": "US",
            "fromDate": "2026-03-01",
            "toDate": "2026-04-15",
            "employerPaid": true,
            "eligibilityCriteria": {
                "services_outside_spain": true,
                "non_spanish_employer": true,
                "not_tax_haven": true,
                "no_double_exemption": true,
                "within_annual_cap": true
            }
        }),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    let r = delete(
        &app,
        &format!("/api/v1/espp-purchases/{a_purchase_id}"),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}
