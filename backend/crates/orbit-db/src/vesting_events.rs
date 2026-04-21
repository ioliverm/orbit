//! `vesting_events` repository (Slice 1 T12; extended in Slice 3 T28).
//!
//! Every query routes through a `&mut Tx` borrowed via
//! [`crate::Tx::for_user`], so RLS scopes the visible row set to the owner
//! (SEC-020..023). Same `NUMERIC(20,4)` ↔ [`Shares`] bridging trick as in
//! [`crate::grants`]: multiply by 10_000 and cast to `bigint` on read, and
//! `$n::numeric / 10000` on write.
//!
//! ADR-014 §1 materializes this table rather than serving it as a view, so
//! that dashboard renders stay a cheap `SELECT *` per grant. Callers
//! regenerate the rows on grant create/update via
//! [`replace_for_grant`], which is a DELETE + batch INSERT inside the
//! transaction to keep the table consistent with the input `Grant`.
//!
//! # Slice 3 extensions (ADR-017 §1 + §2)
//!
//! Five columns added by `migrations/20260523120000_slice_3.sql`:
//!
//!   * `fmv_at_vest` (`NUMERIC(20,6)`, nullable) — per-vest FMV capture.
//!   * `fmv_currency` (`TEXT`, nullable) — paired with `fmv_at_vest` via
//!     the `fmv_pair_coherent` CHECK.
//!   * `is_user_override` (`BOOLEAN NOT NULL DEFAULT false`) — set by
//!     `apply_override` + `bulk_fill_fmv`; the `override_flag_coherent`
//!     CHECK enforces `is_user_override = (overridden_at IS NOT NULL)`.
//!   * `overridden_at` (`TIMESTAMPTZ`, nullable) — handler-owned
//!     timestamp; the `touch_updated_at` trigger does NOT touch it.
//!   * `updated_at` (`TIMESTAMPTZ NOT NULL DEFAULT now()`) — maintained
//!     by the `vesting_events_touch_updated_at` trigger on every write;
//!     backs optimistic concurrency per AC-10.5.
//!
//! The slice-3 repo surface (`apply_override`, `clear_override`,
//! `bulk_fill_fmv`, `list_for_grant_with_overrides`,
//! `count_override_rows`, `sum_override_shares`) implements the write
//! paths for AC-8.2..AC-8.9. The derivation-algorithm extension that
//! preserves overrides across grant-param changes is T29's scope.
//!
//! Traces to:
//!   - ADR-014 §1 (vesting_events DDL, UNIQUE on (grant_id, vest_date)).
//!   - ADR-017 §1 (Slice-3 additive columns + cross-field CHECKs).
//!   - docs/requirements/slice-1-acceptance-criteria.md §4.3.
//!   - docs/requirements/slice-3-acceptance-criteria.md §8 (AC-8.2..AC-8.9).

use chrono::NaiveDate;
use orbit_core::{derive_vesting_events, Cadence, GrantInput, Shares, VestingEvent, VestingState};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

use crate::Tx;

/// A `vesting_events` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VestingEventRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub grant_id: Uuid,
    pub vest_date: NaiveDate,
    pub shares_vested_this_event: Shares,
    pub cumulative_shares_vested: Shares,
    pub state: VestingState,
    pub computed_at: chrono::DateTime<chrono::Utc>,
    /// Slice 3 addition. Decimal passthrough (`NUMERIC(20,6)::text`).
    /// `None` until the user (or a bulk-fill) captures the FMV.
    pub fmv_at_vest: Option<String>,
    /// Slice 3 addition. `USD | EUR | GBP`. Paired with `fmv_at_vest`
    /// per the `fmv_pair_coherent` CHECK constraint.
    pub fmv_currency: Option<String>,
    /// Slice 3 addition. `true` iff the row has been manually edited
    /// (vest date, shares, or FMV). Gated at write-time by the handler;
    /// the DB enforces coherence with `overridden_at` via the
    /// `override_flag_coherent` CHECK.
    pub is_user_override: bool,
    /// Slice 3 addition. `Some(t)` iff `is_user_override = true`.
    pub overridden_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Slice 3 addition. Maintained by the
    /// `vesting_events_touch_updated_at` trigger; used as the
    /// optimistic-concurrency token in [`apply_override`] (AC-10.5).
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// The patch supplied on a PUT `/grants/:grantId/vesting-events/:eventId`
/// (AC-8.2 + AC-8.3). Every field is optional — the handler layer
/// validates per-field rules and composes the patch before calling
/// [`apply_override`].
#[derive(Debug, Clone, Default)]
pub struct VestingEventOverridePatch {
    pub vest_date: Option<NaiveDate>,
    pub shares_vested_this_event: Option<Shares>,
    /// `Some(Some(s))` writes a new FMV; `Some(None)` clears it (the
    /// handler-level blank-save path per AC-8.2.4 / AC-8.3.3); `None`
    /// leaves the column untouched. `fmv_currency` tracks this exactly
    /// per the `fmv_pair_coherent` CHECK.
    pub fmv_at_vest: Option<Option<String>>,
    pub fmv_currency: Option<Option<String>>,
}

/// Outcome of [`apply_override`] — the handler uses this to pick
/// between a 200-with-row response and the 409 stale-state response
/// per AC-10.5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideOutcome {
    /// The patch landed. `updated_at` on the returned row advances.
    Applied(VestingEventRow),
    /// The `expected_updated_at` predicate did not match the DB row's
    /// current `updated_at`. The handler returns 409 with code
    /// `resource.stale_client_state`.
    Conflict,
}

/// Result of [`bulk_fill_fmv`] (Q4, AC-8.6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkFillResult {
    /// Rows that had `fmv_at_vest IS NULL` before the call and now
    /// carry the input FMV + currency. The handler emits one
    /// `vesting_event.override` audit row per applied row
    /// (AC-8.6.4 + AC-8.10.1).
    pub applied_count: u64,
    /// Rows that already carried an FMV and are therefore skipped
    /// (AC-8.6.2 — **even if `is_user_override = false`**, per Q4).
    pub skipped_count: u64,
}

/// DELETE every existing `vesting_events` row for `grant_id`, then INSERT
/// the supplied `events` as a single batch. Both operations run inside the
/// caller's transaction so a mid-batch failure rolls back the delete too.
///
/// A `grant_id` owned by another user is filtered by RLS: the DELETE matches
/// zero rows and the INSERT's `WITH CHECK` rejects the row with an RLS
/// violation error.
///
/// # Slice-3 interaction
///
/// This helper is the Slice-1 hot path for grant-create/update. It does
/// not preserve Slice-3 override state — callers that need to preserve
/// overrides across grant-param changes must compose via the T29
/// override-aware derivation helper and write through [`apply_override`]
/// for the overridden rows. The Slice-1 signature stays untouched so
/// existing callers (create_grant, update_grant that has no overrides)
/// keep working.
pub async fn replace_for_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
    events: Vec<VestingEvent>,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM vesting_events WHERE grant_id = $1 AND user_id = $2")
        .bind(grant_id)
        .bind(user_id)
        .execute(tx.as_executor())
        .await?;

    if events.is_empty() {
        return Ok(());
    }

    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "INSERT INTO vesting_events (\
            user_id, grant_id, vest_date, \
            shares_vested_this_event, cumulative_shares_vested, state\
        ) ",
    );
    qb.push_values(events.iter(), |mut b, event| {
        b.push_bind(user_id)
            .push_bind(grant_id)
            .push_bind(event.vest_date)
            // Both numeric columns go through the scaled-i64 bridge.
            .push_bind(event.shares_vested_this_event)
            .push_unseparated("::numeric / 10000")
            .push_bind(event.cumulative_shares_vested)
            .push_unseparated("::numeric / 10000")
            .push_bind(vesting_state_str(event.state));
    });

    qb.build().execute(tx.as_executor()).await?;
    Ok(())
}

/// List every `vesting_events` row for `grant_id`, oldest first. RLS scopes
/// the visible rows to the owner (`user_id` filter is redundant with the
/// RLS policy but keeps the query self-describing).
pub async fn list_for_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Vec<VestingEventRow>, sqlx::Error> {
    let rows = sqlx::query(&select_all_columns_for_grant())
        .bind(grant_id)
        .bind(user_id)
        .fetch_all(tx.as_executor())
        .await?;

    rows.iter().map(row_to_event).collect()
}

/// Slice-3 alias of [`list_for_grant`]. The Slice-1 helper already
/// returns the full row (including Slice-3 columns after the migration);
/// the alias documents intent at call sites that specifically care
/// about the override surface (AC-8.5.1 grant-detail listing).
///
/// Kept as a separate entry point so future Slice-4+ specialization
/// (e.g., eager-join `grants` to compute algorithmic diffs) has a
/// natural home without rewriting Slice-1 callers.
pub async fn list_for_grant_with_overrides(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Vec<VestingEventRow>, sqlx::Error> {
    list_for_grant(tx, user_id, grant_id).await
}

/// Count of `is_user_override = true` rows for a grant. Used by the
/// grant-edit form banner (AC-8.8.1).
pub async fn count_override_rows(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS n
          FROM vesting_events
         WHERE user_id = $1
           AND grant_id = $2
           AND is_user_override = true
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_one(tx.as_executor())
    .await?;

    let n: i64 = row.try_get("n")?;
    Ok(n.max(0) as u64)
}

/// Sum of `shares_vested_this_event` across overridden rows for a
/// grant. Returned as [`Shares`] (scaled-i64). Drives the AC-8.9.1
/// share-count shrink-below-overrides defensive check — the handler
/// compares the proposed new `grants.share_count` against this sum
/// before allowing a grant-edit to land.
///
/// Returns `0` when the grant has no overridden rows or is not owned
/// by `user_id` (RLS-filtered).
pub async fn sum_override_shares(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Shares, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            COALESCE(
                (SUM(shares_vested_this_event) * 10000)::bigint,
                0::bigint
            ) AS sum_scaled
          FROM vesting_events
         WHERE user_id = $1
           AND grant_id = $2
           AND is_user_override = true
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_one(tx.as_executor())
    .await?;

    row.try_get("sum_scaled")
}

/// Apply a user override to a single `vesting_events` row.
///
/// Semantics (AC-8.2 / AC-8.3 / AC-8.4 / AC-10.5):
///
///   * The UPDATE is predicated on `updated_at = $expected` — on
///     mismatch we return [`OverrideOutcome::Conflict`] and the handler
///     surfaces 409 with `code = "resource.stale_client_state"`.
///   * Any field in `patch` overwrites the corresponding column; a
///     `None` in the patch leaves the column untouched.
///   * `is_user_override = true` and `overridden_at = now()` are set
///     unconditionally — this is the write that claims manual-edit
///     ownership of the row.
///   * The `touch_updated_at` trigger advances `updated_at` as part of
///     the same UPDATE, so the returned row carries the new token the
///     caller's next PUT must match.
///
/// The cross-field CHECK `fmv_pair_coherent` enforces that a
/// FMV-only patch supplies both `fmv_at_vest` and `fmv_currency` (or
/// both as `None`); handler-level validation should gate this first.
///
/// Cross-tenant calls are filtered by RLS (USING) — the UPDATE matches
/// zero rows and the helper returns `Conflict` indistinguishably from
/// a stale-state case. The handler's prior `get_event` lookup
/// distinguishes "not found" from "stale" via the 404-vs-409 split.
pub async fn apply_override(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    event_id: Uuid,
    patch: &VestingEventOverridePatch,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
) -> Result<OverrideOutcome, sqlx::Error> {
    // Compose the SET list dynamically: we only touch columns the
    // patch names. `is_user_override` + `overridden_at` are always set.
    // `updated_at` is trigger-maintained, not written by the handler.
    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "UPDATE vesting_events SET is_user_override = true, overridden_at = now()",
    );

    if let Some(d) = patch.vest_date {
        qb.push(", vest_date = ");
        qb.push_bind(d);
    }
    if let Some(s) = patch.shares_vested_this_event {
        qb.push(", shares_vested_this_event = ");
        qb.push_bind(s);
        qb.push("::numeric / 10000");
    }
    if let Some(ref fmv_opt) = patch.fmv_at_vest {
        qb.push(", fmv_at_vest = ");
        match fmv_opt {
            Some(s) => {
                qb.push_bind(s.clone());
                qb.push("::numeric");
            }
            None => {
                qb.push("NULL");
            }
        }
    }
    if let Some(ref cur_opt) = patch.fmv_currency {
        qb.push(", fmv_currency = ");
        match cur_opt {
            Some(s) => {
                qb.push_bind(s.clone());
            }
            None => {
                qb.push("NULL");
            }
        }
    }

    qb.push(" WHERE id = ");
    qb.push_bind(event_id);
    qb.push(" AND user_id = ");
    qb.push_bind(user_id);
    qb.push(" AND updated_at = ");
    qb.push_bind(expected_updated_at);
    qb.push(" RETURNING ");
    qb.push(RETURNING_COLUMNS);

    let row = qb.build().fetch_optional(tx.as_executor()).await?;
    match row {
        Some(r) => Ok(OverrideOutcome::Applied(row_to_event(&r)?)),
        None => Ok(OverrideOutcome::Conflict),
    }
}

/// Revert `vest_date` and `shares_vested_this_event` to the algorithm's
/// current output for this row; **preserve `fmv_at_vest` and
/// `fmv_currency`** per AC-8.7.1.
///
/// Per AC-8.7.1 (d): if the user had entered an FMV, `is_user_override`
/// stays `true` because the FMV itself is a manual edit (and the
/// `override_flag_coherent` CHECK then requires `overridden_at` to stay
/// non-NULL). Only when the row carried no FMV does the clear drop
/// both flags.
///
/// The algorithm output is derived from the parent grant via
/// [`orbit_core::derive_vesting_events`]. For T28 this uses the
/// Slice-1 signature (no override-aware branch); T29 will extend
/// `derive_vesting_events` to accept existing overrides and align the
/// "algorithmic output for slot" logic. In the meantime, the helper
/// matches the algorithm-row for this event by `vest_date` equality
/// *or*, if the current overridden `vest_date` has no match in the
/// re-derivation, by index position after sort — which covers the
/// common case (user overrode the vest within-window).
///
/// Returns [`ClearOutcome::NotFound`] if the row is not present or not
/// owned by `user_id` (RLS-filtered); [`ClearOutcome::NoAlgorithmicMatch`]
/// if the algorithm produces no row that can be reverted to (the
/// override's `vest_date` is outside the derivation window AND no
/// positional match is possible — practically, the grant has zero
/// derived rows). Otherwise [`ClearOutcome::Cleared(row)`].
pub async fn clear_override(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    event_id: Uuid,
    today: NaiveDate,
) -> Result<ClearOutcome, sqlx::Error> {
    // 1. Fetch the row to clear. RLS scopes us; `None` → NotFound.
    let existing = get_event(tx, user_id, event_id).await?;
    let Some(existing) = existing else {
        return Ok(ClearOutcome::NotFound);
    };

    // 2. Read the parent grant's derivation inputs. We only need the
    // fields that affect the schedule, so a targeted SELECT keeps the
    // round-trip cheap.
    let grant_row = sqlx::query(
        r#"
        SELECT
            (share_count * 10000)::bigint AS share_count_scaled,
            vesting_start, vesting_total_months, cliff_months, vesting_cadence,
            double_trigger, liquidity_event_date
          FROM grants
         WHERE id = $1 AND user_id = $2
        "#,
    )
    .bind(existing.grant_id)
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;
    let Some(grant_row) = grant_row else {
        // Grant got deleted out from under us. Treat as NotFound — the
        // cascade would have removed this row anyway.
        return Ok(ClearOutcome::NotFound);
    };

    let share_count: Shares = grant_row.try_get("share_count_scaled")?;
    let vesting_start: NaiveDate = grant_row.try_get("vesting_start")?;
    let vesting_total_months: i32 = grant_row.try_get("vesting_total_months")?;
    let cliff_months: i32 = grant_row.try_get("cliff_months")?;
    let cadence_str: String = grant_row.try_get("vesting_cadence")?;
    let double_trigger: bool = grant_row.try_get("double_trigger")?;
    let liquidity_event_date: Option<NaiveDate> = grant_row.try_get("liquidity_event_date")?;

    let cadence = match cadence_str.as_str() {
        "quarterly" => Cadence::Quarterly,
        _ => Cadence::Monthly,
    };

    let grant_input = GrantInput {
        share_count,
        vesting_start,
        vesting_total_months: vesting_total_months.max(0) as u32,
        cliff_months: cliff_months.max(0) as u32,
        cadence,
        double_trigger,
        liquidity_event_date,
    };

    // 3. Derive the algorithmic schedule. Failure here is a handler-
    // level bug (invalid grant params); surface as a decode error so
    // the caller's `?` propagates a 500.
    let derived = derive_vesting_events(&grant_input, today, &[]).map_err(|e| {
        sqlx::Error::Decode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("derive_vesting_events failed during clear_override: {e}"),
        )))
    })?;

    // 4. Pick the algorithmic row we're reverting to.
    //
    // The DB row's `cumulative_shares_vested` is the stable anchor: per
    // Slice-1 the field is written once by `replace_for_grant` and
    // [`apply_override`] does NOT mutate it, so it still reflects the
    // original algorithmic cumulative at this row's position. We match
    // on cumulative-equality first.
    //
    // Fall back to `vest_date` equality for the degenerate case where
    // two algorithmic rows share a cumulative (impossible given the
    // Slice-1 monotonic invariant, but preserved as defense-in-depth).
    // If neither match fires — e.g., the grant's `share_count` was
    // shrunk below the overridden row's cumulative — we return
    // `NoAlgorithmicMatch`; the handler surfaces a UI hint per AC-8.5.3.
    let match_by_cumulative = derived
        .iter()
        .find(|e| e.cumulative_shares_vested == existing.cumulative_shares_vested);
    let algo_row = match match_by_cumulative
        .or_else(|| derived.iter().find(|e| e.vest_date == existing.vest_date))
    {
        Some(r) => r,
        None => return Ok(ClearOutcome::NoAlgorithmicMatch),
    };

    // 5. Determine the new override flag per AC-8.7.1 (c)+(d):
    //    - if the row has no FMV after the revert, is_user_override=false
    //      and overridden_at=NULL (clean reset).
    //    - if the row still carries an FMV, is_user_override remains
    //      true and overridden_at is preserved (the CHECK requires it).
    let has_fmv = existing.fmv_at_vest.is_some();

    // 6. UPDATE in place. We do NOT change fmv_at_vest / fmv_currency
    // (preservation per AC-8.7.1 (b)). We DO update vest_date + shares,
    // and conditionally clear the override flag.
    let row = if has_fmv {
        sqlx::query(&format!(
            r#"
            UPDATE vesting_events
               SET vest_date = $3,
                   shares_vested_this_event = $4::numeric / 10000
             WHERE id = $1 AND user_id = $2
         RETURNING {RETURNING_COLUMNS}
            "#,
        ))
        .bind(event_id)
        .bind(user_id)
        .bind(algo_row.vest_date)
        .bind(algo_row.shares_vested_this_event)
        .fetch_one(tx.as_executor())
        .await?
    } else {
        sqlx::query(&format!(
            r#"
            UPDATE vesting_events
               SET vest_date = $3,
                   shares_vested_this_event = $4::numeric / 10000,
                   is_user_override = false,
                   overridden_at = NULL
             WHERE id = $1 AND user_id = $2
         RETURNING {RETURNING_COLUMNS}
            "#,
        ))
        .bind(event_id)
        .bind(user_id)
        .bind(algo_row.vest_date)
        .bind(algo_row.shares_vested_this_event)
        .fetch_one(tx.as_executor())
        .await?
    };

    Ok(ClearOutcome::Cleared(row_to_event(&row)?))
}

/// Outcome of [`clear_override`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClearOutcome {
    /// The revert landed; the returned row carries the algorithmic
    /// `vest_date` and `shares_vested_this_event`; FMV is preserved.
    Cleared(VestingEventRow),
    /// The event id was not found or is not owned by `user_id`.
    NotFound,
    /// The derivation algorithm produced no row matching this event's
    /// `vest_date` — the override is "outside the window" (e.g., grant
    /// params changed since the override was applied). The handler
    /// surfaces a UI hint; the row is left unchanged.
    NoAlgorithmicMatch,
}

/// Bulk-fill FMV on every row for a grant where `fmv_at_vest IS NULL`.
/// Rows that already carry a FMV are **skipped** — per Q4 the gate is
/// `fmv_at_vest IS NULL`, not the `is_user_override` flag (AC-8.6.2).
///
/// Sets `is_user_override = true` and `overridden_at = now()` on every
/// modified row; the handler is expected to emit one
/// `vesting_event.override` audit row per modified row (AC-8.6.4 +
/// AC-8.10.1). A partial-transaction failure rolls everything back
/// (AC-10.7).
///
/// Returns `{ applied_count, skipped_count }`. `skipped_count` is
/// computed with a second aggregate query inside the same transaction
/// so the caller can render the AC-8.6.3 confirmation-echo response
/// without a third round-trip.
pub async fn bulk_fill_fmv(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
    fmv: &str,
    currency: &str,
) -> Result<BulkFillResult, sqlx::Error> {
    // UPDATE first; ON CONFLICT is irrelevant (we're writing to
    // existing rows). `fmv_at_vest IS NULL` is the gate per Q4.
    let update_result = sqlx::query(
        r#"
        UPDATE vesting_events
           SET fmv_at_vest = $3::numeric,
               fmv_currency = $4,
               is_user_override = true,
               overridden_at = now()
         WHERE user_id = $1
           AND grant_id = $2
           AND fmv_at_vest IS NULL
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .bind(fmv)
    .bind(currency)
    .execute(tx.as_executor())
    .await?;

    let applied_count = update_result.rows_affected();

    // Skipped = rows that carried an FMV before the call. After the
    // UPDATE lands, every row for this grant carries an FMV; the
    // "skipped" count is therefore "rows that did NOT come from
    // this UPDATE" = total rows - applied.
    let total_row = sqlx::query(
        r#"
        SELECT COUNT(*) AS n
          FROM vesting_events
         WHERE user_id = $1 AND grant_id = $2
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_one(tx.as_executor())
    .await?;
    let total: i64 = total_row.try_get("n")?;
    let total = total.max(0) as u64;
    let skipped_count = total.saturating_sub(applied_count);

    Ok(BulkFillResult {
        applied_count,
        skipped_count,
    })
}

/// Fetch a single `vesting_events` row by id, scoped to the owner.
/// Returns `None` when the row is absent or RLS-filtered. Used by
/// [`clear_override`] and by handler pre-checks.
pub async fn get_event(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    event_id: Uuid,
) -> Result<Option<VestingEventRow>, sqlx::Error> {
    let row = sqlx::query(&format!(
        r#"
        SELECT {RETURNING_COLUMNS}
          FROM vesting_events
         WHERE id = $1 AND user_id = $2
        "#,
    ))
    .bind(event_id)
    .bind(user_id)
    .fetch_optional(tx.as_executor())
    .await?;

    row.as_ref().map(row_to_event).transpose()
}

// ---------------------------------------------------------------------------
// Column list used by SELECT/RETURNING across the module. Centralized
// so the row_to_event decoder stays in sync.
// ---------------------------------------------------------------------------
const RETURNING_COLUMNS: &str = "\
    id, user_id, grant_id, vest_date, \
    (shares_vested_this_event * 10000)::bigint AS shares_vested_this_event_scaled, \
    (cumulative_shares_vested * 10000)::bigint AS cumulative_shares_vested_scaled, \
    state, computed_at, \
    fmv_at_vest::text AS fmv_at_vest_text, \
    fmv_currency, \
    is_user_override, \
    overridden_at, \
    updated_at\
";

fn select_all_columns_for_grant() -> String {
    format!(
        r#"
        SELECT {RETURNING_COLUMNS}
          FROM vesting_events
         WHERE grant_id = $1 AND user_id = $2
         ORDER BY vest_date ASC
        "#,
    )
}

fn row_to_event(row: &sqlx::postgres::PgRow) -> Result<VestingEventRow, sqlx::Error> {
    let state: String = row.try_get("state")?;
    Ok(VestingEventRow {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        grant_id: row.try_get("grant_id")?,
        vest_date: row.try_get("vest_date")?,
        shares_vested_this_event: row.try_get("shares_vested_this_event_scaled")?,
        cumulative_shares_vested: row.try_get("cumulative_shares_vested_scaled")?,
        state: parse_vesting_state(&state).map_err(|e| sqlx::Error::ColumnDecode {
            index: "state".into(),
            source: Box::new(e),
        })?,
        computed_at: row.try_get("computed_at")?,
        fmv_at_vest: row.try_get("fmv_at_vest_text")?,
        fmv_currency: row.try_get("fmv_currency")?,
        is_user_override: row.try_get("is_user_override")?,
        overridden_at: row.try_get("overridden_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn vesting_state_str(s: VestingState) -> &'static str {
    match s {
        VestingState::Upcoming => "upcoming",
        VestingState::TimeVestedAwaitingLiquidity => "time_vested_awaiting_liquidity",
        VestingState::Vested => "vested",
    }
}

fn parse_vesting_state(s: &str) -> Result<VestingState, UnknownVestingState> {
    match s {
        "upcoming" => Ok(VestingState::Upcoming),
        "time_vested_awaiting_liquidity" => Ok(VestingState::TimeVestedAwaitingLiquidity),
        "vested" => Ok(VestingState::Vested),
        other => Err(UnknownVestingState(other.to_owned())),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown vesting_events.state value: {0:?}")]
struct UnknownVestingState(String);
