//! Slice 3 T29 — vesting-event override + bulk-fmv integration tests.
//!
//! Covers:
//!   - OCC happy path (PUT succeeds + audit row written)
//!   - OCC stale → 409 `resource.stale_client_state`
//!   - `clearOverride` preserves FMV (AC-8.7.1)
//!   - bulk-fmv skips rows with existing FMV (AC-8.6.2)
//!   - grant-shrink-below-overrides → 422 `grant.share_count_below_overrides`

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use chrono::Utc;
use serde_json::json;

mod common;
use common::*;

/// Seed a Slice-1 RSU with the cliff already passed so past-row edits
/// are allowed.
async fn onboarded_with_past_rsu(
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
    let grant_id: uuid::Uuid =
        uuid::Uuid::parse_str(body["grant"]["id"].as_str().unwrap()).unwrap();
    (s, grant_id)
}

async fn list_vesting(app: &axum::Router, s: &Session, grant_id: &uuid::Uuid) -> serde_json::Value {
    let r = get(
        app,
        &format!("/api/v1/grants/{grant_id}/vesting"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    body
}

async fn fetch_event_row(
    _pool: &sqlx::PgPool,
    grant_id: uuid::Uuid,
    idx: i64,
) -> (uuid::Uuid, chrono::DateTime<chrono::Utc>) {
    // Routes through `DATABASE_URL_MIGRATE` when set: the `orbit_app`
    // role's RLS policy casts `current_setting('app.user_id')` to UUID,
    // which errors (22P02 `invalid input syntax for type uuid: ""`) when
    // the GUC has been left empty on a pooled connection after a prior
    // `Tx::for_user` commit. The migrate pool bypasses RLS; callers
    // that still pass `&state.pool` keep the signature for back-compat
    // but the actual query lands on the migrate pool.
    let pool = audit_pool().await;
    let row: (uuid::Uuid, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        r#"
        SELECT id, updated_at FROM vesting_events
        WHERE grant_id = $1
        ORDER BY vest_date ASC
        OFFSET $2 LIMIT 1
        "#,
    )
    .bind(grant_id)
    .bind(idx)
    .fetch_one(&pool)
    .await
    .expect("fetch event row");
    row
}

#[tokio::test]
async fn override_happy_path_writes_audit_with_allowlisted_payload() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-happy").await;

    // Confirm events exist.
    let v = list_vesting(&app, &s, &grant_id).await;
    assert!(v["vestingEvents"].as_array().unwrap().len() >= 3);

    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "42.8000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["isUserOverride"], true);

    let audit_pool = audit_pool().await;
    let n = audit_count(&audit_pool, s.user_id, "vesting_event.override").await;
    assert_eq!(n, 1);
    let payload = audit_last_payload(&audit_pool, s.user_id, "vesting_event.override").await;
    assert_eq!(payload["fields_changed"], json!(["fmv"]));
    assert!(payload["grant_id"].is_string());
    assert_no_forbidden_keys(&payload, "vesting_event.override");
}

#[tokio::test]
async fn override_occ_conflict_returns_409() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-occ").await;
    let (event_id, _updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    let stale_ts = Utc::now() - chrono::Duration::days(365);
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "99.0000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": stale_ts,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn bulk_fmv_skips_existing_rows() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-bulk").await;

    // First, override one event with a known FMV.
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;
    let _ = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "99.0000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    // Now bulk-fill — the overridden row should be skipped.
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
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["skippedCount"], 1);
    assert!(body["appliedCount"].as_u64().unwrap() >= 1);

    // Audit for bulk_fmv with allowlisted payload.
    let audit_pool = audit_pool().await;
    let payload = audit_last_payload(&audit_pool, s.user_id, "vesting_event.bulk_fmv").await;
    assert_eq!(payload["skipped_count"], 1);
    assert!(payload["applied_count"].is_number());
    assert_no_forbidden_keys(&payload, "vesting_event.bulk_fmv");
}

/// T31 — two simultaneous PUTs on the same row with the same
/// `expectedUpdatedAt` cookie; the second must 409.
///
/// Note: the error envelope currently does NOT ship the current
/// `updatedAt` value back to the client (`AppError::Conflict` has no
/// details). The UI-refresh story is "GET the row and retry".
/// Extending the envelope to include `currentUpdatedAt` is tracked as
/// a Slice-3 punt (see T31 report).
#[tokio::test]
async fn override_two_concurrent_puts_first_wins_second_409s() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-occ2").await;
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    // First PUT with the fresh token — succeeds.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "42.0000",
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

    // Second PUT with the SAME stale token → 409.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "fmvAtVest": "43.0000",
            "fmvCurrency": "USD",
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "resource.stale_client_state");
}

/// T33 / B1 — two simultaneous `clearOverride: true` PUTs with the
/// same `expectedUpdatedAt` cookie: the first reverts, the second
/// must 409. Mirrors `override_two_concurrent_puts_first_wins_second_409s`
/// for the clear-override path, closing the read-vs-UPDATE OCC gap
/// the T32 review flagged.
#[tokio::test]
async fn clear_override_two_concurrent_puts_first_wins_second_409s() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-clear-occ").await;
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    // Seed the row as overridden so the clear path has something to
    // revert. Use the freshest updated_at cookie for the first write.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "sharesVested": 500,
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
    let after_override_ua: String = row["updatedAt"].as_str().unwrap().into();

    // First clear — fresh token, succeeds.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "clearOverride": true,
            "expectedUpdatedAt": after_override_ua,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    // Second clear — SAME stale token → 409, not a silent re-clear.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "clearOverride": true,
            "expectedUpdatedAt": after_override_ua,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "resource.stale_client_state");
}

/// T31 — `clearOverride: true` on a row that has ONLY an FMV (no
/// prior shares/date edit) keeps `is_user_override = true` AND
/// preserves the FMV per AC-8.7.1 (c). We set up the row by first
/// applying an FMV-only override, then clearing.
#[tokio::test]
async fn clear_override_fmv_only_row_keeps_override_flag_and_fmv() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-clear-fmv").await;
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

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
    let (_s, _c, row) = body_json(r).await;
    let ua2: String = row["updatedAt"].as_str().unwrap().into();

    // Clear override — shares/vest_date revert, FMV stays, flag stays true.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "clearOverride": true,
            "expectedUpdatedAt": ua2,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, row) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{row}");
    // AC-8.7.1 (c): FMV preserved + override flag still true.
    assert_eq!(row["fmvAtVest"], "42.500000");
    assert_eq!(row["isUserOverride"], true);
}

/// T31 — `clearOverride: true` on a row that has shares+date edits and
/// NO FMV resets the flag per AC-8.7.1 (d).
#[tokio::test]
async fn clear_override_no_fmv_row_drops_override_flag() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-clear-nofmv").await;
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    // Override shares only (no FMV).
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
    let ua2: String = row["updatedAt"].as_str().unwrap().into();

    // Clear — flag + FMV both drop (FMV was already null).
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "clearOverride": true,
            "expectedUpdatedAt": ua2,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, row) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["isUserOverride"], false);
    assert!(row["fmvAtVest"].is_null());
}

/// T31 — `bulk-fmv` on a grant whose rows ALL already carry FMV
/// returns `appliedCount = 0` + `skippedCount = N`.
#[tokio::test]
async fn bulk_fmv_all_skipped_when_every_row_already_has_fmv() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-bulk-all-skip").await;

    // First fill every row.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/bulk-fmv"),
        json!({ "fmv": "30.0000", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    let applied1 = body["appliedCount"].as_u64().unwrap();
    assert!(applied1 >= 1);

    // Second call — every row already has FMV; applied=0.
    let r = post(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/bulk-fmv"),
        json!({ "fmv": "35.0000", "currency": "USD" }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["appliedCount"], 0);
    assert_eq!(body["skippedCount"], applied1);
}

/// T31 — AC-8.4.2 probe: a grant-param change must preserve
/// overridden vesting-event rows.
///
/// If this probe FAILS, the handler's `update` path is still passing
/// `&[]` to `derive_vesting_events` and `replace_for_grant` is
/// wiping overrides. T29 left this as a documented gap; T31 fixes it
/// in the same commit. After the fix the probe passes and we keep
/// it as the regression shield.
#[tokio::test]
async fn grant_update_preserves_user_override_rows() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-preserve").await;
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    // Override shares AND set an FMV on the first past event.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "sharesVested": 321,
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
    // Capture the overridden vest_date so we can relocate the row
    // after the grant update.
    let (_s, _c, ov) = body_json(r).await;
    let overridden_vest_date = ov["vestDate"].as_str().unwrap().to_string();

    // Update the grant — change vesting_total_months from 12 to 9 (and
    // share_count up to keep the shrink guard out of the way).
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        json!({
            "instrument": "rsu",
            "grantDate": "2024-01-15",
            "shareCount": 24000,
            "vestingStart": "2024-01-15",
            "vestingTotalMonths": 9,
            "cliffMonths": 0,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc."
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "update failed: {body}");

    // Re-fetch the vesting-events list and confirm the overridden row
    // is still present with its overridden FMV.
    let r = get(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting"),
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (_s, _c, body) = body_json(r).await;
    let events = body["vestingEvents"].as_array().unwrap();
    let preserved = events
        .iter()
        .find(|e| e["vestDate"].as_str() == Some(&overridden_vest_date));
    assert!(
        preserved.is_some(),
        "AC-8.4.2 regression: overridden row at {overridden_vest_date} was dropped \
         across grant-param change. events={events:?}",
    );
    let row = preserved.unwrap();
    assert_eq!(
        row["fmvAtVest"].as_str().map(str::to_string),
        Some("42.500000".to_string()),
        "overridden FMV dropped across grant-param change: row={row}",
    );
    assert_eq!(
        row["isUserOverride"], true,
        "override flag dropped across grant-param change: row={row}",
    );
}

/// T31 — happy-path companion to the 422 shrink probe: a grant-update
/// that preserves all overrides (new share_count >= sum(overrides))
/// succeeds.
#[tokio::test]
async fn grant_update_happy_path_with_overrides_preserves_sum() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-happy-update").await;
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;

    let _ = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "sharesVested": 500,
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;

    // Update share_count to 15,000 (>> 500) — must succeed.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        json!({
            "instrument": "rsu",
            "grantDate": "2024-01-15",
            "shareCount": 15000,
            "vestingStart": "2024-01-15",
            "vestingTotalMonths": 12,
            "cliffMonths": 0,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc."
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "update failed: {body}");
}

#[tokio::test]
async fn grant_shrink_below_overrides_returns_422() {
    let (state, app) = app().await;
    let (s, grant_id) = onboarded_with_past_rsu(&state, &app, "vev-shrink").await;

    // Override one row with 5000 shares (scaled: 50000000).
    let (event_id, updated_at) = fetch_event_row(&state.pool, grant_id, 0).await;
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}/vesting-events/{event_id}"),
        json!({
            "sharesVested": 5000,
            "expectedUpdatedAt": updated_at,
        }),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    // Attempt to shrink the grant's share_count below 5000 → 422.
    let r = put(
        &app,
        &format!("/api/v1/grants/{grant_id}"),
        json!({
            "instrument": "rsu",
            "grantDate": "2024-01-15",
            "shareCount": 100,
            "vestingStart": "2024-01-15",
            "vestingTotalMonths": 12,
            "cliffMonths": 0,
            "vestingCadence": "monthly",
            "doubleTrigger": false,
            "employerName": "ACME Inc."
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
        .any(|f| f["code"] == "grant.share_count_below_overrides"));
}
