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
use sqlx::{PgConnection, PgPool};
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
///
/// Slice 2 T21 extends the taxonomy with the ESPP purchase, Art. 7.p trip,
/// Modelo 720 upsert and session-revoke actions. The allowlists are
/// declared per-variant below; T23 probes assert they are honored.
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

    // --- Slice 2 T21 ---
    /// `espp_purchase.create` — payload allowlist:
    /// `{ currency, had_lookback: bool, had_discount: bool, notes_lift: bool }`.
    /// **Never** FMV, purchase price, share count, or raw notes text.
    EsppPurchaseCreate,
    /// `espp_purchase.update` — same allowlist as create (`notes_lift`
    /// is always `false` on update; the lift only fires on first purchase).
    EsppPurchaseUpdate,
    /// `espp_purchase.delete` — payload `{ currency }`.
    EsppPurchaseDelete,

    /// `trip.create` — payload allowlist:
    /// `{ country, criteria_answered: int (0..=5), employer_paid: bool }`.
    /// **No raw purpose text, no dates, no criterion values.**
    TripCreate,
    /// `trip.update` — same allowlist as create.
    TripUpdate,
    /// `trip.delete` — payload `{}` (empty — destination and dates would
    /// be leakage).
    TripDelete,

    /// `modelo_720_inputs.upsert` — payload allowlist:
    /// `{ category, outcome }` where `outcome` is one of
    /// `"inserted" | "closed_and_created" | "updated_same_day"`. The
    /// `NoOp` branch writes NO audit row (AC-6.2.5).
    Modelo720Upsert,

    /// `session.revoke` — payload allowlist:
    /// `{ kind: "single", initiator: "self" }`
    /// or `{ kind: "bulk", initiator: "self", count: int }`.
    SessionRevoke,
}

impl WizardAction {
    fn as_str(self) -> &'static str {
        match self {
            WizardAction::DisclaimerAccepted => "dsr.consent.disclaimer_accepted",
            WizardAction::ResidencyCreate => "residency.create",
            WizardAction::GrantCreate => "grant.create",
            WizardAction::GrantUpdate => "grant.update",
            WizardAction::GrantDelete => "grant.delete",
            WizardAction::EsppPurchaseCreate => "espp_purchase.create",
            WizardAction::EsppPurchaseUpdate => "espp_purchase.update",
            WizardAction::EsppPurchaseDelete => "espp_purchase.delete",
            WizardAction::TripCreate => "trip.create",
            WizardAction::TripUpdate => "trip.update",
            WizardAction::TripDelete => "trip.delete",
            WizardAction::Modelo720Upsert => "modelo_720_inputs.upsert",
            WizardAction::SessionRevoke => "session.revoke",
        }
    }

    fn target_kind(self) -> &'static str {
        match self {
            WizardAction::DisclaimerAccepted => "user",
            WizardAction::ResidencyCreate => "residency_period",
            WizardAction::GrantCreate | WizardAction::GrantUpdate | WizardAction::GrantDelete => {
                "grant"
            }
            WizardAction::EsppPurchaseCreate
            | WizardAction::EsppPurchaseUpdate
            | WizardAction::EsppPurchaseDelete => "espp_purchase",
            WizardAction::TripCreate | WizardAction::TripUpdate | WizardAction::TripDelete => {
                "art_7p_trip"
            }
            WizardAction::Modelo720Upsert => "modelo_720_input",
            WizardAction::SessionRevoke => "session",
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

/// INSERT an audit row for a wizard / grant action inside an existing
/// per-user transaction.
///
/// The audit INSERT rides on the same `Tx::for_user` that drove the
/// handler's mutation and MUST be issued **before** `tx.commit()`. This
/// closes the post-commit-crash window where a committed mutation could
/// miss its audit row: either both land or neither does (T25 / S1). The
/// `orbit_app` role has INSERT on `audit_log`; the table is not RLS-
/// scoped, so the per-user GUC does not block the write — the tx
/// membership is what we're after, not tenant scoping.
///
/// Call sites that genuinely have no user-scoped tx (e.g. the pre-tx
/// signup-failure branch) stay on [`record_wizard`] / [`record_auth`],
/// which run against the pool.
pub async fn record_wizard_in_tx(
    conn: &mut PgConnection,
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
    .execute(conn)
    .await?;
    Ok(())
}

/// INSERT an audit row for a wizard / grant action against the pool.
/// Retained for the small set of handlers that have no user-scoped tx
/// at the point of the audit write (signup failure, verify-email
/// failure pre-tx). Handlers WITH a tx must use
/// [`record_wizard_in_tx`] so the audit row is atomic with the
/// mutation (T25 / S1).
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
