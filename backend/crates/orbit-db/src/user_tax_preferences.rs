//! `user_tax_preferences` repository (Slice 3b T37).
//!
//! Time-series sidecar storing the user's tax-residency country and
//! (for Spain-like regimes) the `rendimiento_del_trabajo_percent`
//! default plus the `sell_to_cover_enabled` toggle. Close-and-create
//! shape mirrors [`crate::modelo_720_inputs`] per ADR-018 §1 +
//! ADR-016 §1 (AC-4.4.*). Every query routes through a `&mut Tx`
//! borrowed via [`crate::Tx::for_user`], so RLS scopes the visible row
//! set to the owner (SEC-020..023). The partial unique index
//! `(user_id) WHERE to_date IS NULL` guarantees at most one open row
//! per user.
//!
//! # `rendimiento_del_trabajo_percent` decimal bridging
//!
//! The column is `NUMERIC(5,4)` storing a fraction in `[0, 1]`. To
//! avoid introducing a new decimal dependency, the column is carried
//! across the crate boundary as `Option<String>` via `::numeric` casts
//! on write and `::text` casts on read — consistent with
//! `modelo_720_user_inputs.amount_eur` and `vesting_events.fmv_at_vest`.
//! The handler (T38) parses the submitted percent through a
//! locale-aware validator before it reaches this module.
//!
//! Traces to:
//!   - ADR-018 §1 (user_tax_preferences DDL, partial unique index,
//!     RLS tenant_isolation).
//!   - ADR-018 §3 (handler shape for POST /user-tax-preferences —
//!     same-day short-circuit; AC-4.4.*).
//!   - docs/requirements/slice-3b-acceptance-criteria.md §4.

use chrono::NaiveDate;
use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// A `user_tax_preferences` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserTaxPreference {
    pub id: Uuid,
    pub user_id: Uuid,
    /// ISO-3166 alpha-2 country code, uppercased (DDL CHECK).
    pub country_iso2: String,
    /// Fraction in `[0, 1]` carried as a `NUMERIC(5,4)::text` string
    /// (e.g., `"0.4500"` for 45 %). `None` when the field is hidden
    /// for the selected country (AC-4.2.2) or when the user saved a
    /// Spain row without filling it in (AC-4.2.3).
    pub rendimiento_del_trabajo_percent: Option<String>,
    /// User's sell-to-cover default (AC-4.3). `NOT NULL` in the DDL;
    /// every saved row commits to a definite boolean.
    pub sell_to_cover_enabled: bool,
    pub from_date: NaiveDate,
    /// `None` means "still the currently-open row" for this user.
    pub to_date: Option<NaiveDate>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Input for [`create_or_upsert_same_day`].
///
/// `today` is the calendar day on which this save lands (server-TZ per
/// AC-4.4.*); we take it as a parameter rather than reading
/// `CURRENT_DATE` so handler tests can pin the value.
#[derive(Debug, Clone)]
pub struct UserTaxPreferenceUpsertForm {
    pub country_iso2: String,
    pub rendimiento_del_trabajo_percent: Option<String>,
    pub sell_to_cover_enabled: bool,
    pub today: NaiveDate,
}

/// Outcome of [`create_or_upsert_same_day`] — the caller (T38 handler)
/// uses this to decide whether to write an audit row (AC-4.6.1 no-op
/// path) and what `outcome` string to return in the response envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// No prior row for this user existed; a brand-new row is now the
    /// open one.
    Inserted(UserTaxPreference),
    /// A prior open row existed but its `from_date != today`; it was
    /// closed with `to_date = today` and a new open row was inserted
    /// for today.
    ClosedAndCreated(UserTaxPreference),
    /// A prior open row existed with `from_date = today` and at least
    /// one field differing; the row was updated in place (no 1-day
    /// span row materializes).
    UpdatedSameDay(UserTaxPreference),
    /// The incoming values equal the currently-open row's values.
    /// Per AC-4.6.1 the handler writes no audit row; the caller
    /// returns the existing row unchanged.
    NoOp(UserTaxPreference),
}

impl UpsertOutcome {
    /// Convenience: the row the handler returns in the response body.
    pub fn row(&self) -> &UserTaxPreference {
        match self {
            UpsertOutcome::Inserted(r)
            | UpsertOutcome::ClosedAndCreated(r)
            | UpsertOutcome::UpdatedSameDay(r)
            | UpsertOutcome::NoOp(r) => r,
        }
    }
}

/// Transactional close-and-create for one user per ADR-018 §1 + §3.
///
/// Decision tree (in one DB transaction):
///   1. Look up the currently-open row for `user_id`.
///   2. If none: INSERT a fresh row with `from_date = today` and
///      return `Inserted`.
///   3. If one exists and every submitted field matches: return
///      `NoOp` — the caller skips the audit row (AC-4.6.1).
///   4. If one exists and `from_date = today`: UPDATE the three
///      fields in place and return `UpdatedSameDay` — avoids the
///      1-day open-then-closed span on double-save.
///   5. Otherwise: CLOSE the prior row (`to_date = today`) + INSERT
///      the successor row (`from_date = today`), return
///      `ClosedAndCreated`.
///
/// The partial unique index `(user_id) WHERE to_date IS NULL` guards
/// the invariant: if two calls race, the second's INSERT fails with
/// a `unique_violation` and the caller's tx rolls back. Handlers
/// serialize per-user by going through `Tx::for_user` which BEGINs
/// its own transaction; one of the two wins, the other sees the
/// winner's row on retry and takes the `UpdatedSameDay` or `NoOp`
/// branch.
pub async fn create_or_upsert_same_day(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    form: &UserTaxPreferenceUpsertForm,
) -> Result<UpsertOutcome, sqlx::Error> {
    // Step 1: read the current open row for this user, if any.
    let current = sqlx::query(
        r#"
        SELECT id, user_id, country_iso2,
               rendimiento_del_trabajo_percent::text AS percent_text,
               sell_to_cover_enabled,
               from_date, to_date, created_at, updated_at
          FROM user_tax_preferences
         WHERE user_id = $1 AND to_date IS NULL
         LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;

    let mut closed_prior = false;
    if let Some(existing_row) = current {
        let existing = row_to_preference(&existing_row)?;

        // Step 2: idempotent no-op when every field matches (AC-4.6.1).
        // Compare the percent as NUMERIC so "0.45" == "0.4500" == "0.450"
        // all round-trip to the same stored value — the DB's normalized
        // representation is the arbiter.
        let percent_matches = percents_equal_as_numeric(
            tx,
            existing.rendimiento_del_trabajo_percent.as_deref(),
            form.rendimiento_del_trabajo_percent.as_deref(),
        )
        .await?;
        if existing.country_iso2 == form.country_iso2
            && existing.sell_to_cover_enabled == form.sell_to_cover_enabled
            && percent_matches
        {
            return Ok(UpsertOutcome::NoOp(existing));
        }

        // Step 3: same-day in-place update.
        if existing.from_date == form.today {
            let row = sqlx::query(
                r#"
                UPDATE user_tax_preferences
                   SET country_iso2                    = $3,
                       rendimiento_del_trabajo_percent = $4::numeric,
                       sell_to_cover_enabled           = $5
                 WHERE id = $1 AND user_id = $2
             RETURNING id, user_id, country_iso2,
                       rendimiento_del_trabajo_percent::text AS percent_text,
                       sell_to_cover_enabled,
                       from_date, to_date, created_at, updated_at
                "#,
            )
            .bind(existing.id)
            .bind(user_id)
            .bind(&form.country_iso2)
            .bind(form.rendimiento_del_trabajo_percent.as_deref())
            .bind(form.sell_to_cover_enabled)
            .fetch_one(tx.as_executor())
            .await?;
            return Ok(UpsertOutcome::UpdatedSameDay(row_to_preference(&row)?));
        }

        // Step 4: classic close-and-create. Close the prior row first;
        // the partial unique index now admits a successor INSERT.
        sqlx::query(
            r#"
            UPDATE user_tax_preferences
               SET to_date = $3
             WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(existing.id)
        .bind(user_id)
        .bind(form.today)
        .execute(tx.as_executor())
        .await?;
        closed_prior = true;
    }

    // Step 5 (fall-through): INSERT the new open row. `from_date = today`
    // matches the spec; the partial unique index now admits exactly one
    // such row per user.
    let row = sqlx::query(
        r#"
        INSERT INTO user_tax_preferences (
            user_id, country_iso2, rendimiento_del_trabajo_percent,
            sell_to_cover_enabled, from_date, to_date
        )
        VALUES ($1, $2, $3::numeric, $4, $5, NULL)
        RETURNING id, user_id, country_iso2,
                  rendimiento_del_trabajo_percent::text AS percent_text,
                  sell_to_cover_enabled,
                  from_date, to_date, created_at, updated_at
        "#,
    )
    .bind(user_id)
    .bind(&form.country_iso2)
    .bind(form.rendimiento_del_trabajo_percent.as_deref())
    .bind(form.sell_to_cover_enabled)
    .bind(form.today)
    .fetch_one(tx.as_executor())
    .await?;

    let pref = row_to_preference(&row)?;
    if closed_prior {
        Ok(UpsertOutcome::ClosedAndCreated(pref))
    } else {
        Ok(UpsertOutcome::Inserted(pref))
    }
}

/// The currently-open row for `user_id`, or `None` if no row has ever
/// been saved. Used by the Profile read path and by the default-
/// sourcing logic in T38.
pub async fn current(
    tx: &mut Tx<'_>,
    user_id: Uuid,
) -> Result<Option<UserTaxPreference>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, user_id, country_iso2,
               rendimiento_del_trabajo_percent::text AS percent_text,
               sell_to_cover_enabled,
               from_date, to_date, created_at, updated_at
          FROM user_tax_preferences
         WHERE user_id = $1 AND to_date IS NULL
         LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_preference).transpose()
}

/// Full history for `user_id`, ordered by `from_date DESC` (matches
/// the scan index `user_tax_preferences_user_from_date_idx`). Open and
/// closed rows are both returned; the Profile UI client-side filters
/// the open row out of the history table (AC-4.5.1).
pub async fn history(
    tx: &mut Tx<'_>,
    user_id: Uuid,
) -> Result<Vec<UserTaxPreference>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, user_id, country_iso2,
               rendimiento_del_trabajo_percent::text AS percent_text,
               sell_to_cover_enabled,
               from_date, to_date, created_at, updated_at
          FROM user_tax_preferences
         WHERE user_id = $1
         ORDER BY from_date DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_preference).collect()
}

/// The preference row in effect at a given `date` — the row where
/// `from_date <= date < COALESCE(to_date, 'infinity')`. Returns `None`
/// when `date` is before the earliest `from_date` for this user (the
/// user had no preferences on record as of that date).
///
/// Used by the Slice-3b default-sourcing logic in the T38 handler
/// (the "most recent intent wins" policy per AC-9.8) and by any
/// future Slice-4 path that resolves a historical preference for
/// retroactive tax math.
pub async fn resolve_at_date(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    date: NaiveDate,
) -> Result<Option<UserTaxPreference>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, user_id, country_iso2,
               rendimiento_del_trabajo_percent::text AS percent_text,
               sell_to_cover_enabled,
               from_date, to_date, created_at, updated_at
          FROM user_tax_preferences
         WHERE user_id = $1
           AND from_date <= $2
           AND (to_date IS NULL OR to_date > $2)
         ORDER BY from_date DESC
         LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(date)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_preference).transpose()
}

/// Compare two optional NUMERIC-shaped percent strings for equality.
/// Both NULL is equal; mixed NULL/non-NULL is not; non-NULL values
/// compare through the Postgres `$1::numeric = $2::numeric` cast so
/// "0.45" == "0.4500". Returns `false` if either side fails to parse
/// — the handler's validator is expected to have rejected malformed
/// input before this path.
async fn percents_equal_as_numeric(
    tx: &mut Tx<'_>,
    a: Option<&str>,
    b: Option<&str>,
) -> Result<bool, sqlx::Error> {
    match (a, b) {
        (None, None) => Ok(true),
        (None, Some(_)) | (Some(_), None) => Ok(false),
        (Some(a), Some(b)) => {
            let row = sqlx::query("SELECT ($1::numeric = $2::numeric) AS eq")
                .bind(a)
                .bind(b)
                .fetch_one(tx.as_executor())
                .await?;
            row.try_get::<bool, _>("eq")
        }
    }
}

fn row_to_preference(row: &sqlx::postgres::PgRow) -> Result<UserTaxPreference, sqlx::Error> {
    Ok(UserTaxPreference {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        country_iso2: row.try_get("country_iso2")?,
        rendimiento_del_trabajo_percent: row.try_get("percent_text")?,
        sell_to_cover_enabled: row.try_get("sell_to_cover_enabled")?,
        from_date: row.try_get("from_date")?,
        to_date: row.try_get("to_date")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
