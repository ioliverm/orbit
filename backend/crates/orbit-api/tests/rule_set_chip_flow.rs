//! Slice 3 T29 — rule-set chip endpoint integration tests.

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use chrono::{NaiveDate, Utc};
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
async fn chip_returns_fx_date_and_engine_version() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0823").await;
    let s = onboarded_with_grant(&state, &app, "chip-ok").await;

    let r = get(
        &app,
        "/api/v1/rule-set-chip",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["fxDate"], today.format("%Y-%m-%d").to_string());
    assert!(body["engineVersion"].is_string());
}

#[tokio::test]
async fn chip_fx_date_null_when_fx_empty() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let s = onboarded_with_grant(&state, &app, "chip-empty").await;

    let r = get(
        &app,
        "/api/v1/rule-set-chip",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["fxDate"].is_null());
    // engine_version is always populated, regardless of FX.
    assert!(body["engineVersion"].is_string());
}

/// T31 — the chip's `fxDate` is the latest `rate_date` in
/// `fx_rates` for the user's primary currency (EUR → USD pair).
/// Multiple seeded dates; assert the chip picks the newest.
#[tokio::test]
async fn chip_fx_date_matches_latest_rate_date_for_primary_currency() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today - chrono::Duration::days(3), "1.0700").await;
    seed_fx(&state.pool, today - chrono::Duration::days(1), "1.0750").await;
    seed_fx(&state.pool, today, "1.0820").await;
    let s = onboarded_with_grant(&state, &app, "chip-latest").await;

    let r = get(
        &app,
        "/api/v1/rule-set-chip",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["fxDate"], today.format("%Y-%m-%d").to_string());
}
