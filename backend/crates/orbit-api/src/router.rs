//! Router assembly.
//!
//! The shape per ADR-010:
//!
//!   * `/api/v1/auth/{signup, verify-email, signin, signout, me}`
//!   * `/api/v1/auth/mfa/*`  → 501 (ADR-011 §MFA scaffolding)
//!   * `/healthz`, `/readyz` — NOT under `/api/v1`.

use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::routing::{any, get, post};
use axum::{middleware, Router};
use http::{header, HeaderValue, Method};
use tower::ServiceBuilder;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::middleware as mw;
use crate::state::AppState;

const BODY_LIMIT_BYTES: usize = 128 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Build the fully-wired axum router.
pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(
            HeaderValue::from_str(&state.cors_origin)
                .map(AllowOrigin::exact)
                .unwrap_or_else(|_| {
                    AllowOrigin::exact(HeaderValue::from_static("http://localhost:5173"))
                }),
        )
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::ACCEPT,
            http::HeaderName::from_static("x-csrf-token"),
            http::HeaderName::from_static("x-request-id"),
        ])
        .max_age(Duration::from_secs(600));

    // Authenticated subtree: session + CSRF are required.
    let authed = Router::new()
        .route("/auth/signout", post(handlers::auth::signout))
        .route("/auth/me", get(handlers::auth::me))
        .layer(middleware::from_fn(mw::csrf::require))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            mw::session::require,
        ));

    // Unauthenticated auth subtree. CSRF skipped per ADR-011 note:
    // signup/signin/verify-email are the endpoints that *issue* the CSRF
    // cookie, so requiring one before issuance would be a chicken-and-egg.
    let public_auth = Router::new()
        .route("/auth/signup", post(handlers::auth::signup))
        .route("/auth/signin", post(handlers::auth::signin))
        .route("/auth/verify-email", post(handlers::auth::verify_email))
        .route("/auth/mfa/:rest", any(handlers::auth::mfa_not_implemented));

    let api = Router::new().merge(public_auth).merge(authed);

    Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(middleware::from_fn(mw::security_headers::layer))
        .layer(middleware::from_fn(mw::request_id::layer))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(TimeoutLayer::with_status_code(
                    http::StatusCode::GATEWAY_TIMEOUT,
                    REQUEST_TIMEOUT,
                ))
                .layer(cors),
        )
        .with_state(state)
}
