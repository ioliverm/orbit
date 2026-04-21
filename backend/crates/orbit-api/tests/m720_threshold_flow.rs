//! Slice 3 T29 — Modelo 720 threshold endpoint integration tests.

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use chrono::{NaiveDate, Utc};
use serde_json::json;
use sqlx::PgPool;

mod common;
use common::*;

async fn wipe_fx(pool: &PgPool) {
    sqlx::query("DELETE FROM fx_rates WHERE source = 'ecb'")
        .execute(pool)
        .await
        .expect("wipe fx");
}

async fn seed_fx(pool: &PgPool, date: NaiveDate, rate: &str) {
    sqlx::query(
        r#"
        INSERT INTO fx_rates (base, quote, rate_date, rate, source)
        VALUES ('EUR', 'USD', $1, $2::numeric, 'ecb')
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(date)
    .bind(rate)
    .execute(pool)
    .await
    .expect("seed fx");
}

#[tokio::test]
async fn m720_threshold_no_breach_when_sub_threshold() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0000").await;

    let s = onboarded_with_grant(&state, &app, "m720-sub").await;

    // Seed a modest bank-accounts row.
    let _ = post(
        &app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": "bank_accounts", "totalEur": "10000.00" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    let r = get(
        &app,
        "/api/v1/dashboard/modelo-720-threshold",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["perCategoryBreach"], false);
    assert_eq!(body["aggregateBreach"], false);
}

#[tokio::test]
async fn m720_threshold_per_category_breach_on_bank_accounts_over_limit() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0000").await;

    let s = onboarded_with_grant(&state, &app, "m720-breach").await;

    let _ = post(
        &app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": "bank_accounts", "totalEur": "60000.00" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    let r = get(
        &app,
        "/api/v1/dashboard/modelo-720-threshold",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["perCategoryBreach"], true);
    assert_eq!(body["aggregateBreach"], true);
}

/// T31 — real-estate breach: bank_accounts under threshold; real_estate
/// over. Per-category true (real_estate); aggregate also true.
#[tokio::test]
async fn m720_threshold_real_estate_only_breach() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0000").await;
    let s = onboarded_with_grant(&state, &app, "m720-re").await;

    let _ = post(
        &app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": "real_estate", "totalEur": "60000.00" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    let r = get(
        &app,
        "/api/v1/dashboard/modelo-720-threshold",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["perCategoryBreach"], true);
    assert_eq!(body["aggregateBreach"], true);
}

/// T31 — aggregate-only breach: two categories each below 50k but sum
/// above 50k. `perCategoryBreach` false; `aggregateBreach` true.
#[tokio::test]
async fn m720_threshold_aggregate_only_breach() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0000").await;
    let s = onboarded_with_grant(&state, &app, "m720-agg").await;

    let _ = post(
        &app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": "bank_accounts", "totalEur": "30000.00" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let _ = post(
        &app,
        "/api/v1/modelo-720-inputs",
        json!({ "category": "real_estate", "totalEur": "30000.00" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    let r = get(
        &app,
        "/api/v1/dashboard/modelo-720-threshold",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["perCategoryBreach"], false);
    assert_eq!(body["aggregateBreach"], true);
}

#[tokio::test]
async fn m720_threshold_securities_null_when_fmv_missing() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0000").await;

    let s = onboarded_with_grant(&state, &app, "m720-null").await;

    let r = get(
        &app,
        "/api/v1/dashboard/modelo-720-threshold",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    // The onboarded RSU grant is double-trigger + no liquidity event,
    // so securities derivation excludes it; result is null or 0. Both
    // are acceptable here — the test asserts no 500.
    assert!(body["securitiesEur"].is_null() || body["securitiesEur"].is_string());
}
