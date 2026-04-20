//! Session lookup middleware.
//!
//! Reads the `orbit_sess` cookie, hashes it with SHA-256, and looks up a
//! non-revoked `sessions` row via the raw pool (the session table is keyed
//! by `session_id_hash` — the owning user is the value being resolved, so
//! the lookup itself cannot go through `Tx::for_user` yet). Once resolved,
//! the user id is stashed into request extensions as [`SessionAuth`] and
//! every handler that needs a tx fetches `Tx::for_user(state.pool,
//! auth.user_id)` from there.
//!
//! The session lookup uses `sessions.session_id_hash` (a UNIQUE index), so
//! the single row is either the caller's or nothing. RLS is bypassed for
//! this one read by design: `session_id_hash` is the credential and we do
//! not know the user id yet. The query scope is tightly bounded — exact
//! match on a 32-byte hash — and audited in the integration tests.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::CookieJar;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

/// Authenticated caller, inserted into request extensions by the session
/// middleware for downstream handlers to consume.
#[derive(Debug, Clone, Copy)]
pub struct SessionAuth {
    pub user_id: Uuid,
    pub session_id: Uuid,
}

/// Require a valid `orbit_sess` cookie. Returns 401 `unauthenticated` if
/// the cookie is absent, unparsable, or resolves to a revoked / expired
/// session row.
pub async fn require(
    State(state): State<AppState>,
    jar: CookieJar,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let auth = resolve(&state, &jar).await?;
    req.extensions_mut().insert(auth);
    Ok(next.run(req).await)
}

/// Same as [`require`] but never errors — handlers that want optional auth
/// (e.g. `/auth/me` from a tab that might be signed out) would use this.
/// Not wired in T13a but kept for T13b's wizard-gate middleware.
#[allow(dead_code)]
pub async fn optional(
    State(state): State<AppState>,
    jar: CookieJar,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if let Ok(auth) = resolve(&state, &jar).await {
        req.extensions_mut().insert(auth);
    }
    next.run(req).await
}

async fn resolve(state: &AppState, jar: &CookieJar) -> Result<SessionAuth, AppError> {
    let encoded = jar
        .get(orbit_auth::session::SESSION_COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or(AppError::Unauthenticated)?;

    // `orbit_auth::session::new_session_token` stores
    // `sha256(<raw 32 bytes>)`, where the cookie carries the base64url
    // encoding of those same 32 bytes. Decode before hashing so we match
    // the stored `session_id_hash`.
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    let raw = URL_SAFE_NO_PAD
        .decode(encoded.as_bytes())
        .map_err(|_| AppError::Unauthenticated)?;
    let hash: [u8; 32] = Sha256::digest(&raw).into();

    // Session lookup: the owning user_id is what we are trying to
    // discover, so this read cannot go through `Tx::for_user`. The
    // migration `20260502120000_t13a_session_lookup.sql` ships a
    // SECURITY DEFINER helper so orbit_app can resolve the single row
    // keyed by the cookie hash without bypassing RLS on the table itself.
    let row =
        sqlx::query("SELECT id, user_id, created_at, revoked_at FROM lookup_session_by_hash($1)")
            .bind(&hash[..])
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| AppError::Internal)?;

    let row = row.ok_or(AppError::Unauthenticated)?;
    let session_id: Uuid = row.try_get("id").map_err(|_| AppError::Internal)?;
    let user_id: Uuid = row.try_get("user_id").map_err(|_| AppError::Internal)?;
    let created_at: chrono::DateTime<chrono::Utc> =
        row.try_get("created_at").map_err(|_| AppError::Internal)?;
    let revoked_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("revoked_at").map_err(|_| AppError::Internal)?;

    // Enforce the 30-min session TTL and revoked-at gate in application
    // code since the function returns the raw row.
    if revoked_at.is_some() {
        return Err(AppError::Unauthenticated);
    }
    if chrono::Utc::now() - created_at > chrono::Duration::minutes(30) {
        return Err(AppError::Unauthenticated);
    }

    // Touch last_used_at so the Active-Sessions UI (Slice 2+) reflects the
    // real recency. Best-effort: now that we know `user_id`, route through
    // Tx::for_user so the UPDATE participates in RLS scoping.
    if let Ok(mut tx) = orbit_db::Tx::for_user(&state.pool, user_id).await {
        let _ = sqlx::query("UPDATE sessions SET last_used_at = now() WHERE id = $1")
            .bind(session_id)
            .execute(tx.as_executor())
            .await;
        let _ = tx.commit().await;
    }

    Ok(SessionAuth {
        user_id,
        session_id,
    })
}
