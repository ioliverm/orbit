//! CSRF double-submit middleware (SEC-188).
//!
//! On state-changing methods (POST/PUT/PATCH/DELETE), require a non-empty
//! `orbit_csrf` cookie and a matching `X-CSRF-Token` header. The comparison
//! is constant-time via `orbit_auth::session::verify_csrf_double_submit`.
//!
//! This middleware is installed on authenticated routes only. Pre-session
//! endpoints (`/auth/signup`, `/auth/signin`, `/auth/verify-email`) skip
//! the check because the CSRF cookie is minted as part of the response.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderName, Method};
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::CookieJar;

use crate::error::AppError;

const CSRF_COOKIE: &str = "orbit_csrf";
const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");

pub async fn require(jar: CookieJar, req: Request<Body>, next: Next) -> Result<Response, AppError> {
    if is_state_changing(req.method()) {
        let header = req
            .headers()
            .get(&CSRF_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let cookie = jar.get(CSRF_COOKIE).map(|c| c.value()).unwrap_or("");
        if !orbit_auth::session::verify_csrf_double_submit(header, cookie) {
            return Err(AppError::CsrfMismatch);
        }
    }
    Ok(next.run(req).await)
}

fn is_state_changing(m: &Method) -> bool {
    matches!(
        *m,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}
