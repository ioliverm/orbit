//! Slice 3 T31 — cross-tenant sweep across every Slice-3 endpoint.
//!
//! Mirrors `cross_tenant_slice_2.rs`: two users A and B each reach
//! `complete` with their own Slice-3 state (grant, per-grant override,
//! override on a past vest event). From B's session, every direct-
//! lookup-by-id path for A's ids must 404; every collection path must
//! omit A's rows.
//!
//! `/fx/rate` is public reference data (same output for any viewer) —
//! we confirm both users see the same payload, documenting that the
//! endpoint is intentionally NOT user-scoped.
//!
//! RLS scoping (ADR-014 §6): every SELECT runs under `orbit_app` with
//! `app.user_id` set by `Tx::for_user`. A row belonging to user A is
//! invisible to user B; the repo layer surfaces this as `Ok(None)` or
//! empty vec, which handlers translate to 404 or `{ items: [] }`.

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

/// Seed a Slice-3 power user: RSU grant, per-ticker price, per-grant
/// override, vest-event FMV override.
async fn seed_slice_3_user(
    state: &orbit_api::AppState,
    app: &axum::Router,
    tag: &str,
    employer: &str,
) -> (Session, uuid::Uuid, uuid::Uuid) {
    let s = onboarded(state, app, tag).await;
    let body = json!({
        "instrument": "rsu",
        "grantDate": "2024-01-15",
        "shareCount": 12000,
        "vestingStart": "2024-01-15",
        "vestingTotalMonths": 12,
        "cliffMonths": 0,
        "vestingCadence": "monthly",
        "doubleTrigger": false,
        "employerName": employer,
        "ticker": "ACME"
    });
    let r = post(
        app,
        "/api/v1/grants",
        body,
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CREATED, "create grant: {body}");
    let grant_id = uuid::Uuid::parse_str(body["grant"]["id"].as_str().unwrap()).unwrap();

    // Per-ticker current price (owned by this user).
    let _ = put(
        app,
        "/api/v1/current-prices/ACME",
        json!({ "price": "50.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    // Per-grant current-price override.
    let _ = put(
        app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        json!({ "price": "100.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    // Pull the first vesting-event id (past row) and override its shares.
    // Route through the RLS-bypass pool since raw selects on `vesting_events`
    // under `orbit_app` hit a `current_setting('app.user_id')::uuid` cast
    // error when no `Tx::for_user` is in scope.
    let migrate_pool = audit_pool().await;
    let first_event_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM vesting_events WHERE grant_id = $1 \
         ORDER BY vest_date ASC LIMIT 1",
    )
    .bind(grant_id)
    .fetch_one(&migrate_pool)
    .await
    .expect("first event");
    let updated_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT updated_at FROM vesting_events WHERE id = $1")
            .bind(first_event_id)
            .fetch_one(&migrate_pool)
            .await
            .expect("first event updated_at");

    let _ = put(
        app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{first_event_id}"),
        json!({
            "fmvAtVest": "40.00",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    (s, grant_id, first_event_id)
}

#[tokio::test]
async fn user_b_never_sees_user_a_slice_3_state_on_any_endpoint() {
    let (state, app) = app().await;
    wipe_fx(&state.pool).await;
    let today = Utc::now().date_naive();
    seed_fx(&state.pool, today, "0.9000").await;

    // Two fully-populated users.
    let (a, a_grant, a_event) = seed_slice_3_user(&state, &app, "xt3-a", "Alpha Inc.").await;
    let (b, b_grant, _b_event) = seed_slice_3_user(&state, &app, "xt3-b", "Bravo Corp.").await;

    // ---------- /fx/rate (public) — same output for anyone ----------
    // Explicitly confirms the FX endpoint is reference data: no auth
    // context, both A and B see identical payloads.
    let r_pub = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![],
    )
    .await;
    let (s_pub, _c, b_pub) = body_json(r_pub).await;
    assert_eq!(s_pub, StatusCode::OK);
    let r_a = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![(header::COOKIE.as_str(), a.cookie.clone())],
    )
    .await;
    let (_s, _c, b_a) = body_json(r_a).await;
    let r_b = get(
        &app,
        &format!("/api/v1/fx/rate?quote=USD&on={today}"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (_s, _c, b_b) = body_json(r_b).await;
    assert_eq!(
        b_pub, b_a,
        "/fx/rate is public reference data and must be identical anon vs A"
    );
    assert_eq!(
        b_a, b_b,
        "/fx/rate must yield the same payload for A and B (reference data)"
    );

    // ---------- Direct-lookup-by-id: A's ids → 404 from B ----------

    // /api/v1/grants/:A-grant/current-price-override
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_grant}/current-price-override"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "GET override cross-tenant"
    );

    let r = put(
        &app,
        &format!("/api/v1/grants/{a_grant}/current-price-override"),
        json!({ "price": "1.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "PUT override cross-tenant"
    );

    let r = delete(
        &app,
        &format!("/api/v1/grants/{a_grant}/current-price-override"),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "DELETE override cross-tenant"
    );

    // /api/v1/grants/:A-grant/vesting-events/:A-event (PUT)
    let stale_ts = Utc::now();
    let r = put(
        &app,
        &format!("/api/v1/grants/{a_grant}/vesting-events/{a_event}"),
        json!({
            "fmvAtVest": "99.00",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": stale_ts,
        }),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "PUT vesting-event cross-tenant"
    );

    // /api/v1/grants/:A-grant/vesting-events/bulk-fmv
    let r = post(
        &app,
        &format!("/api/v1/grants/{a_grant}/vesting-events/bulk-fmv"),
        json!({ "fmv": "1.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), b.cookie.clone()),
            ("x-csrf-token", b.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::NOT_FOUND,
        "POST bulk-fmv cross-tenant"
    );

    // ---------- Collection endpoints: A's data absent ----------

    // /api/v1/current-prices — B's list has ONLY B's tickers. The
    // per-ticker PUT is keyed by (user_id, ticker) so A's "ACME" row
    // is invisible to B's scope; if B also had ACME her row would
    // appear and A's row wouldn't.
    let r = get(
        &app,
        "/api/v1/current-prices",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let prices = body["prices"].as_array().unwrap();
    // Both users entered ACME, so B sees 1 row — hers. Verify the row
    // is NOT carrying A-specific metadata by checking ownership is
    // B's grant via the cross-tenant GET probe below.
    assert_eq!(prices.len(), 1, "B sees exactly her own ticker row: {body}");

    // /api/v1/dashboard/paper-gains — B sees her 1 grant, never A's.
    let r = get(
        &app,
        "/api/v1/dashboard/paper-gains",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    let per_grant = body["perGrant"].as_array().unwrap();
    for row in per_grant {
        assert_ne!(
            row["grantId"].as_str().unwrap(),
            a_grant.to_string(),
            "B sees A's grant in paper-gains: {body}",
        );
    }
    // B's grant id present.
    assert!(
        per_grant
            .iter()
            .any(|r| r["grantId"].as_str().unwrap() == b_grant.to_string()),
        "B's own grant missing from paper-gains: {body}",
    );

    // /api/v1/dashboard/modelo-720-threshold — returns B's M720 state
    // only. A's data does not show up even if the aggregate would have
    // breached under A's state.
    let r = get(
        &app,
        "/api/v1/dashboard/modelo-720-threshold",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    // B has no M720 inputs; bankAccountsEur is null / 0.
    assert!(
        body["bankAccountsEur"].is_null() || body["bankAccountsEur"] == "0.00",
        "B sees M720 data from nowhere: {body}"
    );

    // /api/v1/rule-set-chip — reference-data-ish; same shape regardless.
    let r = get(
        &app,
        "/api/v1/rule-set-chip",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["engineVersion"].is_string());

    // /api/v1/grants — B's list must not contain A's grant id.
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    let gids: Vec<String> = body["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !gids.contains(&a_grant.to_string()),
        "A's grant leaked into B's /grants list: {body}"
    );
    assert!(gids.contains(&b_grant.to_string()));

    // /api/v1/grants/:A-grant directly — 404.
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_grant}"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND, "GET grant cross-tenant");

    // /api/v1/grants/:A-grant/vesting list — RLS yields empty, handler
    // surfaces as 404 via the ownership check on the parent grant.
    let r = get(
        &app,
        &format!("/api/v1/grants/{a_grant}/vesting"),
        vec![(header::COOKIE.as_str(), b.cookie.clone())],
    )
    .await;
    // Either 404 (ownership check fails) or empty list. We accept both
    // as "isolation is preserved"; the assertion catches any response
    // that contains A's event ids.
    let (status, _c, body) = body_json(r).await;
    if status == StatusCode::OK {
        let events = body["vestingEvents"].as_array().unwrap();
        for ev in events {
            assert_ne!(
                ev["id"].as_str().unwrap(),
                a_event.to_string(),
                "A's vest event leaked into B's view: {body}"
            );
        }
    } else {
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
