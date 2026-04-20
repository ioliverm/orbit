//! Session-management repository helpers (Slice 2 T20).
//!
//! The Slice-1 `sessions` table backs the auth middleware's cookie-lookup
//! path (`middleware::session::lookup_session_by_hash`) — those queries are
//! allowed to read the hash columns. This module is the **UI-facing**
//! surface (AC-7.1..AC-7.2): listing the caller's active sessions and
//! revoking one or all others. The list shape deliberately omits
//! `session_id_hash` and `refresh_token_hash` so they can never leak through
//! a JSON response body (AC-7.1.3, SEC-054).
//!
//! Every query routes through a `&mut Tx` borrowed via
//! [`crate::Tx::for_user`]; RLS scopes the visible row set to the owner.
//!
//! Traces to:
//!   - ADR-016 §1 (sessions.country_iso2 additive column).
//!   - ADR-016 §3 (GET /auth/sessions, DELETE /auth/sessions/:id,
//!     POST /auth/sessions/revoke-all-others).
//!   - docs/requirements/slice-2-acceptance-criteria.md §7.

use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// A session row as surfaced by the Slice-2 list endpoint.
///
/// Deliberately **does not** carry `session_id_hash`, `refresh_token_hash`,
/// `ip_hash`, or `family_id`. Those columns are for the middleware's
/// cookie-lookup path only; the UI has no use for them and serializing
/// them would violate G-29 (log/response redaction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionForListing {
    pub id: Uuid,
    pub user_id: Uuid,
    pub user_agent: String,
    /// ISO 3166-1 alpha-2 code derived at session-creation time, or `None`
    /// when the GeoIP lookup failed / had not yet run (Slice-1 rows).
    pub country_iso2: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: chrono::DateTime<chrono::Utc>,
}

/// Outcome of [`revoke_other`] — the handler maps these to HTTP codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevokeOtherOutcome {
    /// 204: the target session was non-current and got `revoked_at = now()`.
    Revoked,
    /// 403: the target session id is the caller's own current session. Per
    /// AC-7.2.3 this UI path is not allowed to end the current session —
    /// users must sign out via the user-menu signout flow.
    CannotRevokeCurrent,
    /// 404: the target session does not exist, belongs to another user
    /// (filtered by RLS), or was already revoked / expired (AC-10.6).
    NotFound,
}

/// List the caller's active (`revoked_at IS NULL`) sessions, newest-used
/// first. The ordering is stable across refreshes (AC-7.1.2).
pub async fn list_for_user(
    tx: &mut Tx<'_>,
    user_id: Uuid,
) -> Result<Vec<SessionForListing>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, user_id, user_agent, country_iso2, created_at, last_used_at
          FROM sessions
         WHERE user_id = $1 AND revoked_at IS NULL
         ORDER BY last_used_at DESC, created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_session).collect()
}

/// Revoke a single non-current session.
///
/// Returns [`RevokeOtherOutcome::CannotRevokeCurrent`] when `target_id ==
/// current_session_id` — the caller must not use this helper to end its
/// own session (AC-7.2.3). The check is a defense-in-depth mirror of the
/// UI guard: the UI disables the button on the current row, but a crafted
/// request still lands here and must be rejected with 403 rather than
/// accepted.
///
/// RLS scopes the UPDATE to the caller's rows; cross-tenant `target_id`
/// matches zero rows and returns [`RevokeOtherOutcome::NotFound`], which
/// the handler surfaces as 404 (not 403) per AC-10.3. An already-revoked
/// row is likewise `NotFound` because the WHERE includes `revoked_at IS
/// NULL` (AC-10.6 stale-tab handling).
pub async fn revoke_other(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    target_id: Uuid,
    current_session_id: Uuid,
) -> Result<RevokeOtherOutcome, sqlx::Error> {
    if target_id == current_session_id {
        return Ok(RevokeOtherOutcome::CannotRevokeCurrent);
    }

    let result = sqlx::query(
        r#"
        UPDATE sessions
           SET revoked_at    = now(),
               revoke_reason = 'admin'
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(target_id)
    .bind(user_id)
    .execute(tx.as_executor())
    .await?;

    if result.rows_affected() == 0 {
        Ok(RevokeOtherOutcome::NotFound)
    } else {
        Ok(RevokeOtherOutcome::Revoked)
    }
}

/// Bulk-revoke every active session for `user_id` **except** the current
/// one. Returns the number of rows actually revoked — the value surfaces
/// verbatim in the response body `{ revokedCount }` and in the audit-log
/// `payload_summary.count` per ADR-016 §3. Using `::bigint` then narrowing
/// here keeps the return type convenient for the handler (`usize`) even on
/// 32-bit targets.
pub async fn revoke_all_others(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    current_session_id: Uuid,
) -> Result<usize, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE sessions
           SET revoked_at    = now(),
               revoke_reason = 'admin'
         WHERE user_id = $1
           AND id <> $2
           AND revoked_at IS NULL
        "#,
    )
    .bind(user_id)
    .bind(current_session_id)
    .execute(tx.as_executor())
    .await?;

    // `rows_affected()` returns u64; usize is fine on all supported targets
    // (revoking more than usize::MAX sessions is not a concern).
    Ok(result.rows_affected() as usize)
}

fn row_to_session(row: &sqlx::postgres::PgRow) -> Result<SessionForListing, sqlx::Error> {
    Ok(SessionForListing {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        user_agent: row.try_get("user_agent")?,
        country_iso2: row.try_get("country_iso2")?,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
    })
}
