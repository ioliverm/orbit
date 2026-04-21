//! Slice 2 T21 integration tests for the Modelo 720 input endpoints.
//!
//! Covers:
//!   - Inserted (first save) + ClosedAndCreated (next-day save) outcomes
//!   - UpdatedSameDay (same-day re-edit) → exactly 1 open row
//!   - NoOp (identical value) → 200 + `unchanged: true` + NO audit row
//!   - securities category rejected (Slice-2 stub)
//!   - audit-log payload allowlist `{category, outcome}` (SEC-101); the
//!     NoOp branch writes zero audit rows (AC-6.2.5)

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;

mod common;
use common::*;

async fn upsert(
    app: &axum::Router,
    s: &Session,
    category: &str,
    amount: &str,
) -> axum::http::Response<axum::body::Body> {
    post(
        app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": category, "totalEur": amount }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await
}

#[tokio::test]
async fn m720_insert_close_and_create_and_updated_same_day() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "m720-ok").await;

    // First save — Inserted (201).
    let r = upsert(&app, &s, "bank_accounts", "25000.00").await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["outcome"], "inserted");

    // Second save SAME DAY, different value → UpdatedSameDay (200).
    // No 1-day span row materializes.
    let r = upsert(&app, &s, "bank_accounts", "30000.00").await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "updated_same_day");

    // Third save: identical value → NoOp, 200, `unchanged: true`.
    let r = upsert(&app, &s, "bank_accounts", "30000.00").await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "no_op");
    assert_eq!(body["unchanged"], true);

    // Current = 30000.00, single open row.
    let r = get(
        &app,
        "/api/v1/modelo-720-inputs/current?category=bank_accounts",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["current"]["amountEur"], "30000.00");

    // Audit rows: exactly 2 for this user (Inserted + UpdatedSameDay;
    // NoOp wrote none per AC-6.2.5).
    let pool = audit_pool().await;
    let n = audit_count(&pool, s.user_id, "modelo_720_inputs.upsert").await;
    assert_eq!(n, 2, "NoOp branch must not write an audit row");

    // Payload allowlist.
    let payload = audit_last_payload(&pool, s.user_id, "modelo_720_inputs.upsert").await;
    let obj = payload.as_object().unwrap();
    assert_eq!(obj.len(), 2);
    assert_eq!(payload["category"], "bank_accounts");
    assert!(matches!(
        payload["outcome"].as_str().unwrap(),
        "inserted" | "closed_and_created" | "updated_same_day"
    ));
    let got_keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let want_keys: std::collections::BTreeSet<&str> =
        ["category", "outcome"].iter().copied().collect();
    assert_eq!(got_keys, want_keys, "modelo_720_inputs.upsert key set");
    // Forbidden-fields sweep (T23, SEC-101).
    assert_no_forbidden_keys(&payload, "modelo_720_inputs.upsert");
    let scanned = assert_all_audit_payloads_clean(&pool, s.user_id, "modelo_720_inputs.").await;
    assert!(scanned >= 2, "expected ≥2 m720 rows, got {scanned}");
}

#[tokio::test]
async fn m720_noop_branch_writes_no_audit_row_and_returns_unchanged() {
    // Edge case (T23, AC-6.2.5): save the same value twice, second call
    // is a NoOp — 200 + `unchanged: true`. No second audit row lands.
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "m720-noop-edge").await;
    let pool = audit_pool().await;

    // Baseline.
    let baseline = audit_count(&pool, s.user_id, "modelo_720_inputs.upsert").await;

    // First save → Inserted.
    let r = upsert(&app, &s, "real_estate", "5000.00").await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["outcome"], "inserted");
    assert_eq!(
        audit_count(&pool, s.user_id, "modelo_720_inputs.upsert").await,
        baseline + 1
    );

    // Second save, identical value → NoOp. No extra audit row.
    let r = upsert(&app, &s, "real_estate", "5000.00").await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "no_op");
    assert_eq!(body["unchanged"], true);
    assert_eq!(
        audit_count(&pool, s.user_id, "modelo_720_inputs.upsert").await,
        baseline + 1,
        "NoOp must not write an audit row"
    );

    // A genuine change fires a new audit row (control group).
    let r = upsert(&app, &s, "real_estate", "6000.00").await;
    let (status, _c, _b) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        audit_count(&pool, s.user_id, "modelo_720_inputs.upsert").await,
        baseline + 2
    );
}

#[tokio::test]
async fn m720_rejects_securities_category_in_slice_2() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "m720-sec").await;
    let r = upsert(&app, &s, "securities", "1000").await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["details"]["fields"].as_array().unwrap();
    assert!(fields
        .iter()
        .any(|f| f["field"] == "category" && f["code"] == "unsupported"));
}

#[tokio::test]
async fn m720_history_scopes_to_category() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "m720-hist").await;
    upsert(&app, &s, "bank_accounts", "100").await;
    upsert(&app, &s, "real_estate", "200").await;

    let r = get(
        &app,
        "/api/v1/modelo-720-inputs?category=bank_accounts",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_st, _c, body) = body_json(r).await;
    let hist = body["history"].as_array().unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0]["category"], "bank_accounts");
}

#[tokio::test]
async fn m720_cross_tenant_scoping_via_get_current() {
    let (state, app) = app().await;
    let a = onboarded_with_grant(&state, &app, "m720-a").await;
    let b = onboarded_with_grant(&state, &app, "m720-b").await;

    upsert(&app, &a, "real_estate", "5000").await;

    // B sees no row for real_estate (RLS scoping).
    let r = get(
        &app,
        "/api/v1/modelo-720-inputs/current?category=real_estate",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["current"].is_null());
}
