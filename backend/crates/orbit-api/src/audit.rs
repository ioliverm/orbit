//! Typed audit_log writer (SEC-100..SEC-103).
//!
//! The `orbit_app` role has INSERT-only grants on `audit_log`; UPDATE /
//! DELETE are forbidden at the DB layer (SEC-102). `payload_summary` is
//! constrained to a narrow allowlist of non-FP dimensions (SEC-101) —
//! callers build it via [`payload`] + typed JSON literals, never from
//! user-supplied structs.

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Action taxonomy for Slice 1 auth handlers. Keep this enum small; adding
/// a variant is a deliberate security review (SEC-100).
#[derive(Debug, Clone, Copy)]
pub enum AuthAction {
    SignupSuccess,
    SignupFailure,
    LoginSuccess,
    LoginFailure,
    Logout,
    EmailVerifySuccess,
    EmailVerifyFailure,
}

impl AuthAction {
    fn as_str(self) -> &'static str {
        match self {
            AuthAction::SignupSuccess => "signup.success",
            AuthAction::SignupFailure => "signup.failure",
            AuthAction::LoginSuccess => "login.success",
            AuthAction::LoginFailure => "login.failure",
            AuthAction::Logout => "logout",
            AuthAction::EmailVerifySuccess => "email_verify.success",
            AuthAction::EmailVerifyFailure => "email_verify.failure",
        }
    }
}

/// Wizard + grant CRUD actions (Slice 1 T13b). Every variant has an
/// allowlisted `payload_summary` shape (SEC-101) — share counts, strike
/// amounts, autonomía values and the Beckham flag NEVER appear in the
/// payload; handlers build the JSON via a typed helper at each call site.
#[derive(Debug, Clone, Copy)]
pub enum WizardAction {
    /// `dsr.consent.disclaimer_accepted` — payload `{ version }`.
    DisclaimerAccepted,
    /// `residency.create` — payload booleans only per AC-4.1.8.
    ResidencyCreate,
    /// `grant.create` — payload `{ instrument, double_trigger, cadence }`.
    GrantCreate,
    /// `grant.update` — same shape.
    GrantUpdate,
    /// `grant.delete` — payload `{ instrument }`.
    GrantDelete,
}

impl WizardAction {
    fn as_str(self) -> &'static str {
        match self {
            WizardAction::DisclaimerAccepted => "dsr.consent.disclaimer_accepted",
            WizardAction::ResidencyCreate => "residency.create",
            WizardAction::GrantCreate => "grant.create",
            WizardAction::GrantUpdate => "grant.update",
            WizardAction::GrantDelete => "grant.delete",
        }
    }

    fn target_kind(self) -> &'static str {
        match self {
            WizardAction::DisclaimerAccepted => "user",
            WizardAction::ResidencyCreate => "residency_period",
            WizardAction::GrantCreate | WizardAction::GrantUpdate | WizardAction::GrantDelete => {
                "grant"
            }
        }
    }
}

/// INSERT a single audit_log row. `ip_hash` is the HMAC-SHA256 of the
/// request IP with the ip-hash key from `AppState` (SEC-054).
///
/// Auth-layer inserts bypass `Tx::for_user` because (a) the signup/login
/// path does not yet have a user-scoped tx and (b) `audit_log` is not
/// RLS-scoped (operator reads go via `orbit_support`).
pub async fn record_auth(
    pool: &PgPool,
    action: AuthAction,
    user_id: Option<Uuid>,
    ip_hash: Option<&[u8]>,
    payload: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            user_id, actor_kind, action, target_kind, target_id,
            ip_hash, payload_summary
        )
        VALUES ($1, 'user', $2, NULL, $1, $3, $4)
        "#,
    )
    .bind(user_id)
    .bind(action.as_str())
    .bind(ip_hash)
    .bind(payload)
    .execute(pool)
    .await?;
    Ok(())
}

/// INSERT an audit row for a wizard / grant action. Runs on the pool
/// because `audit_log` is not RLS-scoped; callers that already hold a
/// `Tx::for_user` can just pass the pool and don't need to thread the
/// audit write through their transaction (the DB keeps integrity via the
/// handler's commit/rollback of the underlying mutation — a committed
/// residency/grant write without its audit row is a correctness bug we
/// prevent by ordering the audit INSERT before the outer `tx.commit()`).
pub async fn record_wizard(
    pool: &PgPool,
    action: WizardAction,
    user_id: Uuid,
    target_id: Option<Uuid>,
    ip_hash: Option<&[u8]>,
    payload: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_log (
            user_id, actor_kind, action, target_kind, target_id,
            ip_hash, payload_summary
        )
        VALUES ($1, 'user', $2, $3, $4, $5, $6)
        "#,
    )
    .bind(user_id)
    .bind(action.as_str())
    .bind(action.target_kind())
    .bind(target_id.unwrap_or(user_id))
    .bind(ip_hash)
    .bind(payload)
    .execute(pool)
    .await?;
    Ok(())
}

/// Compute the HMAC-SHA256 `ip_hash` for a raw IP string per SEC-054. Any
/// input (including "unknown") is stable-hashed; a `None` IP maps to
/// `None` (no hash).
pub fn hash_ip(key: &[u8; 32], ip: Option<&str>) -> Option<[u8; 32]> {
    let raw = ip?;
    let mut mac = HmacSha256::new_from_slice(key).ok()?;
    mac.update(raw.as_bytes());
    let out = mac.finalize().into_bytes();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    Some(arr)
}
