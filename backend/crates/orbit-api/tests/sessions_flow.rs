//! Slice 2 T21 integration tests for the session list + revoke endpoints.
//!
//! Covers:
//!   - GET /auth/sessions returns only allowlisted fields (SEC-054)
//!   - DELETE on current session returns 403 `cannot_revoke_current`
//!     (AC-7.2.3)
//!   - DELETE on another session returns 204 + audit row with
//!     `{kind: "single", initiator: "self"}`
//!   - POST /auth/sessions/revoke-all-others preserves current, returns
//!     `{ revokedCount: N }`, audit payload `{kind, initiator, count}`
//!   - sessions list is reachable regardless of onboarding stage

#![cfg(feature = "integration-tests")]

use axum::http::{header, StatusCode};
use serde_json::json;

mod common;
use common::*;

async fn list_sessions_raw(app: &axum::Router, s: &Session) -> serde_json::Value {
    let r = get(
        app,
        "/api/v1/auth/sessions",
        vec![(header::COOKIE.as_str(), s.cookie.clone())],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK, "list_sessions: {body}");
    body
}

/// Issue a second session for the same user by signing in via a second
/// signup — since signin requires an existing password we just spin up a
/// second signup_verified session for the *same* user by re-using the
/// verify-email path and extracting cookies. Simpler: sign the same user
/// in from a fresh request by running `POST /auth/verify-email` against
/// a fresh verification token (works because /verify-email issues a
/// session), and the `sessions` INSERT lands a second row under the same
/// user_id.
async fn second_session_for(
    state: &orbit_api::AppState,
    app: &axum::Router,
    primary: &Session,
) -> Session {
    // Mint another verification token for the same user.
    let raw = uuid::Uuid::new_v4().to_string().replace('-', "");
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, raw.as_bytes());
    let hash: [u8; 32] = sha2::Digest::finalize(hasher).into();
    let mut tx = orbit_db::Tx::for_user(&state.pool, primary.user_id)
        .await
        .expect("tx");
    sqlx::query(
        "INSERT INTO email_verifications (user_id, token_hash, expires_at) \
         VALUES ($1, $2, now() + INTERVAL '24 hours')",
    )
    .bind(primary.user_id)
    .bind(&hash[..])
    .execute(tx.as_executor())
    .await
    .expect("insert");
    tx.commit().await.expect("commit");

    // verify-email issues a brand-new session for the user.
    let r = post(
        app,
        "/api/v1/auth/verify-email",
        json!({ "token": raw }),
        vec![],
    )
    .await;
    let (_st, cookies, _) = body_json(r).await;
    let sess = cookie_value(&cookies, "orbit_sess").unwrap();
    let csrf = cookie_value(&cookies, "orbit_csrf").unwrap();
    Session {
        user_id: primary.user_id,
        cookie: format!("orbit_sess={sess}; orbit_csrf={csrf}"),
        csrf,
    }
}

#[tokio::test]
async fn list_sessions_response_shape_omits_hashes() {
    let (state, app) = app().await;
    let s = signup_verified(&state, &app, "sessions-shape").await;
    let body = list_sessions_raw(&app, &s).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty());
    let row = &sessions[0];
    for forbidden in [
        "sessionIdHash",
        "session_id_hash",
        "refreshTokenHash",
        "refresh_token_hash",
        "ipHash",
        "ip_hash",
        "familyId",
        "family_id",
    ] {
        assert!(
            row.get(forbidden).is_none(),
            "leaked key {forbidden} in sessions row"
        );
    }
    assert_eq!(row["isCurrent"], true);
}

#[tokio::test]
async fn cannot_revoke_current_session_from_device_list() {
    let (state, app) = app().await;
    let s = signup_verified(&state, &app, "sessions-self").await;
    let body = list_sessions_raw(&app, &s).await;
    let current_id = body["sessions"][0]["id"].as_str().unwrap().to_string();

    let r = delete(
        &app,
        &format!("/api/v1/auth/sessions/{current_id}"),
        vec![
            (header::COOKIE.as_str(), s.cookie.clone()),
            ("x-csrf-token", s.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "cannot_revoke_current");
}

#[tokio::test]
async fn revoke_other_single_writes_audit_row() {
    let (state, app) = app().await;
    let primary = signup_verified(&state, &app, "sessions-other").await;
    let secondary = second_session_for(&state, &app, &primary).await;

    // List from primary should now see two sessions.
    let body = list_sessions_raw(&app, &primary).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    let target = sessions.iter().find(|r| r["isCurrent"] == false).unwrap();
    let target_id = target["id"].as_str().unwrap().to_string();

    // Primary revokes the other.
    let r = delete(
        &app,
        &format!("/api/v1/auth/sessions/{target_id}"),
        vec![
            (header::COOKIE.as_str(), primary.cookie.clone()),
            ("x-csrf-token", primary.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // List shrinks back to 1.
    let body = list_sessions_raw(&app, &primary).await;
    assert_eq!(body["sessions"].as_array().unwrap().len(), 1);

    // The secondary cookie is now invalid — listing should 401.
    let r = get(
        &app,
        "/api/v1/auth/sessions",
        vec![(header::COOKIE.as_str(), secondary.cookie.clone())],
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // Audit row shape.
    let pool = audit_pool().await;
    let payload = audit_last_payload(&pool, primary.user_id, "session.revoke").await;
    assert_eq!(payload["kind"], "single");
    assert_eq!(payload["initiator"], "self");
    let obj = payload.as_object().unwrap();
    assert_eq!(obj.len(), 2);

    // Double-revoke of the same target returns 404 (already revoked).
    let r = delete(
        &app,
        &format!("/api/v1/auth/sessions/{target_id}"),
        vec![
            (header::COOKIE.as_str(), primary.cookie.clone()),
            ("x-csrf-token", primary.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revoke_all_others_preserves_current_and_reports_count() {
    let (state, app) = app().await;
    let primary = signup_verified(&state, &app, "sessions-bulk").await;
    let _sec_a = second_session_for(&state, &app, &primary).await;
    let _sec_b = second_session_for(&state, &app, &primary).await;

    let body = list_sessions_raw(&app, &primary).await;
    assert_eq!(body["sessions"].as_array().unwrap().len(), 3);

    let r = post(
        &app,
        "/api/v1/auth/sessions/revoke-all-others",
        json!({}),
        vec![
            (header::COOKIE.as_str(), primary.cookie.clone()),
            ("x-csrf-token", primary.csrf.clone()),
        ],
    )
    .await;
    let (status, _c, body) = body_json(r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["revokedCount"], 2);

    // List shrinks to just current.
    let body = list_sessions_raw(&app, &primary).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["isCurrent"], true);

    // Audit row: bulk shape with count.
    let pool = audit_pool().await;
    let payload = audit_last_payload(&pool, primary.user_id, "session.revoke").await;
    assert_eq!(payload["kind"], "bulk");
    assert_eq!(payload["initiator"], "self");
    assert_eq!(payload["count"], 2);
    assert_eq!(payload.as_object().unwrap().len(), 3);
}

#[tokio::test]
async fn session_revoke_audit_payload_allowlist_is_strict() {
    // SEC-101 + T23: revoke-one + revoke-all-others each write exactly-
    // key-set payloads per ADR-016 §3. No raw IP, no hash values, no
    // country strings, no session id.
    let (state, app) = app().await;
    let primary = signup_verified(&state, &app, "s-allow").await;
    let _sec_a = second_session_for(&state, &app, &primary).await;
    let _sec_b = second_session_for(&state, &app, &primary).await;

    // Revoke one.
    let body = list_sessions_raw(&app, &primary).await;
    let target = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["isCurrent"] == false)
        .unwrap();
    let target_id = target["id"].as_str().unwrap().to_string();
    let r = delete(
        &app,
        &format!("/api/v1/auth/sessions/{target_id}"),
        vec![
            (header::COOKIE.as_str(), primary.cookie.clone()),
            ("x-csrf-token", primary.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // Revoke all others (the remaining non-current).
    let r = post(
        &app,
        "/api/v1/auth/sessions/revoke-all-others",
        json!({}),
        vec![
            (header::COOKIE.as_str(), primary.cookie.clone()),
            ("x-csrf-token", primary.csrf.clone()),
        ],
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);

    let pool = audit_pool().await;

    // Single-revoke payload: exactly {kind, initiator}.
    let rows: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT action, payload_summary FROM audit_log \
         WHERE user_id = $1 AND action = 'session.revoke' \
         ORDER BY occurred_at ASC",
    )
    .bind(primary.user_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "expected 2 session.revoke rows");

    // Row 0: single.
    let single = &rows[0].1;
    let obj = single.as_object().unwrap();
    assert_eq!(obj.len(), 2, "single payload should be exactly 2 keys");
    let got: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let want: std::collections::BTreeSet<&str> = ["kind", "initiator"].iter().copied().collect();
    assert_eq!(got, want, "session.revoke (single) key set");
    assert_eq!(single["kind"], "single");
    assert_eq!(single["initiator"], "self");
    assert_no_forbidden_keys(single, "session.revoke.single");

    // Row 1: bulk.
    let bulk = &rows[1].1;
    let obj = bulk.as_object().unwrap();
    assert_eq!(obj.len(), 3, "bulk payload should be exactly 3 keys");
    let got: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let want: std::collections::BTreeSet<&str> =
        ["kind", "initiator", "count"].iter().copied().collect();
    assert_eq!(got, want, "session.revoke (bulk) key set");
    assert_eq!(bulk["kind"], "bulk");
    assert_eq!(bulk["initiator"], "self");
    assert!(bulk["count"].is_number());
    assert_no_forbidden_keys(bulk, "session.revoke.bulk");

    // Cross-action sweep.
    let scanned = assert_all_audit_payloads_clean(&pool, primary.user_id, "session.").await;
    assert!(
        scanned >= 2,
        "expected ≥2 session.* rows for this user, got {scanned}"
    );
}

#[tokio::test]
async fn concurrent_revoke_same_target_writes_one_audit_row() {
    // T23 edge case: two concurrent DELETEs on the same session id race;
    // exactly one lands a `session.revoke` audit row. The other returns
    // 404 NotFound (sessions_mgmt::revoke_other already returns
    // RevokeOtherOutcome::NotFound on the loser — that branch does NOT
    // call audit::record_wizard).
    let (state, app) = app().await;
    let primary = signup_verified(&state, &app, "s-conc").await;
    let _secondary = second_session_for(&state, &app, &primary).await;

    let body = list_sessions_raw(&app, &primary).await;
    let target_id = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["isCurrent"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let pool = audit_pool().await;
    let baseline = audit_count(&pool, primary.user_id, "session.revoke").await;

    // Fire both DELETEs concurrently.
    let path = format!("/api/v1/auth/sessions/{target_id}");
    let hdrs = vec![
        (header::COOKIE.as_str(), primary.cookie.clone()),
        ("x-csrf-token", primary.csrf.clone()),
    ];
    let (r1, r2) = tokio::join!(
        delete(&app, &path, hdrs.clone()),
        delete(&app, &path, hdrs.clone()),
    );
    let statuses = [r1.status(), r2.status()];
    let ok_count = statuses
        .iter()
        .filter(|s| **s == StatusCode::NO_CONTENT)
        .count();
    let not_found_count = statuses
        .iter()
        .filter(|s| **s == StatusCode::NOT_FOUND)
        .count();
    assert_eq!(
        ok_count, 1,
        "exactly one concurrent revoke should succeed: {statuses:?}"
    );
    assert_eq!(
        not_found_count, 1,
        "the losing concurrent revoke should 404: {statuses:?}"
    );

    let after = audit_count(&pool, primary.user_id, "session.revoke").await;
    assert_eq!(
        after - baseline,
        1,
        "exactly one session.revoke audit row must land for the winning call"
    );
}
