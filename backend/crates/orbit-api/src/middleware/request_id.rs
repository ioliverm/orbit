//! `X-Request-Id` injection middleware (ADR-010 §6 "Request correlation").
//!
//! Every response carries an `X-Request-Id: <uuid>` header, either echoed
//! from the inbound request (when a trusted upstream set one) or minted
//! server-side. Logs from `orbit_log::event!` tag the same id via
//! `Extensions::insert` so the correlation is end-to-end.

use axum::http::{HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

pub const HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Request id stashed in `request.extensions()` for handler/log access.
#[derive(Debug, Clone, Copy)]
pub struct RequestId(pub Uuid);

/// axum middleware: mint-or-echo the request id, attach it to extensions,
/// and set it on the outgoing response.
pub async fn layer(mut req: Request<axum::body::Body>, next: Next) -> Response {
    let id = req
        .headers()
        .get(&HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::new_v4);

    req.extensions_mut().insert(RequestId(id));

    let mut resp = next.run(req).await;
    if let Ok(value) = HeaderValue::from_str(&id.to_string()) {
        resp.headers_mut().insert(HEADER, value);
    }
    resp
}
