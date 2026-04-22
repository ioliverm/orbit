//! Slice 3b T38 — user tax-preferences flow integration tests.
//!
//! Covers:
//!   - POST create (inserted); GET current returns the new row.
//!   - POST same-day same values (no_op); no new audit row.
//!   - POST same-day different values (updated_same_day).
//!   - POST next-day different values (closed_and_created); GET
//!     history shows both rows.
//!   - Cross-tenant 404 on GET current for another user.
//!   - Audit-allowlist sweep: payloads carry only `{ outcome }`.

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;

mod common;
use common::*;

#[tokio::test]
async fn post_inserted_then_get_current_returns_row() {
    let (state, app) = app().await;
    let s = onboarded(&state, &app, "utp-insert").await;

    let r = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4500",
            "sellToCoverEnabled": true,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["outcome"], "inserted");
    assert_eq!(body["current"]["countryIso2"], "ES");
    assert_eq!(body["current"]["sellToCoverEnabled"], true);

    let r = get(
        &app,
        "/api/v1/user-tax-preferences/current",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["current"]["countryIso2"], "ES");
}

#[tokio::test]
async fn post_same_day_same_values_is_no_op_no_new_audit() {
    let (state, app) = app().await;
    let s = onboarded(&state, &app, "utp-noop").await;

    let body = json!({
        "countryIso2": "ES",
        "rendimientoDelTrabajoPercent": "0.4500",
        "sellToCoverEnabled": true,
    });
    let headers = vec![
        (header::COOKIE.as_str(), s.cookie.clone()),
        ("x-csrf-token", s.csrf.clone()),
    ];

    let _ = post(
        &app,
        "/api/v1/user-tax-preferences",
        body.clone(),
        headers.clone(),
    )
    .await;

    let audit_pool = audit_pool().await;
    let n_before = audit_count(&audit_pool, s.user_id, "user_tax_preferences.upsert").await;

    // Second POST with identical body → no_op.
    let r = post(&app, "/api/v1/user-tax-preferences", body, headers).await;
    let (status, _c, resp) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["outcome"], "no_op");
    assert_eq!(resp["unchanged"], true);

    let n_after = audit_count(&audit_pool, s.user_id, "user_tax_preferences.upsert").await;
    assert_eq!(
        n_after, n_before,
        "no_op must NOT write a new audit row (AC-4.6.1)",
    );
}

#[tokio::test]
async fn post_same_day_different_values_updated_same_day() {
    let (state, app) = app().await;
    let s = onboarded(&state, &app, "utp-updated").await;

    let headers = vec![
        (header::COOKIE.as_str(), s.cookie.clone()),
        ("x-csrf-token", s.csrf.clone()),
    ];

    let _ = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4500",
            "sellToCoverEnabled": true,
        }),
        headers.clone(),
    )
    .await;

    // Same day, different percent → updated_same_day (200 OK).
    let r = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4700",
            "sellToCoverEnabled": true,
        }),
        headers,
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "updated_same_day");
    assert_eq!(body["current"]["rendimientoDelTrabajoPercent"], "0.4700");
}

/// Next-day different values — the prior open row is closed
/// (`to_date = prior.today`) and a successor is inserted. The API test
/// harness cannot easily fast-forward the server clock; instead we
/// rewind the existing row's `from_date` in the DB to simulate the
/// prior day, then submit a new body.
#[tokio::test]
async fn post_next_day_different_values_closed_and_created() {
    let (state, app) = app().await;
    let s = onboarded(&state, &app, "utp-cc").await;

    let headers = vec![
        (header::COOKIE.as_str(), s.cookie.clone()),
        ("x-csrf-token", s.csrf.clone()),
    ];

    let _ = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4500",
            "sellToCoverEnabled": true,
        }),
        headers.clone(),
    )
    .await;

    // Rewind the existing open row's from_date to yesterday so the
    // handler takes the close-and-create branch on the next save.
    let migrate_pool = audit_pool().await;
    sqlx::query(
        "UPDATE user_tax_preferences SET from_date = from_date - INTERVAL '1 day' \
         WHERE user_id = $1 AND to_date IS NULL",
    )
    .bind(s.user_id)
    .execute(&migrate_pool)
    .await
    .expect("rewind from_date");

    let r = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4700",
            "sellToCoverEnabled": false,
        }),
        headers,
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["outcome"], "closed_and_created");
    assert_eq!(body["current"]["sellToCoverEnabled"], false);

    // GET /user-tax-preferences should now list both rows.
    let r = get(
        &app,
        "/api/v1/user-tax-preferences",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let prefs = body["preferences"].as_array().unwrap();
    assert!(
        prefs.len() >= 2,
        "history must include both rows; got {}",
        prefs.len(),
    );
}

#[tokio::test]
async fn cross_tenant_cannot_see_other_users_preferences() {
    let (state, app) = app().await;
    let a = onboarded(&state, &app, "utp-xa").await;
    let b = onboarded(&state, &app, "utp-xb").await;

    // A saves.
    let _ = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4500",
            "sellToCoverEnabled": true,
        }),
        vec![
            (header::COOKIE.as_str(), a.cookie.clone()),
            ("x-csrf-token", a.csrf.clone()),
        ],
    )
    .await;

    // B reads — MUST not see A's row (RLS).
    let r = get(
        &app,
        "/api/v1/user-tax-preferences/current",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["current"].is_null(),
        "cross-tenant read leaked A's row: {body}",
    );
}

#[tokio::test]
async fn audit_payload_is_allowlisted() {
    let (state, app) = app().await;
    let s = onboarded(&state, &app, "utp-audit").await;

    let _ = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4500",
            "sellToCoverEnabled": true,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(&audit_pool, s.user_id, "user_tax_preferences.upsert").await;
    assert_eq!(payload["outcome"], "inserted");
    // SEC-101-strict per ADR-018 §5: ONLY `outcome`.
    let obj = payload.as_object().expect("payload is an object");
    assert_eq!(
        obj.len(),
        1,
        "audit payload must carry ONLY `outcome`; got {payload}",
    );
    assert_no_forbidden_keys(&payload, "user_tax_preferences.upsert");
}

#[tokio::test]
async fn rejects_missing_sell_to_cover_toggle() {
    let (state, app) = app().await;
    let s = onboarded(&state, &app, "utp-reqtoggle").await;

    let r = post(
        &app,
        "/api/v1/user-tax-preferences",
        json!({
            "countryIso2": "ES",
            "rendimientoDelTrabajoPercent": "0.4500",
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
        .any(|f| f["code"] == "user_tax_preferences.sell_to_cover_enabled.required"));
}
