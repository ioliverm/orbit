//! Slice 3 T29 — FX endpoint integration tests.
//!
//! Covers:
//!   - `GET /fx/rate` fresh, walkback, stale, unavailable
//!   - `GET /fx/latest`
//!   - Quote validation (400 on malformed `quote`)

#![cfg(feature = "integration-tests")]

use axum::http::StatusCode;
use chrono::{NaiveDate, Utc};
use sqlx::PgPool;

mod common;
use common::*;

async fn wipe_fx(pool: &PgPool) {
    sqlx::query("DELETE FROM fx_rates WHERE source = 'ecb'")
        .execute(pool)
        .await
        .expect("wipe fx_rates");
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
async fn fx_rate_fresh_returns_zero_walkback() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0823").await;

    let r = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["quote"], "USD");
    assert_eq!(body["walkback"], 0);
    assert_eq!(body["staleness"], "fresh");
}

#[tokio::test]
async fn fx_rate_walkback_one_to_two_days() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    let two_days_ago = today - chrono::Duration::days(2);
    seed_fx(&state.pool, two_days_ago, "1.0800").await;

    let r = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["walkback"], 2);
    assert_eq!(body["staleness"], "walkback");
}

#[tokio::test]
async fn fx_rate_unavailable_when_gap_exceeds_seven_days() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    // Seed way outside the walkback window.
    seed_fx(&state.pool, today - chrono::Duration::days(14), "1.0700").await;

    let r = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["staleness"], "unavailable");
    assert!(body["rate"].is_null());
}

#[tokio::test]
async fn fx_latest_returns_most_recent() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "1.0823").await;
    seed_fx(&state.pool, today - chrono::Duration::days(1), "1.0800").await;

    let r = get(&app, "/api/v1/fx/latest?quote=USD", vec![]).await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rateDate"], today.format("%Y-%m-%d").to_string());
    assert_eq!(body["rate"], "1.0823");
}

#[tokio::test]
async fn fx_rate_rejects_invalid_quote() {
    let (_state, app) = app().await;
    let r = get(&app, "/api/v1/fx/rate?quote=xx", vec![]).await;
    let (status, _c, _body) = body_json(r).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// T31 — 7-day walkback should find an intermediate rate when some of
/// the intervening dates are missing. Seeds a rate 7 days old with
/// nothing in between; expects `walkback = 7` and `staleness = "walkback"`.
#[tokio::test]
async fn fx_rate_lookup_walkback_returns_7_day_old_rate_when_intervening_dates_missing() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    let seven_days_ago = today - chrono::Duration::days(7);
    seed_fx(&state.pool, seven_days_ago, "1.0700").await;

    let r = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["walkback"], 7);
    assert_eq!(body["staleness"], "walkback");
    assert_eq!(body["rate"], "1.0700");
}

/// T31 — beyond the 7-day window returns unavailable (mirrors the
/// existing 14-day case but pins the 8-day boundary).
#[tokio::test]
async fn fx_rate_lookup_beyond_7_days_returns_unavailable() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today - chrono::Duration::days(8), "1.0600").await;

    let r = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["staleness"], "unavailable");
    assert!(body["rate"].is_null());
}

/// T31 — `/fx/latest` with an empty `fx_rates` returns a null rate.
#[tokio::test]
async fn fx_latest_returns_null_when_fx_rates_empty() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let r = get(&app, "/api/v1/fx/latest?quote=USD", vec![]).await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["rate"].is_null() || body["rate"] == serde_json::Value::Null);
}
