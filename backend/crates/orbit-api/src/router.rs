//! Router assembly.
//!
//! The shape per ADR-010:
//!
//!   * `/api/v1/auth/{signup, verify-email, signin, signout, me}`
//!   * `/api/v1/auth/mfa/*`  → 501 (ADR-011 §MFA scaffolding)
//!   * `/api/v1/consent/disclaimer` — T13b, authenticated, wizard-exempt.
//!   * `/api/v1/residency{,/autonomias}` — T13b, autonomias public.
//!   * `/api/v1/grants{,/:id,/:id/vesting}` — T13b, CRUD + vesting read,
//!     gated by the onboarding middleware.
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

    // Authenticated subtree: session + CSRF are required. Wizard-stage
    // endpoints (`/auth/*`, `/consent/*`, `/residency*`) live here WITHOUT
    // the onboarding gate — they are how the user *completes* the wizard.
    // Session management (Slice 2 T21) also lives here — AC-7.1.1 asks
    // that the device list be reachable regardless of onboarding stage.
    let wizard_authed = Router::new()
        .route("/auth/signout", post(handlers::auth::signout))
        .route("/auth/me", get(handlers::auth::me))
        .route("/auth/sessions", get(handlers::auth::list_sessions))
        .route(
            "/auth/sessions/:session_id",
            axum::routing::delete(handlers::auth::revoke_session),
        )
        .route(
            "/auth/sessions/revoke-all-others",
            post(handlers::auth::revoke_all_others),
        )
        .route("/consent/disclaimer", post(handlers::consent::disclaimer))
        .route(
            "/residency",
            get(handlers::residency::get).post(handlers::residency::create),
        )
        // Slice 3b T38 — Profile tax-preferences. Same posture as residency:
        // authenticated but onboarding-gate-exempt, so a user who returns
        // to the Profile section before completing onboarding can save.
        .route(
            "/user-tax-preferences",
            get(handlers::user_tax_preferences::list_history)
                .post(handlers::user_tax_preferences::upsert),
        )
        .route(
            "/user-tax-preferences/current",
            get(handlers::user_tax_preferences::get_current),
        );

    // Endpoints past the wizard: the onboarding gate applies here. Returns
    // 403 `onboarding.required` with the user's current stage if the user
    // has not yet completed disclaimer + residency + first-grant (AC G-8).
    let gated_authed = Router::new()
        .route(
            "/grants",
            get(handlers::grants::list).post(handlers::grants::create),
        )
        .route(
            "/grants/:id",
            get(handlers::grants::get_one)
                .put(handlers::grants::update)
                .delete(handlers::grants::delete),
        )
        .route(
            "/grants/:id/vesting",
            get(handlers::grants::vesting_for_grant),
        )
        // Slice 2 T21 — ESPP purchases, Art.7.p trips, M720 inputs,
        // stacked-dashboard read.
        .route(
            "/grants/:grant_id/espp-purchases",
            get(handlers::espp_purchases::list_for_grant).post(handlers::espp_purchases::create),
        )
        .route(
            "/espp-purchases/:id",
            get(handlers::espp_purchases::get_one)
                .put(handlers::espp_purchases::update)
                .delete(handlers::espp_purchases::delete),
        )
        .route(
            "/trips",
            get(handlers::trips::list).post(handlers::trips::create),
        )
        .route(
            "/trips/:id",
            get(handlers::trips::get_one)
                .put(handlers::trips::update)
                .delete(handlers::trips::delete),
        )
        .route(
            "/modelo-720-inputs",
            get(handlers::modelo_720_inputs::list_history)
                .post(handlers::modelo_720_inputs::upsert),
        )
        .route(
            "/modelo-720-inputs/current",
            get(handlers::modelo_720_inputs::get_current),
        )
        .route("/dashboard/stacked", get(handlers::dashboard::stacked))
        // Slice 3 T29 — current prices, vesting-event overrides, paper-gains,
        // M720 threshold, rule-set chip.
        .route("/current-prices", get(handlers::current_prices::list))
        .route(
            "/current-prices/:ticker",
            axum::routing::put(handlers::current_prices::upsert)
                .delete(handlers::current_prices::delete),
        )
        .route(
            "/grants/:id/current-price-override",
            get(handlers::current_prices::get_grant_override)
                .put(handlers::current_prices::upsert_grant_override)
                .delete(handlers::current_prices::delete_grant_override),
        )
        .route(
            "/grants/:grantId/vesting-events/:eventId",
            axum::routing::put(handlers::vesting_events::upsert_override),
        )
        .route(
            "/grants/:grantId/vesting-events/bulk-fmv",
            post(handlers::vesting_events::bulk_fmv),
        )
        .route(
            "/dashboard/paper-gains",
            get(handlers::dashboard_paper_gains::paper_gains),
        )
        .route(
            "/dashboard/modelo-720-threshold",
            get(handlers::dashboard_m720::threshold),
        )
        .route("/rule-set-chip", get(handlers::rule_set_chip::get))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            mw::onboarding::require_first_grant_or_later,
        ));

    let authed = Router::new()
        .merge(wizard_authed)
        .merge(gated_authed)
        .layer(middleware::from_fn(mw::csrf::require))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            mw::session::require,
        ));

    // Unauthenticated auth subtree. CSRF skipped per ADR-011 note:
    // signup/signin/verify-email are the endpoints that *issue* the CSRF
    // cookie, so requiring one before issuance would be a chicken-and-egg.
    //
    // The `/residency/autonomias` endpoint is unauthenticated by design
    // (ADR-014 Alternatives): the SPA fetches it on wizard mount, before
    // any user identity exists.
    let public_api = Router::new()
        .route("/auth/signup", post(handlers::auth::signup))
        .route("/auth/signin", post(handlers::auth::signin))
        .route("/auth/verify-email", post(handlers::auth::verify_email))
        .route("/auth/mfa/:rest", any(handlers::auth::mfa_not_implemented))
        .route(
            "/residency/autonomias",
            get(handlers::residency::list_autonomias),
        )
        // Slice 3 T29 — FX reads are reference-data (no auth, not
        // RLS-scoped). Same posture as /residency/autonomias.
        .route("/fx/rate", get(handlers::fx::get_rate))
        .route("/fx/latest", get(handlers::fx::get_latest));

    let api = Router::new().merge(public_api).merge(authed);

    Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            mw::security_headers::layer,
        ))
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
