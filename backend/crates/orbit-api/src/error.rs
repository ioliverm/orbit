//! API error envelope (ADR-010 §7, SEC-051).
//!
//! Every error response has the shape:
//!
//! ```json
//! { "error": { "code": "<stable-id>", "message": "<short>", "details": {...}? } }
//! ```
//!
//! `code` is stable; the SPA switches on it. `message` is a short English
//! string used as a developer-readable fallback — the SPA renders user
//! copy from its locale catalogue keyed by `code`, so `message` is never
//! shown to end users directly.
//!
//! Critically: response bodies never carry stack traces, validated input,
//! or any Financial-Personal field (SEC-051). The `details` payload is
//! restricted to validation field names and error-code strings.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};

/// Uniform error envelope emitted on 4xx / 5xx.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub error: ErrorPayload,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub code: &'static str,
    pub message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// API-level error kinds.
///
/// Kept narrow on purpose: each variant maps to one HTTP status and one
/// stable error code. Handler code constructs these; middleware translates
/// at the boundary.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 400 — malformed JSON or missing envelope fields.
    #[error("request malformed")]
    BadRequest,

    /// 401 — no session / expired session.
    #[error("unauthenticated")]
    Unauthenticated,

    /// 401 — generic credential failure (SEC-003 / SEC-004). Never says
    /// "unknown user" vs "wrong password" vs "locked".
    #[error("invalid credentials")]
    InvalidCredentials,

    /// 401 — the client has exhausted the per-account failure budget and
    /// must solve a CAPTCHA (ADR-011 signin flow; Slice 1 emits the code
    /// but does not yet verify a captcha challenge).
    #[error("captcha required")]
    CaptchaRequired,

    /// 403 — CSRF double-submit mismatch.
    #[error("csrf mismatch")]
    CsrfMismatch,

    /// 403 — the user is trying to reach a route beyond their current
    /// onboarding stage. The SPA uses `details.stage` to route them to the
    /// correct wizard step (ADR-014 §3, AC G-8).
    #[error("onboarding required")]
    OnboardingRequired { stage: &'static str },

    /// 403 — the user asked to revoke their own current session via the
    /// device-list endpoint (AC-7.2.3). The UI should route them through
    /// sign-out instead.
    #[error("cannot revoke current session")]
    CannotRevokeCurrent,

    /// 404 — resource not found or not owned (RLS fail-closed, AC-7.3).
    #[error("not found")]
    NotFound,

    /// 409 — optimistic-concurrency mismatch. The client holds a stale
    /// `updated_at` for a resource that has since been written. The SPA
    /// surfaces the AC-10.5 copy and prompts a refresh.
    #[error("conflict")]
    Conflict,

    /// 422 — validation error with a per-field map.
    #[error("validation")]
    Validation(Vec<FieldError>),

    /// 429 — rate limit exceeded. `retry_after_secs` populates `Retry-After`.
    #[error("rate limited")]
    RateLimited { retry_after_secs: u64 },

    /// 501 — declared endpoint, not yet implemented (auth/mfa/*).
    #[error("not implemented")]
    NotImplemented,

    /// 500 — server error. Message body stays generic.
    #[error("server internal")]
    Internal,
}

/// Validation failure for a single field. `field` is the `camelCase` JSON
/// name; `code` is a stable identifier like `"required"` or
/// `"cliff_exceeds_vesting"`.
#[derive(Debug, Clone, Serialize)]
pub struct FieldError {
    pub field: String,
    pub code: String,
}

impl AppError {
    fn parts(&self) -> (StatusCode, &'static str, &'static str, Option<Value>) {
        match self {
            AppError::BadRequest => (
                StatusCode::BAD_REQUEST,
                "request_malformed",
                "The request could not be parsed.",
                None,
            ),
            AppError::Unauthenticated => (
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "Authentication required.",
                None,
            ),
            AppError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "auth",
                "Invalid credentials.",
                None,
            ),
            AppError::CaptchaRequired => (
                StatusCode::UNAUTHORIZED,
                "captcha_required",
                "Resolve the captcha challenge and retry.",
                None,
            ),
            AppError::CsrfMismatch => (StatusCode::FORBIDDEN, "csrf", "CSRF token mismatch.", None),
            AppError::OnboardingRequired { stage } => (
                StatusCode::FORBIDDEN,
                "onboarding.required",
                "Complete the onboarding step before continuing.",
                Some(json!({ "stage": stage })),
            ),
            AppError::CannotRevokeCurrent => (
                StatusCode::FORBIDDEN,
                "cannot_revoke_current",
                "Cannot revoke the current session from the device list.",
                None,
            ),
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Resource not found.",
                None,
            ),
            AppError::Conflict => (
                StatusCode::CONFLICT,
                "resource.stale_client_state",
                "The resource was modified elsewhere; refresh to see current values.",
                None,
            ),
            AppError::Validation(fields) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                "One or more fields failed validation.",
                Some(json!({ "fields": fields })),
            ),
            AppError::RateLimited { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Try again later.",
                None,
            ),
            AppError::NotImplemented => (
                StatusCode::NOT_IMPLEMENTED,
                "not_implemented",
                "This endpoint is not yet implemented.",
                None,
            ),
            AppError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_internal",
                "Internal server error.",
                None,
            ),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message, details) = self.parts();
        let body = ErrorBody {
            error: ErrorPayload {
                code,
                message,
                details,
            },
        };
        let mut resp = (status, Json(body)).into_response();
        if let AppError::RateLimited { retry_after_secs } = self {
            if let Ok(v) = retry_after_secs.to_string().parse() {
                resp.headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }
        resp
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        // Row-not-found is the RLS fail-closed path (AC-7.3): map to 404.
        if matches!(e, sqlx::Error::RowNotFound) {
            return AppError::NotFound;
        }
        AppError::Internal
    }
}

impl From<orbit_db::Error> for AppError {
    fn from(_: orbit_db::Error) -> Self {
        AppError::Internal
    }
}
