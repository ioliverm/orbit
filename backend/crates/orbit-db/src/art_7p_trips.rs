//! `art_7p_trips` repository (Slice 2 T20).
//!
//! One row per professional trip abroad that the user declares for the
//! Art. 7.p partial-year exemption (AC-5.1..AC-5.3). Every query routes
//! through a `&mut Tx` borrowed via [`crate::Tx::for_user`], so RLS scopes
//! the visible row set to the owner (SEC-020..023).
//!
//! # `eligibility_criteria` JSONB shape
//!
//! Per ADR-016 §9.1 the five-criterion eligibility checklist is stored as a
//! JSONB object. The DB only enforces `jsonb_typeof = 'object'`; the handler
//! layer in `orbit-api` (T21) validates the five-keys shape + the
//! `true | false | null` value type before writing (SEC-163). At the repo
//! boundary we carry the object as a [`serde_json::Value`] — deliberately
//! unchecked — so the handler tests can round-trip arbitrary well-formed
//! objects without this module interpreting them.
//!
//! Traces to:
//!   - ADR-016 §1 (art_7p_trips DDL, RLS, touch_updated_at).
//!   - ADR-016 §9.1 (JSONB shape decision).
//!   - docs/requirements/slice-2-acceptance-criteria.md §5.

use chrono::NaiveDate;
use sqlx::Row;
use uuid::Uuid;

use crate::Tx;

/// An `art_7p_trips` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Art7pTrip {
    pub id: Uuid,
    pub user_id: Uuid,
    /// ISO 3166-1 alpha-2 country code (DDL CHECK `length = 2`).
    pub destination_country: String,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub employer_paid: bool,
    pub purpose: Option<String>,
    /// Opaque JSONB object. Handler enforces the five-keys shape per
    /// ADR-016 §9.1; the DB only enforces object-ness.
    pub eligibility_criteria: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Input shape for [`create`] / [`update`].
#[derive(Debug, Clone)]
pub struct Art7pTripForm {
    pub destination_country: String,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub employer_paid: bool,
    pub purpose: Option<String>,
    pub eligibility_criteria: serde_json::Value,
}

/// Per-criterion count row returned by [`annual_summary`].
///
/// Each field counts trips in the requested year whose
/// `eligibility_criteria.<key>` is `true`. `false` and `null` answers do not
/// contribute — the field is a lower bound on "user self-asserted yes" for
/// that criterion, which is what the Slice-2 annual-cap tracker panel
/// (AC-5.1.3) displays alongside the non-monetary day-count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Art7pAnnualSummary {
    /// The year the summary covers (inclusive of trips whose range
    /// intersects this year — a trip that spans year boundaries counts).
    pub year: i32,
    /// Total trips in `year` for the caller.
    pub trip_count: i64,
    /// Sum of inclusive day-counts per trip, clamped to `year`'s bounds.
    pub day_count_declared: i64,
    /// Subset of `trip_count` where `employer_paid = true`.
    pub employer_paid_trip_count: i64,
    /// Per-criterion "yes" counts. Each corresponds to one of the five
    /// well-known JSONB keys (ADR-016 §1 `COMMENT ON COLUMN`).
    pub criterion_services_outside_spain_yes: i64,
    pub criterion_non_spanish_employer_yes: i64,
    pub criterion_not_tax_haven_yes: i64,
    pub criterion_no_double_exemption_yes: i64,
    pub criterion_within_annual_cap_yes: i64,
}

/// INSERT a new trip owned by `user_id`.
pub async fn create(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    form: &Art7pTripForm,
) -> Result<Art7pTrip, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO art_7p_trips (
            user_id, destination_country,
            from_date, to_date,
            employer_paid, purpose, eligibility_criteria
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, user_id, destination_country,
                  from_date, to_date,
                  employer_paid, purpose, eligibility_criteria,
                  created_at, updated_at
        "#,
    )
    .bind(user_id)
    .bind(&form.destination_country)
    .bind(form.from_date)
    .bind(form.to_date)
    .bind(form.employer_paid)
    .bind(form.purpose.as_deref())
    .bind(&form.eligibility_criteria)
    .fetch_one(tx.as_executor())
    .await?;

    row_to_trip(&row)
}

/// List every trip owned by `user_id`, newest first by `from_date` then
/// `created_at` (tie-break across same-start-date trips).
pub async fn list(tx: &mut Tx<'_>, user_id: Uuid) -> Result<Vec<Art7pTrip>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, user_id, destination_country,
               from_date, to_date,
               employer_paid, purpose, eligibility_criteria,
               created_at, updated_at
          FROM art_7p_trips
         WHERE user_id = $1
         ORDER BY from_date DESC, created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_trip).collect()
}

/// Fetch a single trip by id, scoped to the owner. Returns `None` when the
/// row does not exist or is not owned by `user_id` (AC-5.3.4: 404 not 403).
pub async fn get_by_id(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    trip_id: Uuid,
) -> Result<Option<Art7pTrip>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, user_id, destination_country,
               from_date, to_date,
               employer_paid, purpose, eligibility_criteria,
               created_at, updated_at
          FROM art_7p_trips
         WHERE id = $1 AND user_id = $2
        "#,
    )
    .bind(trip_id)
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_trip).transpose()
}

/// Full-replace UPDATE on a trip. Returns `sqlx::Error::RowNotFound` if no
/// row matched (bad id or RLS-filtered).
pub async fn update(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    trip_id: Uuid,
    form: &Art7pTripForm,
) -> Result<Art7pTrip, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE art_7p_trips
           SET destination_country  = $3,
               from_date            = $4,
               to_date              = $5,
               employer_paid        = $6,
               purpose              = $7,
               eligibility_criteria = $8
         WHERE id = $1 AND user_id = $2
     RETURNING id, user_id, destination_country,
               from_date, to_date,
               employer_paid, purpose, eligibility_criteria,
               created_at, updated_at
        "#,
    )
    .bind(trip_id)
    .bind(user_id)
    .bind(&form.destination_country)
    .bind(form.from_date)
    .bind(form.to_date)
    .bind(form.employer_paid)
    .bind(form.purpose.as_deref())
    .bind(&form.eligibility_criteria)
    .fetch_one(tx.as_executor())
    .await?;

    row_to_trip(&row)
}

/// DELETE a trip. Returns `Ok(())` whether or not a row matched — RLS
/// filters cross-tenant rows and the handler distinguishes "not found" from
/// "not owned" via a prior [`get_by_id`] (AC-5.3.4).
pub async fn delete(tx: &mut Tx<'_>, user_id: Uuid, trip_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM art_7p_trips WHERE id = $1 AND user_id = $2")
        .bind(trip_id)
        .bind(user_id)
        .execute(tx.as_executor())
        .await?;
    Ok(())
}

/// Compute the annual-cap tracker summary for `user_id` × `year` (AC-5.1.3).
///
/// "Day count declared" is the sum, across every trip whose `[from_date,
/// to_date]` interval intersects `year`, of inclusive days clamped to the
/// `[year-01-01, year-12-31]` window. Cross-year trips contribute only their
/// in-year portion, so a trip 2025-12-28..2026-01-05 contributes 4 days to
/// 2025 (Dec 28, 29, 30, 31) and 5 days to 2026 (Jan 1..5). Endpoint
/// inclusivity matches AC-5.1.3's "sum of trip days, inclusive of both
/// endpoints".
///
/// Per-criterion yes-counts read through `eligibility_criteria ->> 'key' =
/// 'true'`, so a `false` or missing answer does not contribute. This
/// matches what the UI's `(N/5)` chip is built from.
pub async fn annual_summary(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    year: i32,
) -> Result<Art7pAnnualSummary, sqlx::Error> {
    // One round-trip aggregate. Clamp `from_date` up to Jan 1, `to_date`
    // down to Dec 31, and take the inclusive day count as
    // `(clamped_to - clamped_from)::int + 1`. Rows whose range does not
    // intersect the year are excluded by the WHERE clause.
    let row = sqlx::query(
        r#"
        WITH window_bounds AS (
            SELECT
                make_date($2::int, 1, 1)  AS year_start,
                make_date($2::int, 12, 31) AS year_end
        )
        SELECT
            COALESCE(COUNT(*), 0)::bigint AS trip_count,
            COALESCE(SUM(
                (LEAST(t.to_date, wb.year_end)
                 - GREATEST(t.from_date, wb.year_start))::bigint + 1
            ), 0)::bigint AS day_count_declared,
            COALESCE(SUM(CASE WHEN t.employer_paid THEN 1 ELSE 0 END), 0)::bigint
                AS employer_paid_trip_count,
            COALESCE(SUM(CASE WHEN t.eligibility_criteria ->> 'services_outside_spain' = 'true' THEN 1 ELSE 0 END), 0)::bigint
                AS criterion_services_outside_spain_yes,
            COALESCE(SUM(CASE WHEN t.eligibility_criteria ->> 'non_spanish_employer' = 'true' THEN 1 ELSE 0 END), 0)::bigint
                AS criterion_non_spanish_employer_yes,
            COALESCE(SUM(CASE WHEN t.eligibility_criteria ->> 'not_tax_haven' = 'true' THEN 1 ELSE 0 END), 0)::bigint
                AS criterion_not_tax_haven_yes,
            COALESCE(SUM(CASE WHEN t.eligibility_criteria ->> 'no_double_exemption' = 'true' THEN 1 ELSE 0 END), 0)::bigint
                AS criterion_no_double_exemption_yes,
            COALESCE(SUM(CASE WHEN t.eligibility_criteria ->> 'within_annual_cap' = 'true' THEN 1 ELSE 0 END), 0)::bigint
                AS criterion_within_annual_cap_yes
          FROM art_7p_trips t
          CROSS JOIN window_bounds wb
         WHERE t.user_id = $1
           AND t.from_date <= wb.year_end
           AND t.to_date   >= wb.year_start
        "#,
    )
    .bind(user_id)
    .bind(year)
    .fetch_one(tx.as_executor())
    .await?;

    Ok(Art7pAnnualSummary {
        year,
        trip_count: row.try_get("trip_count")?,
        day_count_declared: row.try_get("day_count_declared")?,
        employer_paid_trip_count: row.try_get("employer_paid_trip_count")?,
        criterion_services_outside_spain_yes: row
            .try_get("criterion_services_outside_spain_yes")?,
        criterion_non_spanish_employer_yes: row.try_get("criterion_non_spanish_employer_yes")?,
        criterion_not_tax_haven_yes: row.try_get("criterion_not_tax_haven_yes")?,
        criterion_no_double_exemption_yes: row.try_get("criterion_no_double_exemption_yes")?,
        criterion_within_annual_cap_yes: row.try_get("criterion_within_annual_cap_yes")?,
    })
}

fn row_to_trip(row: &sqlx::postgres::PgRow) -> Result<Art7pTrip, sqlx::Error> {
    Ok(Art7pTrip {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        destination_country: row.try_get("destination_country")?,
        from_date: row.try_get("from_date")?,
        to_date: row.try_get("to_date")?,
        employer_paid: row.try_get("employer_paid")?,
        purpose: row.try_get("purpose")?,
        eligibility_criteria: row.try_get("eligibility_criteria")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
