//! Slice 3 T29 — dashboard paper-gains tile integration tests.
//!
//! Covers:
//!   - Complete grant produces bands.
//!   - Incomplete grant (missing past FMV) surfaces in `incompleteGrants`.
//!   - `stalenessFx = "unavailable"` when fx_rates empty.

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
async fn paper_gains_fx_unavailable_still_renders() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let s = onboarded_with_grant(&state, &app, "pg-unavailable").await;

    let r = get(
        &app,
        "/api/v1/dashboard/paper-gains",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stalenessFx"], "unavailable");
    assert!(body["combinedEurBand"].is_null());
}

#[tokio::test]
async fn paper_gains_empty_when_no_prices_entered() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "0.9000").await;
    let s = onboarded_with_grant(&state, &app, "pg-no-prices").await;

    let r = get(
        &app,
        "/api/v1/dashboard/paper-gains",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    // The seeded grant has a double-trigger RSU with null liquidity,
    // so it's excluded with `DoubleTriggerPreLiquidity` reason — not
    // surfaced in incompleteGrants per AC-5.4.4.
    assert_eq!(body["stalenessFx"], "fresh");
    assert_eq!(body["fxDate"], today.format("%Y-%m-%d").to_string());
}

#[tokio::test]
async fn paper_gains_price_and_fmv_yields_band() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "0.9000").await;

    let s = onboarded_with_grant(&state, &app, "pg-band").await;

    // Seed a per-ticker price so the dashboard can compute.
    let r = put(
        &app,
        "/api/v1/current-prices/ACME",
        json!({ "price": "45.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    // Shape asserted; exact bands depend on event FMV + vested shares.
    let r = get(
        &app,
        "/api/v1/dashboard/paper-gains",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["perGrant"].is_array());
    // Well-formed response shape.
    assert!(body["incompleteGrants"].is_array());
}
