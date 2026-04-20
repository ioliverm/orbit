//! Auth endpoints (ADR-011 §Flows, SEC-003..SEC-010).
//!
//! Scope T13a:
//!   - `POST /auth/signup`
//!   - `POST /auth/verify-email`
//!   - `POST /auth/signin`
//!   - `POST /auth/signout`
//!   - `GET  /auth/me`
//!   - `POST /auth/mfa/*` → 501
//!
//! Not in scope (land in T13b):
//!   - `POST /consent/disclaimer`, `/residency/*`, `/grants/*`, `/auth/me`
//!     extended onboarding stage resolution.

use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use cookie::{time::Duration as CookieDuration, Cookie, SameSite};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::net::SocketAddr;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{self, AuthAction};
use crate::error::{AppError, FieldError};
use crate::hibp::{self, HibpCheck};
use crate::middleware::rate_limit::{self, Decision, Limiter};
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Constants (ADR-011)
// ---------------------------------------------------------------------------

const EMAIL_VERIFY_TTL_HOURS: i64 = 24;
const SESSION_TTL_SECS: i64 = 1800;
const REFRESH_TTL_SECS: i64 = 604_800;

const CSRF_COOKIE: &str = "orbit_csrf";
const REFRESH_COOKIE: &str = "orbit_refresh";

const SIGNIN_CAPTCHA_FAILURE_THRESHOLD: i64 = 3;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SignupBody {
    #[validate(email, length(max = 254))]
    pub email: String,
    #[validate(length(min = 12, max = 200))]
    pub password: String,
    #[serde(default)]
    pub locale_hint: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SigninBody {
    #[validate(email, length(max = 254))]
    pub email: String,
    #[validate(length(min = 1, max = 200))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct VerifyEmailBody {
    #[validate(length(min = 1, max = 128))]
    pub token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub user: MeUser,
    pub residency: Option<Value>,
    pub onboarding_stage: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeUser {
    pub id: Uuid,
    pub email: String,
    pub locale: String,
    pub primary_currency: String,
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// `POST /api/v1/auth/signup` — create a user + email verification row.
/// Always responds 201 (SEC-003 no-enumeration). Emails are not sent in
/// T13a; the verification link is logged to stdout via `orbit_log::event!`
/// so local dev can copy it.
pub async fn signup(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<SignupBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;

    // Rate limits per SEC-160: 5/IP/hour + 3/email/hour.
    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    let ip_key = format!("signup:ip:{}", hex_or_unknown(ip_hash.as_ref()));
    let email_key = format!(
        "signup:email:{}",
        sha256_hex(body.email.to_lowercase().as_bytes())
    );
    check_rate(
        &state,
        &ip_key,
        Limiter {
            capacity: 5.0,
            period_secs: 3_600,
        },
    )
    .await?;
    check_rate(
        &state,
        &email_key,
        Limiter {
            capacity: 3.0,
            period_secs: 3_600,
        },
    )
    .await?;

    // HIBP k-anonymity. Fail-closed: reject the password on any network
    // error (SEC-003).
    match hibp::check(&state.http, &body.password).await {
        HibpCheck::NotBreached => {}
        HibpCheck::Breached | HibpCheck::Unavailable => {
            return Err(AppError::Validation(vec![FieldError {
                field: "password".into(),
                code: "breached".into(),
            }]));
        }
    }

    // Hash password. Any failure is audited as signup.failure.
    let pwhash = match orbit_auth::password::hash(&body.password) {
        Ok(s) => s,
        Err(_) => {
            let _ = audit::record_auth(
                &state.pool,
                AuthAction::SignupFailure,
                None,
                ip_hash.as_ref().map(|s| &s[..]),
                json!({ "reason": "hash" }),
            )
            .await;
            return Err(AppError::Internal);
        }
    };

    // Insert user + verification row in one tx. Duplicate-email hits the
    // `users_email_key` UNIQUE constraint; we swallow that silently and
    // still return 201 (SEC-003 — no enumeration).
    let locale = body
        .locale_hint
        .as_deref()
        .filter(|l| *l == "es-ES" || *l == "en")
        .unwrap_or("es-ES");

    let mut tx = state.pool.begin().await.map_err(|_| AppError::Internal)?;

    let inserted: Option<Uuid> = sqlx::query(
        r#"
        INSERT INTO users (email, password_hash, locale)
        VALUES ($1, $2, $3)
        ON CONFLICT (email) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(&body.email)
    .bind(&pwhash)
    .bind(locale)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| AppError::Internal)?
    .map(|r| r.try_get::<Uuid, _>("id"))
    .transpose()
    .map_err(|_| AppError::Internal)?;

    if let Some(user_id) = inserted {
        // Prime `app.user_id` in this tx so the email_verifications INSERT
        // clears the tenant_isolation RLS policy's WITH CHECK clause.
        sqlx::query("SELECT set_config('app.user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|_| AppError::Internal)?;

        // Mint the verification token.
        let (raw_token, token_hash) = mint_token_bytes();
        sqlx::query(
            r#"
            INSERT INTO email_verifications (user_id, token_hash, expires_at)
            VALUES ($1, $2, now() + ($3::text)::interval)
            "#,
        )
        .bind(user_id)
        .bind(&token_hash[..])
        .bind(format!("{EMAIL_VERIFY_TTL_HOURS} hours"))
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::Internal)?;

        tx.commit().await.map_err(|_| AppError::Internal)?;

        // Slice 1 dev surface: log the verification link instead of sending
        // email. `SafeString` is the explicit opt-in per SEC-050.
        let link = format!("http://localhost:5173/signup/verify-email?token={raw_token}");
        orbit_log::event!(
            orbit_log::Level::Info,
            "auth.signup.verification_link",
            link = orbit_log::SafeString::new(link)
        );

        audit::record_auth(
            &state.pool,
            AuthAction::SignupSuccess,
            Some(user_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({ "locale": locale }),
        )
        .await
        .map_err(|_| AppError::Internal)?;
    } else {
        // Duplicate email — no-op, no-leak. Still record for detection.
        tx.rollback().await.ok();
        audit::record_auth(
            &state.pool,
            AuthAction::SignupFailure,
            None,
            ip_hash.as_ref().map(|s| &s[..]),
            json!({ "reason": "duplicate" }),
        )
        .await
        .map_err(|_| AppError::Internal)?;
    }

    Ok((StatusCode::CREATED, Json(json!({}))).into_response())
}

/// `POST /api/v1/auth/verify-email` — consume the token, set
/// `email_verified_at`, mint a session.
pub async fn verify_email(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    Json(body): Json<VerifyEmailBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());

    let token_hash = sha256_bytes(body.token.as_bytes());

    // Pre-session lookup goes through the SECURITY DEFINER helper
    // (migration 20260502120000_t13a_session_lookup.sql) since we don't
    // know the user_id yet. Once resolved, all writes route through a
    // per-user tx below.
    let row = sqlx::query(
        "SELECT id, user_id, expires_at, consumed_at FROM lookup_email_verification_by_hash($1)",
    )
    .bind(&token_hash[..])
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| AppError::Internal)?;

    let row = match row {
        Some(r) => r,
        None => {
            let _ = audit::record_auth(
                &state.pool,
                AuthAction::EmailVerifyFailure,
                None,
                ip_hash.as_ref().map(|s| &s[..]),
                json!({ "reason": "unknown_token" }),
            )
            .await;
            return Err(AppError::Unauthenticated);
        }
    };
    let id: Uuid = row.try_get("id").map_err(|_| AppError::Internal)?;
    let user_id: Uuid = row.try_get("user_id").map_err(|_| AppError::Internal)?;
    let expires_at: chrono::DateTime<chrono::Utc> =
        row.try_get("expires_at").map_err(|_| AppError::Internal)?;
    let consumed_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("consumed_at").map_err(|_| AppError::Internal)?;

    if consumed_at.is_some() || expires_at < chrono::Utc::now() {
        let _ = audit::record_auth(
            &state.pool,
            AuthAction::EmailVerifyFailure,
            Some(user_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({ "reason": "expired_or_consumed" }),
        )
        .await;
        return Err(AppError::Unauthenticated);
    }

    // Now that `user_id` is resolved, route all writes via Tx::for_user so
    // the email_verifications and sessions INSERT/UPDATEs are RLS-scoped.
    let mut tx = orbit_db::Tx::for_user(&state.pool, user_id)
        .await
        .map_err(|_| AppError::Internal)?;

    sqlx::query("UPDATE email_verifications SET consumed_at = now() WHERE id = $1")
        .bind(id)
        .execute(tx.as_executor())
        .await
        .map_err(|_| AppError::Internal)?;

    sqlx::query(
        "UPDATE users SET email_verified_at = now() WHERE id = $1 AND email_verified_at IS NULL",
    )
    .bind(user_id)
    .execute(tx.as_executor())
    .await
    .map_err(|_| AppError::Internal)?;

    let ua = user_agent(&headers);
    let cookies = issue_session_tx(&mut tx, user_id, ip_hash.as_ref(), &ua, &state).await?;
    tx.commit().await.map_err(|_| AppError::Internal)?;

    audit::record_auth(
        &state.pool,
        AuthAction::LoginSuccess,
        Some(user_id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({ "reason": "post_verification" }),
    )
    .await
    .map_err(|_| AppError::Internal)?;

    Ok(cookie_response(cookies, StatusCode::OK, json!({})))
}

/// `POST /api/v1/auth/signin` — email + password → session cookies.
pub async fn signin(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    Json(body): Json<SigninBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    let email_key_frag = sha256_hex(body.email.to_lowercase().as_bytes());

    // SEC-160: per-IP 10/10m, per-account 5/10m.
    let ip_key = format!("signin:ip:{}", hex_or_unknown(ip_hash.as_ref()));
    let account_key = format!("signin:account:{email_key_frag}");
    check_rate(
        &state,
        &ip_key,
        Limiter {
            capacity: 10.0,
            period_secs: 600,
        },
    )
    .await?;
    check_rate(
        &state,
        &account_key,
        Limiter {
            capacity: 5.0,
            period_secs: 600,
        },
    )
    .await?;

    // Captcha-required trigger: N consecutive failures for this email in
    // the past 10 min. Slice 1 just emits the code; no server-side verify.
    let recent_failures: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint FROM audit_log
         WHERE action = 'login.failure'
           AND payload_summary->>'email_hash' = $1
           AND occurred_at > now() - INTERVAL '10 minutes'
        "#,
    )
    .bind(&email_key_frag)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let row = sqlx::query(
        r#"
        SELECT id, password_hash, email_verified_at
          FROM users
         WHERE email = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(&body.email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| AppError::Internal)?;

    let (user_id, hash, email_verified) = match row {
        Some(r) => (
            r.try_get::<Uuid, _>("id").map_err(|_| AppError::Internal)?,
            r.try_get::<String, _>("password_hash")
                .map_err(|_| AppError::Internal)?,
            r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("email_verified_at")
                .map_err(|_| AppError::Internal)?,
        ),
        None => {
            // Record the failure (no user id — generic) and return 401.
            audit::record_auth(
                &state.pool,
                AuthAction::LoginFailure,
                None,
                ip_hash.as_ref().map(|s| &s[..]),
                json!({
                    "reason": "unknown_email",
                    "email_hash": email_key_frag,
                }),
            )
            .await
            .map_err(|_| AppError::Internal)?;
            if recent_failures + 1 >= SIGNIN_CAPTCHA_FAILURE_THRESHOLD {
                return Err(AppError::CaptchaRequired);
            }
            return Err(AppError::InvalidCredentials);
        }
    };

    let verified = orbit_auth::password::verify(&body.password, &hash).unwrap_or(false);
    if !verified || email_verified.is_none() {
        audit::record_auth(
            &state.pool,
            AuthAction::LoginFailure,
            Some(user_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({
                "reason": if verified { "email_unverified" } else { "bad_password" },
                "email_hash": email_key_frag,
            }),
        )
        .await
        .map_err(|_| AppError::Internal)?;
        if recent_failures + 1 >= SIGNIN_CAPTCHA_FAILURE_THRESHOLD {
            return Err(AppError::CaptchaRequired);
        }
        return Err(AppError::InvalidCredentials);
    }

    // Issue the session inside a per-user tx so the sessions INSERT
    // passes RLS WITH CHECK.
    let mut tx = orbit_db::Tx::for_user(&state.pool, user_id)
        .await
        .map_err(|_| AppError::Internal)?;
    let ua = user_agent(&headers);
    let cookies = issue_session_tx(&mut tx, user_id, ip_hash.as_ref(), &ua, &state).await?;
    tx.commit().await.map_err(|_| AppError::Internal)?;

    audit::record_auth(
        &state.pool,
        AuthAction::LoginSuccess,
        Some(user_id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({ "reason": "password" }),
    )
    .await
    .map_err(|_| AppError::Internal)?;

    Ok(cookie_response(cookies, StatusCode::OK, json!({})))
}

/// `POST /api/v1/auth/signout` — revoke the current session.
pub async fn signout(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    ip: ClientIp,
) -> Result<Response, AppError> {
    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id)
        .await
        .map_err(|_| AppError::Internal)?;
    sqlx::query(
        r#"
        UPDATE sessions
           SET revoked_at = now(),
               revoke_reason = 'user_signout'
         WHERE id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(auth.session_id)
    .execute(tx.as_executor())
    .await
    .map_err(|_| AppError::Internal)?;
    tx.commit().await.map_err(|_| AppError::Internal)?;

    let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
    audit::record_auth(
        &state.pool,
        AuthAction::Logout,
        Some(auth.user_id),
        ip_hash.as_ref().map(|s| &s[..]),
        json!({}),
    )
    .await
    .map_err(|_| AppError::Internal)?;

    // Clear cookies by emitting expired placeholders.
    let jar = clear_cookies(&state);
    let mut resp = StatusCode::NO_CONTENT.into_response();
    apply_cookies(resp.headers_mut(), jar);
    Ok(resp)
}

/// `GET /api/v1/auth/me` — identity for the SPA on boot.
///
/// T13a returns residency=null and stage in {"disclaimer", "residency"}.
/// T13b extends this to full onboarding stage resolution (first_grant,
/// complete).
pub async fn me(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
) -> Result<Json<MeResponse>, AppError> {
    // User row is not RLS-scoped (the ADR-014 DDL leaves users non-RLS
    // with access via id). Read directly.
    let row = sqlx::query(
        r#"
        SELECT id, email, locale, primary_currency, disclaimer_accepted_at
          FROM users
         WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| AppError::Internal)?
    .ok_or(AppError::Unauthenticated)?;

    let id: Uuid = row.try_get("id").map_err(|_| AppError::Internal)?;
    let email: String = row.try_get("email").map_err(|_| AppError::Internal)?;
    let locale: String = row.try_get("locale").map_err(|_| AppError::Internal)?;
    let primary_currency: String = row
        .try_get("primary_currency")
        .map_err(|_| AppError::Internal)?;
    let disclaimer: Option<chrono::DateTime<chrono::Utc>> = row
        .try_get("disclaimer_accepted_at")
        .map_err(|_| AppError::Internal)?;

    let stage = if disclaimer.is_none() {
        "disclaimer"
    } else {
        "residency"
    };

    Ok(Json(MeResponse {
        user: MeUser {
            id,
            email,
            locale,
            primary_currency,
        },
        residency: None,
        onboarding_stage: stage,
    }))
}

/// `POST /api/v1/auth/mfa/*` — Slice 1 scaffold only.
pub async fn mfa_not_implemented() -> AppError {
    AppError::NotImplemented
}

// ---------------------------------------------------------------------------
// Session issuance
// ---------------------------------------------------------------------------

struct IssuedCookies {
    session: Cookie<'static>,
    refresh: Cookie<'static>,
    csrf: Cookie<'static>,
}

/// Mint + persist a new session. Caller supplies the `Tx` so the INSERT
/// participates in the surrounding user-scoped transaction (RLS on
/// `sessions` requires `app.user_id` to match the row being inserted).
async fn issue_session_tx(
    tx: &mut orbit_db::Tx<'_>,
    user_id: Uuid,
    ip_hash: Option<&[u8; 32]>,
    user_agent: &str,
    state: &AppState,
) -> Result<IssuedCookies, AppError> {
    let (session_token, session_hash) = orbit_auth::session::new_session_token();
    let (csrf_token, _) = orbit_auth::session::new_csrf_token();

    // Refresh token: same shape as session, separate storage slot.
    let (refresh_raw, refresh_hash) = mint_token_bytes();
    let family_id = Uuid::new_v4();
    let ip_bytes: &[u8] = ip_hash.map(|h| &h[..]).unwrap_or(&[0u8; 32][..]);

    sqlx::query(
        r#"
        INSERT INTO sessions (
            user_id, session_id_hash, refresh_token_hash, family_id,
            ip_hash, user_agent
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(user_id)
    .bind(&session_hash.0[..])
    .bind(&refresh_hash[..])
    .bind(family_id)
    .bind(ip_bytes)
    .bind(truncate(user_agent, 512))
    .execute(tx.as_executor())
    .await
    .map_err(|_| AppError::Internal)?;

    Ok(IssuedCookies {
        session: build_cookie(
            orbit_auth::session::SESSION_COOKIE_NAME,
            session_token.as_cookie_value().to_string(),
            "/",
            SESSION_TTL_SECS,
            true,
            state.cookie_secure,
        ),
        refresh: build_cookie(
            REFRESH_COOKIE,
            refresh_raw,
            "/api/v1/auth",
            REFRESH_TTL_SECS,
            true,
            state.cookie_secure,
        ),
        csrf: build_cookie(
            CSRF_COOKIE,
            csrf_token.as_cookie_value().to_string(),
            "/",
            SESSION_TTL_SECS,
            false,
            state.cookie_secure,
        ),
    })
}

fn clear_cookies(state: &AppState) -> IssuedCookies {
    IssuedCookies {
        session: build_cookie(
            orbit_auth::session::SESSION_COOKIE_NAME,
            String::new(),
            "/",
            0,
            true,
            state.cookie_secure,
        ),
        refresh: build_cookie(
            REFRESH_COOKIE,
            String::new(),
            "/api/v1/auth",
            0,
            true,
            state.cookie_secure,
        ),
        csrf: build_cookie(
            CSRF_COOKIE,
            String::new(),
            "/",
            0,
            false,
            state.cookie_secure,
        ),
    }
}

fn build_cookie(
    name: &'static str,
    value: String,
    path: &'static str,
    max_age_secs: i64,
    http_only: bool,
    secure: bool,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .path(path)
        .http_only(http_only)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(CookieDuration::seconds(max_age_secs))
        .build()
}

fn cookie_response(cookies: IssuedCookies, status: StatusCode, body: Value) -> Response {
    let mut resp = (status, Json(body)).into_response();
    apply_cookies(resp.headers_mut(), cookies);
    resp
}

fn apply_cookies(headers: &mut HeaderMap, cookies: IssuedCookies) {
    for c in [cookies.session, cookies.refresh, cookies.csrf] {
        if let Ok(v) = HeaderValue::from_str(&c.to_string()) {
            headers.append(header::SET_COOKIE, v);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Client IP extractor (X-Forwarded-For aware). Handlers consume this as
/// `ip: ClientIp` and feed it to the HMAC for `ip_hash`.
pub struct ClientIp(pub Option<String>);

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // Prefer X-Forwarded-For (first IP), fall back to ConnectInfo.
        if let Some(xff) = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(first) = xff.split(',').next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    return Ok(ClientIp(Some(trimmed.to_string())));
                }
            }
        }
        if let Some(ci) = parts.extensions.get::<ConnectInfo<SocketAddr>>() {
            return Ok(ClientIp(Some(ci.0.ip().to_string())));
        }
        Ok(ClientIp(None))
    }
}

fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}

fn mint_token_bytes() -> (String, [u8; 32]) {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use rand::rngs::OsRng;
    use rand::RngCore;

    let mut raw = [0u8; 32];
    OsRng.fill_bytes(&mut raw);
    let encoded = URL_SAFE_NO_PAD.encode(raw);
    let hash: [u8; 32] = Sha256::digest(encoded.as_bytes()).into();
    (encoded, hash)
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256_bytes(data))
}

fn hex_or_unknown(hash: Option<&[u8; 32]>) -> String {
    match hash {
        Some(h) => hex::encode(&h[..]),
        None => "unknown".to_string(),
    }
}

async fn check_rate(state: &AppState, key: &str, limiter: Limiter) -> Result<(), AppError> {
    match rate_limit::try_consume(&state.pool, key, limiter).await {
        Ok(Decision::Allowed) => Ok(()),
        Ok(Decision::RateLimited { retry_after_secs }) => {
            Err(AppError::RateLimited { retry_after_secs })
        }
        Err(_) => Err(AppError::Internal),
    }
}

fn validation_errors(err: validator::ValidationErrors) -> AppError {
    let fields = err
        .field_errors()
        .into_iter()
        .flat_map(|(name, errs)| {
            errs.iter().map(move |e| FieldError {
                field: name.to_string(),
                code: e.code.to_string(),
            })
        })
        .collect();
    AppError::Validation(fields)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_byte_cap() {
        assert_eq!(truncate("abcdef", 3), "abc");
        assert_eq!(truncate("abc", 6), "abc");
    }

    #[test]
    fn mint_token_is_base64url() {
        let (raw, hash) = mint_token_bytes();
        assert_eq!(raw.len(), 43);
        assert_eq!(hash.len(), 32);
        assert!(raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    }
}
