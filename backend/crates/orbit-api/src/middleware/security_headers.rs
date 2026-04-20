//! Security header middleware (SEC-180..SEC-186).
//!
//! CSP strict — no `'unsafe-inline'`, no `'unsafe-eval'`. These are applied
//! to every response regardless of content type so that a stray HTML error
//! page would still be covered.

use axum::http::{header, HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;

const CSP: &str = "default-src 'self'; \
    script-src 'self'; \
    style-src 'self'; \
    img-src 'self' data:; \
    font-src 'self'; \
    connect-src 'self'; \
    frame-ancestors 'none'; \
    form-action 'self'; \
    base-uri 'self'; \
    object-src 'none'";

const PERMISSIONS: &str = "geolocation=(), camera=(), microphone=(), payment=(self), usb=()";

pub async fn layer(req: Request<axum::body::Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();

    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(PERMISSIONS),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    resp
}
