//! `modelo_720_user_inputs` repository (Slice 2 T20).
//!
//! Time-series store for the two user-entered Modelo 720 category totals
//! (`bank_accounts`, `real_estate`). Close-and-create shape mirrors
//! [`crate::residency`] per ADR-016 §1 (AC-6.2.2). Every query routes through
//! a `&mut Tx` borrowed via [`crate::Tx::for_user`], so RLS scopes the
//! visible row set to the owner (SEC-020..023). The partial unique index
//! `(user_id, category) WHERE to_date IS NULL` guarantees at most one open
//! row per (user, category).
//!
//! # `amount_eur` decimal bridging
//!
//! `amount_eur` is `NUMERIC(20,2)`. Consistent with `grants.strike_amount`,
//! the Slice-2 repo does not introduce a new decimal dep; the column is
//! carried across the crate boundary as `String` via `::numeric` casts on
//! write and `::text` casts on read. The handler (T21) parses the submitted
//! amount through a locale-aware validator before it reaches this module.
//!
//! Traces to:
//!   - ADR-016 §1 (modelo_720_user_inputs DDL, partial unique index,
//!     RLS tenant_isolation).
//!   - ADR-016 §3 (handler shape for POST /modelo-720-inputs — same-day
//!     short-circuit; AC-6.2.3).
//!   - docs/requirements/slice-2-acceptance-criteria.md §6.

use chrono::NaiveDate;
use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// A `modelo_720_user_inputs` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modelo720UserInput {
    pub id: Uuid,
    pub user_id: Uuid,
    /// `bank_accounts` or `real_estate` (DDL CHECK).
    pub category: String,
    /// Decimal passthrough (`NUMERIC(20,2)::text`). Always `>= 0`.
    pub amount_eur: String,
    pub reference_date: NaiveDate,
    pub from_date: NaiveDate,
    /// `None` means "still the current period" for this category.
    pub to_date: Option<NaiveDate>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Input for [`create_or_upsert_same_day`].
///
/// `today` is the calendar day on which this save lands (server-TZ per
/// AC-6.2.3); we take it as a parameter rather than reading `CURRENT_DATE`
/// so handler tests can pin the value.
#[derive(Debug, Clone)]
pub struct Modelo720UpsertForm {
    pub category: String,
    pub amount_eur: String,
    pub reference_date: NaiveDate,
    pub today: NaiveDate,
}

/// Outcome of [`create_or_upsert_same_day`] — the caller (T21 handler) uses
/// this to decide whether to write an audit row (AC-6.2.5 no-op path) and
/// whether to return `201 Created` vs `200 OK`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// No prior row for `(user, category)` existed; a brand-new row is now
    /// the open one. 201 semantics.
    Inserted(Modelo720UserInput),
    /// A prior open row existed but its `from_date != today`; it was closed
    /// with `to_date = today` and a new open row was inserted for today.
    /// 200 semantics (the user's intent was a successor value).
    ClosedAndCreated(Modelo720UserInput),
    /// A prior open row existed with `from_date = today` and a different
    /// `amount_eur`; the row was updated in place (no 1-day span row
    /// materializes). 200 semantics.
    UpdatedSameDay(Modelo720UserInput),
    /// The incoming `amount_eur` equals the currently-open row's value.
    /// Per AC-6.2.5 the handler writes no audit row; the caller returns
    /// the existing row unchanged with 200 semantics.
    NoOp(Modelo720UserInput),
}

impl UpsertOutcome {
    /// Convenience: the row the handler returns in the response body.
    pub fn row(&self) -> &Modelo720UserInput {
        match self {
            UpsertOutcome::Inserted(r)
            | UpsertOutcome::ClosedAndCreated(r)
            | UpsertOutcome::UpdatedSameDay(r)
            | UpsertOutcome::NoOp(r) => r,
        }
    }
}

/// Transactional close-and-create for one (user, category) per ADR-016 §3.
///
/// Decision tree (in one DB transaction):
///   1. Look up the currently-open row for `(user_id, form.category)`.
///   2. If none: INSERT a fresh row with `from_date = today` and return
///      `Inserted`.
///   3. If one exists and `amount_eur` matches the incoming value:
///      return `NoOp` — the caller skips the audit row (AC-6.2.5).
///   4. If one exists and `from_date = today`: UPDATE `amount_eur` +
///      `reference_date` in place and return `UpdatedSameDay` — avoids the
///      1-day open-then-closed span on double-save (AC-6.2.3 clarified).
///   5. Otherwise: CLOSE the prior row (`to_date = today`) + INSERT the
///      successor row (`from_date = today`), return `ClosedAndCreated`.
///
/// The partial unique index `(user_id, category) WHERE to_date IS NULL`
/// guards the invariant: if two calls race, the second's INSERT fails with
/// a `unique_violation` and the caller's tx rolls back. Handlers serialize
/// per-user by going through `Tx::for_user` which BEGINs its own
/// transaction; one of the two wins, the other sees the winner's row on
/// retry and takes the `UpdatedSameDay` or `NoOp` branch.
pub async fn create_or_upsert_same_day(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    form: &Modelo720UpsertForm,
) -> Result<UpsertOutcome, sqlx::Error> {
    // Step 1: read the current open row for this category, if any. The
    // amount comes back as a string so the equality check below does not
    // drift through a float round-trip.
    let current = sqlx::query(
        r#"
        SELECT id, user_id, category,
               amount_eur::text AS amount_eur_text,
               reference_date, from_date, to_date, created_at
          FROM modelo_720_user_inputs
         WHERE user_id = $1 AND category = $2 AND to_date IS NULL
         LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(&form.category)
    .fetch_optional(tx.as_executor())
    .await?;

    let mut closed_prior = false;
    if let Some(existing_row) = current {
        let existing = row_to_input(&existing_row)?;

        // Step 2: idempotent no-op when amount is unchanged (AC-6.2.5).
        // Compare through NUMERIC round-trip rather than raw-string equality
        // so callers that submit "100" vs "100.00" vs "100.0" all match
        // against the stored 2-decimal value.
        if amounts_equal_as_numeric(tx, &existing.amount_eur, &form.amount_eur).await? {
            return Ok(UpsertOutcome::NoOp(existing));
        }

        // Step 3: same-day in-place update (AC-6.2.3 clarified).
        if existing.from_date == form.today {
            let row = sqlx::query(
                r#"
                UPDATE modelo_720_user_inputs
                   SET amount_eur     = $3::numeric,
                       reference_date = $4
                 WHERE id = $1 AND user_id = $2
             RETURNING id, user_id, category,
                       amount_eur::text AS amount_eur_text,
                       reference_date, from_date, to_date, created_at
                "#,
            )
            .bind(existing.id)
            .bind(user_id)
            .bind(&form.amount_eur)
            .bind(form.reference_date)
            .fetch_one(tx.as_executor())
            .await?;
            return Ok(UpsertOutcome::UpdatedSameDay(row_to_input(&row)?));
        }

        // Step 4: classic close-and-create. Close the prior row first; the
        // partial unique index now has no `to_date IS NULL` row for this
        // (user, category), so the successor INSERT is free to land.
        sqlx::query(
            r#"
            UPDATE modelo_720_user_inputs
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

    // Step 5 (fall-through from no-prior-row or just-closed-row): INSERT
    // the new open row. `from_date = today` matches the spec; the partial
    // unique index now admits exactly one such row per (user, category).
    let row = sqlx::query(
        r#"
        INSERT INTO modelo_720_user_inputs (
            user_id, category, amount_eur, reference_date, from_date, to_date
        )
        VALUES ($1, $2, $3::numeric, $4, $5, NULL)
        RETURNING id, user_id, category,
                  amount_eur::text AS amount_eur_text,
                  reference_date, from_date, to_date, created_at
        "#,
    )
    .bind(user_id)
    .bind(&form.category)
    .bind(&form.amount_eur)
    .bind(form.reference_date)
    .bind(form.today)
    .fetch_one(tx.as_executor())
    .await?;

    let input = row_to_input(&row)?;
    if closed_prior {
        Ok(UpsertOutcome::ClosedAndCreated(input))
    } else {
        Ok(UpsertOutcome::Inserted(input))
    }
}

/// The currently-open row for `(user_id, category)`, or `None` if no row
/// has ever been saved for this category.
pub async fn current(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    category: &str,
) -> Result<Option<Modelo720UserInput>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, user_id, category,
               amount_eur::text AS amount_eur_text,
               reference_date, from_date, to_date, created_at
          FROM modelo_720_user_inputs
         WHERE user_id = $1 AND category = $2 AND to_date IS NULL
         LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(category)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_input).transpose()
}

/// Full history for `user_id`, ordered by `from_date DESC` (matches the
/// scan-pattern index `modelo_720_user_inputs_user_category_from_idx`).
/// Both categories are returned; the caller partitions client-side.
pub async fn history(
    tx: &mut Tx<'_>,
    user_id: Uuid,
) -> Result<Vec<Modelo720UserInput>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, user_id, category,
               amount_eur::text AS amount_eur_text,
               reference_date, from_date, to_date, created_at
          FROM modelo_720_user_inputs
         WHERE user_id = $1
         ORDER BY category ASC, from_date DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_input).collect()
}

/// Compare two NUMERIC-shaped strings for equality. Delegates the parse to
/// Postgres via `$1::numeric = $2::numeric` so "100" == "100.00" == "100.0"
/// (all normalize to the same NUMERIC(20,2) value that the column stores).
/// Returns `false` if either side fails to parse — the handler's validator
/// is expected to have rejected malformed input before this path.
async fn amounts_equal_as_numeric(tx: &mut Tx<'_>, a: &str, b: &str) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT ($1::numeric = $2::numeric) AS eq")
        .bind(a)
        .bind(b)
        .fetch_one(tx.as_executor())
        .await?;
    row.try_get::<bool, _>("eq")
}

fn row_to_input(row: &sqlx::postgres::PgRow) -> Result<Modelo720UserInput, sqlx::Error> {
    Ok(Modelo720UserInput {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        category: row.try_get("category")?,
        amount_eur: row.try_get("amount_eur_text")?,
        reference_date: row.try_get("reference_date")?,
        from_date: row.try_get("from_date")?,
        to_date: row.try_get("to_date")?,
        created_at: row.try_get("created_at")?,
    })
}
