//! Consent endpoints (Slice 1 T13b).
//!
//! Currently: `POST /api/v1/consent/disclaimer`. Authenticated, CSRF-gated.
//! Does **not** require onboarding completion — this is the endpoint that
//! completes the first onboarding step. Idempotent per ADR-014 §3: if
//! `users.disclaimer_accepted_at` is already set the handler is a no-op
//! and the audit row is not re-written.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::json;
use validator::Validate;

use crate::audit::{self, WizardAction};
use crate::error::{AppError, FieldError};
use crate::handlers::auth::ClientIp;
use crate::middleware::session::SessionAuth;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DisclaimerBody {
    #[validate(length(min = 1, max = 64))]
    pub version: String,
}

/// `POST /api/v1/consent/disclaimer` — record disclaimer acceptance (G-9).
pub async fn disclaimer(
    State(state): State<AppState>,
    Extension(auth): Extension<SessionAuth>,
    ip: ClientIp,
    Json(body): Json<DisclaimerBody>,
) -> Result<Response, AppError> {
    body.validate().map_err(validation_errors)?;

    let mut tx = orbit_db::Tx::for_user(&state.pool, auth.user_id).await?;

    // Idempotent: set only when not already accepted. `RETURNING` lets us
    // detect whether this call actually mutated anything so we don't write
    // duplicate audit rows on refresh (AC-G-10 implied by "re-login does
    // not re-display the modal").
    let updated: Option<(String,)> = sqlx::query_as(
        r#"
        UPDATE users
           SET disclaimer_accepted_at      = now(),
               disclaimer_accepted_version = $2
         WHERE id = $1
           AND disclaimer_accepted_at IS NULL
        RETURNING disclaimer_accepted_version AS v
        "#,
    )
    .bind(auth.user_id)
    .bind(&body.version)
    .fetch_optional(tx.as_executor())
    .await?;
    tx.commit().await?;

    if updated.is_some() {
        let ip_hash = audit::hash_ip(&state.ip_hash_key, ip.0.as_deref());
        audit::record_wizard(
            &state.pool,
            WizardAction::DisclaimerAccepted,
            auth.user_id,
            Some(auth.user_id),
            ip_hash.as_ref().map(|s| &s[..]),
            json!({ "version": body.version }),
        )
        .await?;
    }

    Ok(StatusCode::NO_CONTENT.into_response())
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
