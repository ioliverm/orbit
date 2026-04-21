//! Slice 2 T21 integration tests for the Art. 7.p trip endpoints.
//!
//! Covers:
//!   - happy-path POST/GET/PUT/DELETE + annual-cap tracker shape
//!   - cross-tenant 404 probe
//!   - validator failures: unknown criterion key, null criterion,
//!     missing criterion, non-boolean criterion, bad country length
//!   - audit-log payload allowlist (SEC-101) — `{country, criteria_answered,
//!     employer_paid}` only; no dates, no purpose, no criterion values.

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;

mod common;
use common::*;

fn trip_body() -> serde_json::Value {
    json!({
        "destinationCountry": "US",
        "fromDate": "2026-03-01",
        "toDate": "2026-04-15",
        "employerPaid": true,
        "purpose": "Kickoff with NYC team",
        "eligibilityCriteria": {
            "services_outside_spain": true,
            "non_spanish_employer": true,
            "not_tax_haven": true,
            "no_double_exemption": true,
            "within_annual_cap": true
        }
    })
}

#[tokio::test]
async fn trip_crud_happy_path_and_annual_tracker_embedded() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "trip-crud").await;

    // Create a trip.
    let r = post(
        &app,
        "/api/v1/trips",
        trip_body(),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "create: {body}");
    let trip_id = body["trip"]["id"].as_str().unwrap().to_string();
    assert_eq!(body["trip"]["destinationCountry"], "US");
    assert_eq!(body["trip"]["employerPaid"], true);

    // List with ?year=2026 → our trip shows up + tracker populated.
    let r = get(
        &app,
        "/api/v1/trips?year=2026",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let trips = body["trips"].as_array().unwrap();
    assert_eq!(trips.len(), 1);
    let tracker = &body["annualCapTracker"];
    assert_eq!(tracker["year"], 2026);
    assert_eq!(tracker["tripCount"], 1);
    assert_eq!(tracker["employerPaidTripCount"], 1);
    assert_eq!(
        tracker["criteriaMetCountByKey"]["services_outside_spain"],
        1
    );

    // Year override for a year with zero trips.
    let r = get(
        &app,
        "/api/v1/trips?year=2020",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    assert_eq!(body["annualCapTracker"]["tripCount"], 0);

    // Get one.
    let r = get(
        &app,
        &format!("/api/v1/trips/{trip_id}"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["trip"]["id"], trip_id);

    // Update (flip `within_annual_cap`).
    let mut updated = trip_body();
    updated["eligibilityCriteria"]["within_annual_cap"] = json!(false);
    let r = put(
        &app,
        &format!("/api/v1/trips/{trip_id}"),
        updated,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["trip"]["eligibilityCriteria"]["within_annual_cap"],
        false
    );

    // Delete.
    let r = delete(
        &app,
        &format!("/api/v1/trips/{trip_id}"),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // Audit-log allowlist.
    let pool = audit_pool().await;
    let payload = audit_last_payload(&pool, s.user_id, "trip.create").await;
    let obj = payload.as_object().unwrap();
    assert_eq!(obj.len(), 3);
    assert_eq!(payload["destination_country_iso2"], "US");
    assert_eq!(payload["criteria_answered"], 5);
    assert_eq!(payload["employer_paid"], true);
    // Positive allowlist: keys-set equals expected.
    let got_keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let want_keys: std::collections::BTreeSet<&str> = [
        "destination_country_iso2",
        "criteria_answered",
        "employer_paid",
    ]
    .iter()
    .copied()
    .collect();
    assert_eq!(got_keys, want_keys, "trip.create key set");
    // Forbidden-fields sweep (T23, SEC-101).
    assert_no_forbidden_keys(&payload, "trip.create");

    // Update payload obeys the same shape.
    let upd_payload = audit_last_payload(&pool, s.user_id, "trip.update").await;
    assert_eq!(upd_payload.as_object().unwrap().len(), 3);
    assert_no_forbidden_keys(&upd_payload, "trip.update");

    // Delete payload is empty (no detail surface at all).
    let del_payload = audit_last_payload(&pool, s.user_id, "trip.delete").await;
    assert_eq!(del_payload.as_object().unwrap().len(), 0);
    assert_no_forbidden_keys(&del_payload, "trip.delete");

    // Cross-action sweep: every trip.* row for this user passes.
    let scanned = assert_all_audit_payloads_clean(&pool, s.user_id, "trip.").await;
    assert!(scanned >= 3, "expected ≥3 trip.* rows, got {scanned}");
}

#[tokio::test]
async fn trip_validation_rejects_bad_eligibility() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "trip-val").await;

    // null criterion.
    let mut b = trip_body();
    b["eligibilityCriteria"]["not_tax_haven"] = json!(null);
    let r = post(
        &app,
        "/api/v1/trips",
        b,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["code"] == "answer_required"));

    // missing criterion.
    let mut b = trip_body();
    b["eligibilityCriteria"]
        .as_object_mut()
        .unwrap()
        .remove("no_double_exemption");
    let r = post(
        &app,
        "/api/v1/trips",
        b,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // unknown key.
    let mut b = trip_body();
    b["eligibilityCriteria"]
        .as_object_mut()
        .unwrap()
        .insert("custom_key".into(), json!(true));
    let r = post(
        &app,
        "/api/v1/trips",
        b,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["code"] == "unknown_key"));

    // bad country length.
    let mut b = trip_body();
    b["destinationCountry"] = json!("USA");
    let r = post(
        &app,
        "/api/v1/trips",
        b,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn trip_criteria_shape_strict_validation() {
    // Edge cases (T23, AC-5.2.3):
    //   - extra key beyond the five allowed → 422 `unknown_key`.
    //   - non-boolean value → 422.
    //   - exactly-five expected keys all present → accepted.
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "trip-criteria").await;

    // Extra key (above the five-key allowlist).
    let mut b = trip_body();
    b["eligibilityCriteria"]
        .as_object_mut()
        .unwrap()
        .insert("fabricated_criterion".into(), json!(true));
    let r = post(
        &app,
        "/api/v1/trips",
        b,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    // Field path is `eligibilityCriteria.{key}` for per-key errors; the
    // unknown-key error surfaces the offending key in the field path.
    assert!(
        fields.iter().any(|f| f["code"] == "unknown_key"
            && f["field"]
                .as_str()
                .unwrap_or("")
                .starts_with("eligibilityCriteria")),
        "expected unknown_key error scoped to eligibilityCriteria.*: {body}"
    );

    // Non-boolean value (string instead of bool).
    let mut b = trip_body();
    b["eligibilityCriteria"]["not_tax_haven"] = json!("yes");
    let r = post(
        &app,
        "/api/v1/trips",
        b,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(
        fields
            .iter()
            .any(|f| f["field"].as_str().unwrap_or("").contains("not_tax_haven")),
        "non-boolean criterion must surface the offending field"
    );

    // Non-boolean (number). Should also reject.
    let mut b = trip_body();
    b["eligibilityCriteria"]["within_annual_cap"] = json!(1);
    let r = post(
        &app,
        "/api/v1/trips",
        b,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn trip_cross_tenant_404() {
    let (state, app) = app().await;
    let a = onboarded_with_grant(&state, &app, "trip-a").await;
    let b = onboarded_with_grant(&state, &app, "trip-b").await;

    let r = post(
        &app,
        "/api/v1/trips",
        trip_body(),
        vec![
            (header::COOKIE.as_str(), a.cookie.clone()),
            ("x-csrf-token", a.csrf.clone()),
        ],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let trip_id = body["trip"]["id"].as_str().unwrap().to_string();

    let r = get(
        &app,
        &format!("/api/v1/trips/{trip_id}"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}
