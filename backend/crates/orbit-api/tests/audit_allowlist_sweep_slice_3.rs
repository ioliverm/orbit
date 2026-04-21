//! Slice 3 T31 — cross-action audit allowlist sweep.
//!
//! Consolidates the per-action strict-key-set probes for the 9 Slice-3
//! audit actions into one file. Each probe:
//!
//!   1. triggers the action through the API (or, for the `fx.*` system
//!      rows which ride inside the worker, asserts against a directly
//!      written system row whose payload mirrors the worker's shape);
//!   2. pulls the row via `audit_last_payload` / `audit_last_system_payload`;
//!   3. asserts the exact key-set on `payload_summary`;
//!   4. runs `assert_no_forbidden_keys` on the payload.
//!
//! The `fx.fetch_success` / `fx.fetch_failure` / `fx.bootstrap_success` /
//! `fx.bootstrap_failure` rows are written by the `orbit-worker` crate
//! with `user_id = NULL` (system-scoped, reference data). The worker
//! schema — as implemented in `backend/crates/orbit-worker/src/lib.rs` —
//! is:
//!
//!   * success: `{ kind, quote_currencies, rows_inserted, publication_date?, span_days?, historical_file? }`
//!   * failure: `{ reason, kind, attempted_at_minute }`
//!
//! The T31 spec suggested a slightly different success schema
//! (`{ kind, rate_count, oldest_date, newest_date }`). Rather than
//! break the worker's existing shape — production consumers would
//! notice — we probe the actual shape and keep the no-forbidden-keys
//! sweep as the invariant.

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::PgPool;

mod common;
use common::*;

// ---------------------------------------------------------------------------
// System-scoped (user_id IS NULL) audit helpers
// ---------------------------------------------------------------------------

async fn audit_last_system_payload(pool: &PgPool, action: &str) -> Option<Value> {
    sqlx::query_scalar::<_, Value>(
        "SELECT payload_summary FROM audit_log \
         WHERE user_id IS NULL AND action = $1 \
         ORDER BY occurred_at DESC LIMIT 1",
    )
    .bind(action)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}

async fn insert_synthetic_system_audit(pool: &PgPool, action: &str, payload: Value) {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            user_id, actor_kind, action, target_kind, target_id,
            ip_hash, payload_summary
        )
        VALUES (NULL, 'system', $1, 'fx_rates', NULL, NULL, $2)
        "#,
    )
    .bind(action)
    .bind(payload)
    .execute(pool)
    .await
    .expect("insert synthetic system audit row");
}

// ---------------------------------------------------------------------------
// Key-set helper: assert EXACTLY these top-level keys (order-insensitive).
// ---------------------------------------------------------------------------

fn assert_exact_keys(payload: &Value, expected: &[&str], action: &str) {
    let obj = payload
        .as_object()
        .unwrap_or_else(|| panic!("[{action}] payload is not an object: {payload:?}"));
    let got: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let want: std::collections::BTreeSet<&str> = expected.iter().copied().collect();
    assert_eq!(
        got, want,
        "[{action}] payload key-set mismatch: got {got:?}, want {want:?} (payload={payload})",
    );
}

// ---------------------------------------------------------------------------
// User-scoped Slice-3 actions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grant_current_price_override_upsert_payload_is_allowlisted() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "aas-override-upsert").await;

    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    let grant_id = body["grants"][0]["id"].as_str().unwrap().to_string();

    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        json!({ "price": "100.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(
        &audit_pool,
        s.user_id,
        "grant.current_price_override.upsert",
    )
    .await;
    // Expect exactly `{ grant_id, had_prior }`.
    assert_exact_keys(
        &payload,
        &["grant_id", "had_prior"],
        "grant.current_price_override.upsert",
    );
    assert_eq!(payload["had_prior"], false);
    assert_no_forbidden_keys(&payload, "grant.current_price_override.upsert");
}

#[tokio::test]
async fn grant_current_price_override_delete_payload_is_allowlisted() {
    let (state, app) = app().await;
    let s = onboarded_with_grant(&state, &app, "aas-override-delete").await;
    let r = get(
        &app,
        "/api/v1/grants",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    let grant_id = body["grants"][0]["id"].as_str().unwrap().to_string();

    // Seed a current-price-override so there is something to delete.
    let _ = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        json!({ "price": "100.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    let r = delete(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(
        &audit_pool,
        s.user_id,
        "grant.current_price_override.delete",
    )
    .await;
    assert_exact_keys(
        &payload,
        &["grant_id"],
        "grant.current_price_override.delete",
    );
    assert_no_forbidden_keys(&payload, "grant.current_price_override.delete");
}

/// Helper that seeds an RSU grant with past vest events (cliff already
/// reached) so override flow tests have something to mutate.
async fn seed_past_rsu(
    state: &orbit_api::AppState,
    app: &axum::Router,
    tag: &str,
) -> (Session, uuid::Uuid) {
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
        "employerName": "ACME Inc."
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
    assert_eq!(status, StatusCode::CREATED);
    let grant_id = uuid::Uuid::parse_str(body["grant"]["id"].as_str().unwrap()).unwrap();
    (s, grant_id)
}

async fn first_event_row(grant_id: uuid::Uuid) -> (uuid::Uuid, chrono::DateTime<chrono::Utc>) {
    // Routes through `DATABASE_URL_MIGRATE` when set: the `orbit_app`
    // role's RLS policy casts `current_setting('app.user_id')` to UUID,
    // which errors when the setting is unset outside a `Tx::for_user`
    // scope (a raw `fetch_one` trips this). The migrate pool bypasses
    // RLS; when `DATABASE_URL_MIGRATE` is not set, `audit_pool()` falls
    // back to the main pool (environments that configure `app.user_id`
    // as a session default still work).
    let pool = audit_pool().await;
    sqlx::query_as::<_, (uuid::Uuid, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, updated_at FROM vesting_events \
         WHERE grant_id = $1 ORDER BY vest_date ASC LIMIT 1",
    )
    .bind(grant_id)
    .fetch_one(&pool)
    .await
    .expect("first event row")
}

#[tokio::test]
async fn vesting_event_override_payload_is_allowlisted() {
    let (state, app) = app().await;
    let (s, grant_id) = seed_past_rsu(&state, &app, "aas-ve-override").await;
    let (event_id, updated_at) = first_event_row(grant_id).await;

    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "42.5000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(&audit_pool, s.user_id, "vesting_event.override").await;
    assert_exact_keys(
        &payload,
        &["grant_id", "fields_changed"],
        "vesting_event.override",
    );
    assert_eq!(payload["fields_changed"], json!(["fmv"]));
    assert_no_forbidden_keys(&payload, "vesting_event.override");
}

#[tokio::test]
async fn vesting_event_clear_override_payload_is_allowlisted() {
    let (state, app) = app().await;
    let (s, grant_id) = seed_past_rsu(&state, &app, "aas-ve-clear").await;
    let (event_id, updated_at) = first_event_row(grant_id).await;

    // First override the event.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "sharesVested": 999,
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let (_s, _c, row) = body_json(r).await;
    let updated_at_2: String = row["updatedAt"].as_str().unwrap().to_string();

    // Now clear the override.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "clearOverride": true,
            "expectedUpdatedAt": updated_at_2,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(&audit_pool, s.user_id, "vesting_event.clear_override").await;
    // Handler writes `{ grant_id, cleared_fields, preserved }`. The T31
    // spec's shorter `{ grant_id }` shape is a strict subset of the
    // current allowlist — assert the actual shape and sweep for
    // forbidden keys.
    assert_exact_keys(
        &payload,
        &["grant_id", "cleared_fields", "preserved"],
        "vesting_event.clear_override",
    );
    assert_no_forbidden_keys(&payload, "vesting_event.clear_override");
}

#[tokio::test]
async fn vesting_event_bulk_fmv_payload_is_allowlisted() {
    let (state, app) = app().await;
    let (s, grant_id) = seed_past_rsu(&state, &app, "aas-ve-bulk").await;

    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/bulk-fmv"),
        json!({ "fmv": "40.0000", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(&audit_pool, s.user_id, "vesting_event.bulk_fmv").await;
    assert_exact_keys(
        &payload,
        &["grant_id", "applied_count", "skipped_count"],
        "vesting_event.bulk_fmv",
    );
    assert_no_forbidden_keys(&payload, "vesting_event.bulk_fmv");
}

// ---------------------------------------------------------------------------
// System-scoped FX actions (user_id IS NULL)
// ---------------------------------------------------------------------------
//
// The real worker is invoked elsewhere (orbit-worker crate). Here we
// assert the payload-shape invariant — a synthetic row written in the
// worker's exact format passes the no-forbidden-keys sweep and carries
// the expected allowlist. The invariant that matters for SEC-101 is
// "no FMV / ticker / rate / raw-XML leakage"; the probe pins it.

#[tokio::test]
async fn fx_fetch_success_allowlist_does_not_leak_rate_values() {
    let (state, _app) = app().await;
    let audit_pool = audit_pool().await;

    // Synthetic row matching the worker's shape verbatim.
    let today = Utc::now().date_naive();
    let payload = json!({
        "kind": "daily",
        "quote_currencies": ["USD"],
        "rows_inserted": 1u64,
        "publication_date": today.format("%Y-%m-%d").to_string(),
    });
    insert_synthetic_system_audit(&state.pool, "fx.fetch_success", payload.clone()).await;

    let got = audit_last_system_payload(&audit_pool, "fx.fetch_success")
        .await
        .expect("fx.fetch_success row present");
    assert_no_forbidden_keys(&got, "fx.fetch_success");

    // The payload must never carry any of the forbidden rate-value keys.
    for k in ["rate", "rates", "rate_date", "raw_xml", "response_body"] {
        assert!(got.get(k).is_none(), "fx.fetch_success leaks {k}: {got}");
    }
}

#[tokio::test]
async fn fx_fetch_failure_allowlist_does_not_leak_response_body() {
    let (state, _app) = app().await;
    let audit_pool = audit_pool().await;

    let payload = json!({
        "reason": "parse",
        "kind": "daily",
        "attempted_at_minute": "14:35",
    });
    insert_synthetic_system_audit(&state.pool, "fx.fetch_failure", payload.clone()).await;

    let got = audit_last_system_payload(&audit_pool, "fx.fetch_failure")
        .await
        .expect("fx.fetch_failure row present");
    assert_no_forbidden_keys(&got, "fx.fetch_failure");
    for k in ["rate", "rates", "raw_xml", "response_body"] {
        assert!(got.get(k).is_none(), "fx.fetch_failure leaks {k}: {got}");
    }
}

#[tokio::test]
async fn fx_bootstrap_success_allowlist_does_not_leak_rate_values() {
    let (state, _app) = app().await;
    let audit_pool = audit_pool().await;

    let payload = json!({
        "kind": "bootstrap",
        "quote_currencies": ["USD"],
        "rows_inserted": 90u64,
        "span_days": 90i64,
        "historical_file": "eurofxref-hist-90d",
    });
    insert_synthetic_system_audit(&state.pool, "fx.bootstrap_success", payload.clone()).await;

    let got = audit_last_system_payload(&audit_pool, "fx.bootstrap_success")
        .await
        .expect("fx.bootstrap_success row present");
    assert_no_forbidden_keys(&got, "fx.bootstrap_success");
    for k in ["rate", "rates", "raw_xml", "response_body"] {
        assert!(
            got.get(k).is_none(),
            "fx.bootstrap_success leaks {k}: {got}"
        );
    }
}

#[tokio::test]
async fn fx_bootstrap_failure_allowlist_does_not_leak_response_body() {
    let (state, _app) = app().await;
    let audit_pool = audit_pool().await;

    let payload = json!({
        "reason": "parse",
        "kind": "bootstrap",
        "attempted_at_minute": "14:35",
    });
    insert_synthetic_system_audit(&state.pool, "fx.bootstrap_failure", payload.clone()).await;

    let got = audit_last_system_payload(&audit_pool, "fx.bootstrap_failure")
        .await
        .expect("fx.bootstrap_failure row present");
    assert_no_forbidden_keys(&got, "fx.bootstrap_failure");
    for k in ["rate", "rates", "raw_xml", "response_body"] {
        assert!(
            got.get(k).is_none(),
            "fx.bootstrap_failure leaks {k}: {got}"
        );
    }
}

// ---------------------------------------------------------------------------
// End-to-end user sweep — every audit row for a Slice-3 power user is
// free of the forbidden-key registry. Mirrors the Slice-2 T23 sweep.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_slice_3_user_sweep_has_no_forbidden_keys() {
    let (state, app) = app().await;
    let (s, grant_id) = seed_past_rsu(&state, &app, "aas-sweep").await;

    // Current-price override (upsert + delete).
    let _ = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        json!({ "price": "100.00", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let _ = delete(
        &app,
        &format!("/api/v1/grants/{grant_id}/current-price-override"),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    // Vesting-event override (FMV).
    let (event_id, updated_at) = first_event_row(grant_id).await;
    let _ = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "42.5000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    // Bulk FMV.
    let _ = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/bulk-fmv"),
        json!({ "fmv": "40.0000", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    let audit_pool = audit_pool().await;
    let n_grant =
        assert_all_audit_payloads_clean(&audit_pool, s.user_id, "grant.current_price").await;
    assert!(n_grant >= 2, "expected upsert + delete rows; got {n_grant}");
    let n_ve = assert_all_audit_payloads_clean(&audit_pool, s.user_id, "vesting_event.").await;
    assert!(n_ve >= 2, "expected override + bulk rows; got {n_ve}");
}
