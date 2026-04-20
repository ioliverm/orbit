//! Slice 2 T21 integration tests for GET /api/v1/dashboard/stacked.
//!
//! Shape-level coverage. The algorithm's correctness is pinned by the
//! shared fixture in `orbit-core/tests/fixtures/stacked_grants_cases.json`
//! plus the Rust + TS parity suites; here we check that the endpoint
//! wires the right data into the right response shape.

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;

mod common;
use common::*;

#[tokio::test]
async fn stacked_dashboard_single_employer_single_grant() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "stacked-single").await;

    let r = get(
        &app,
        "/api/v1/dashboard/stacked",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);

    let by_employer = body["byEmployer"].as_array().unwrap();
    assert_eq!(by_employer.len(), 1);
    let es = &by_employer[0];
    assert_eq!(es["employerName"], "ACME Inc.");
    assert_eq!(es["employerKey"], "acme inc.");
    assert_eq!(es["grantIds"].as_array().unwrap().len(), 1);
    assert!(!body["combined"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn stacked_dashboard_mixed_employers() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "stacked-mixed").await;

    // Add a second grant at a different employer.
    let mut body = basic_rsu_body();
    body["employerName"] = json!("Bravo Corp.");
    body["grantDate"] = json!("2024-10-15");
    body["vestingStart"] = json!("2024-10-15");
    let r = post(
        &app,
        "/api/v1/grants",
        body,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    let r = get(
        &app,
        "/api/v1/dashboard/stacked",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let by_employer = body["byEmployer"].as_array().unwrap();
    assert_eq!(by_employer.len(), 2);
    let names: Vec<_> = by_employer
        .iter()
        .map(|e| e["employerName"].as_str().unwrap())
        .collect();
    // Sorted alphabetically.
    assert_eq!(names, vec!["ACME Inc.", "Bravo Corp."]);
}

#[tokio::test]
async fn stacked_dashboard_mixed_instruments_carries_instrument_label() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "stacked-instr").await;

    // Second grant at the same employer, instrument = nso.
    let body = json!({
        "instrument": "nso",
        "grantDate": "2024-10-15",
        "shareCount": 15000,
        "strikeAmount": "8.00",
        "strikeCurrency": "USD",
        "vestingStart": "2024-10-15",
        "vestingTotalMonths": 48,
        "cliffMonths": 12,
        "vestingCadence": "monthly",
        "doubleTrigger": false,
        "employerName": "ACME Inc."
    });
    let r = post(
        &app,
        "/api/v1/grants",
        body,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    let r = get(
        &app,
        "/api/v1/dashboard/stacked",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let by_employer = body["byEmployer"].as_array().unwrap();
    assert_eq!(by_employer.len(), 1, "same-employer merge");
    assert_eq!(by_employer[0]["grantIds"].as_array().unwrap().len(), 2);

    // Every point has per_grant_breakdown entries; the instrument
    // strings include both "rsu" and "nso" somewhere across the curve.
    let points = by_employer[0]["points"].as_array().unwrap();
    let mut saw_rsu = false;
    let mut saw_nso = false;
    for p in points {
        for b in p["perGrantBreakdown"].as_array().unwrap() {
            match b["instrument"].as_str() {
                Some("rsu") => saw_rsu = true,
                Some("nso") => saw_nso = true,
                _ => {}
            }
        }
    }
    assert!(
        saw_rsu && saw_nso,
        "mixed instruments visible in drill-down"
    );
}
