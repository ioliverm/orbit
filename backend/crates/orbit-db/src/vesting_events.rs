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
//! # Slice 3b extensions (ADR-018 §1 + §2)
//!
//! Five additive columns added by `migrations/20260530120000_slice_3b.sql`
//! for the sell-to-cover track:
//!
//!   * `tax_withholding_percent` (`NUMERIC(5,4)`, nullable fraction in
//!     `[0, 1]`).
//!   * `share_sell_price` (`NUMERIC(20,6)`, nullable).
//!   * `share_sell_currency` (`TEXT`, nullable — `USD | EUR | GBP`).
//!   * `is_sell_to_cover_override` (`BOOLEAN NOT NULL DEFAULT false`).
//!   * `sell_to_cover_overridden_at` (`TIMESTAMPTZ`, nullable).
//!
//! Two new cross-field CHECKs enforce `sell_to_cover_triplet_coherent`
//! (all-or-none on the first three) and
//! `sell_to_cover_override_flag_coherent`
//! (`is_sell_to_cover_override = (sell_to_cover_overridden_at IS NOT NULL)`).
//!
//! The Slice-3b repo surface ([`apply_sell_to_cover_override`],
//! [`clear_sell_to_cover_override`]) implements the write paths for
//! AC-5.* and AC-7.5.*. [`clear_override`] is extended (ADR-018 §2
//! supersede) to additionally clear the sell-to-cover triplet — the
//! Slice-3 "preserve FMV on full clear" semantic is replaced by the
//! Slice-3b "nuclear revert clears both tracks" semantic. Callers who
//! want to preserve FMV while clearing only the sell-to-cover triplet
//! use [`clear_sell_to_cover_override`] instead.
//!
//! Traces to:
//!   - ADR-014 §1 (vesting_events DDL, UNIQUE on (grant_id, vest_date)).
//!   - ADR-017 §1 (Slice-3 additive columns + cross-field CHECKs).
//!   - ADR-018 §1 (Slice-3b additive columns + cross-field CHECKs).
//!   - ADR-018 §2 (full-clear supersedes FMV preservation).
//!   - docs/requirements/slice-1-acceptance-criteria.md §4.3.
//!   - docs/requirements/slice-3-acceptance-criteria.md §8 (AC-8.2..AC-8.9).
//!   - docs/requirements/slice-3b-acceptance-criteria.md §5 + §7.5.

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
    /// Slice 3b addition. Fraction in `[0, 1]` carried as
    /// `NUMERIC(5,4)::text` (e.g., `"0.4500"`). `None` until the user
    /// captures a sell-to-cover override (or the handler seeds from
    /// `user_tax_preferences` per AC-7.6). Paired with
    /// [`Self::share_sell_price`] and [`Self::share_sell_currency`] via
    /// the `sell_to_cover_triplet_coherent` CHECK.
    pub tax_withholding_percent: Option<String>,
    /// Slice 3b addition. Per-share sell price at vest for the
    /// sell-to-cover calculation. Decimal passthrough
    /// (`NUMERIC(20,6)::text`).
    pub share_sell_price: Option<String>,
    /// Slice 3b addition. `USD | EUR | GBP`. Paired with
    /// [`Self::share_sell_price`] and [`Self::tax_withholding_percent`].
    pub share_sell_currency: Option<String>,
    /// Slice 3b addition. `true` iff the row has been manually
    /// edited in the sell-to-cover dialog (triplet captured).
    /// Independent of [`Self::is_user_override`] so the two revert
    /// paths ([`clear_override`] vs [`clear_sell_to_cover_override`])
    /// compose cleanly. The DB enforces coherence with
    /// [`Self::sell_to_cover_overridden_at`] via the
    /// `sell_to_cover_override_flag_coherent` CHECK.
    pub is_sell_to_cover_override: bool,
    /// Slice 3b addition. `Some(t)` iff
    /// [`Self::is_sell_to_cover_override`] is `true`.
    pub sell_to_cover_overridden_at: Option<chrono::DateTime<chrono::Utc>>,
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
///
/// The `Applied` variant carries a full [`VestingEventRow`]; every
/// outcome originates from a DB round-trip that already produced a
/// heap-resident row. The large-variant clippy lint is suppressed
/// because boxing here would only trade a stack-move for a heap
/// indirection with no runtime benefit and would churn every
/// call-site's match expression (handler, tests, probes).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
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

/// Pre-DELETE snapshot of the override metadata for a grant. Used by
/// [`replace_for_grant_preserving_overrides`] to re-stitch the
/// Slice-3 override columns onto the freshly-derived rows — matching
/// by `vest_date`, which [`orbit_core::derive_vesting_events`]
/// guarantees is the authoritative substitution key.
#[derive(Debug, Clone)]
pub struct OverrideMeta {
    pub vest_date: NaiveDate,
    pub fmv_at_vest: Option<String>,
    pub fmv_currency: Option<String>,
    pub overridden_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// List `vest_date`-keyed override metadata for every overridden row
/// on this grant (i.e., `is_user_override = true`). Used by the Slice-3
/// override-preserving grant-update path (AC-8.4.2).
pub async fn list_overrides_for_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Vec<OverrideMeta>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            vest_date,
            fmv_at_vest::text AS fmv_at_vest_text,
            fmv_currency,
            overridden_at,
            (shares_vested_this_event * 10000)::bigint AS shares_scaled
          FROM vesting_events
         WHERE user_id = $1
           AND grant_id = $2
           AND is_user_override = true
         ORDER BY vest_date ASC
        "#,
    )
    .bind(user_id)
    .bind(grant_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter()
        .map(|r| {
            Ok(OverrideMeta {
                vest_date: r.try_get("vest_date")?,
                fmv_at_vest: r.try_get("fmv_at_vest_text")?,
                fmv_currency: r.try_get("fmv_currency")?,
                overridden_at: r.try_get("overridden_at")?,
            })
        })
        .collect()
}

/// Slice-3 override-preserving variant of [`replace_for_grant`]
/// (AC-8.4.2).
///
/// Semantics: same DELETE + batch-INSERT shape as Slice-1, but for each
/// event whose `vest_date` appears in `override_meta`, the insert
/// carries `fmv_at_vest`, `fmv_currency`, `overridden_at`, and
/// `is_user_override = true` verbatim from the prior override. Events
/// whose `vest_date` does NOT appear in `override_meta` are inserted
/// with the defaults (no FMV, no override flag) — matching the Slice-1
/// behaviour for non-overridden rows.
///
/// Caller contract: `events` must already be the output of
/// `derive_vesting_events(grant, today, overrides)` (Slice-3
/// override-aware path), so that every override's `vest_date` is
/// present in `events`. Violating the contract silently drops the
/// override — this is surfaced by the T31 integration probe
/// `grant_update_preserves_user_override_rows`.
pub async fn replace_for_grant_preserving_overrides(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
    events: Vec<VestingEvent>,
    override_meta: &[OverrideMeta],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM vesting_events WHERE grant_id = $1 AND user_id = $2")
        .bind(grant_id)
        .bind(user_id)
        .execute(tx.as_executor())
        .await?;

    if events.is_empty() {
        return Ok(());
    }

    // Index the override metadata for O(n) stitching (no HashMap allocation
    // for the typical small-N case; tight loop is still O(events × overrides)
    // in the worst case).
    let find_meta = |d: NaiveDate| override_meta.iter().find(|m| m.vest_date == d);

    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "INSERT INTO vesting_events (\
            user_id, grant_id, vest_date, \
            shares_vested_this_event, cumulative_shares_vested, state, \
            fmv_at_vest, fmv_currency, is_user_override, overridden_at\
        ) ",
    );
    qb.push_values(events.iter(), |mut b, event| {
        let meta = find_meta(event.vest_date);
        b.push_bind(user_id)
            .push_bind(grant_id)
            .push_bind(event.vest_date)
            .push_bind(event.shares_vested_this_event)
            .push_unseparated("::numeric / 10000")
            .push_bind(event.cumulative_shares_vested)
            .push_unseparated("::numeric / 10000")
            .push_bind(vesting_state_str(event.state));
        // Slice-3 override columns. When there's no matching override
        // we push NULL / false to mirror the Slice-1 defaults.
        match meta {
            Some(m) => {
                match m.fmv_at_vest.as_deref() {
                    Some(s) => {
                        b.push_bind(s.to_string()).push_unseparated("::numeric");
                    }
                    None => {
                        b.push("NULL");
                    }
                }
                match m.fmv_currency.as_deref() {
                    Some(c) => {
                        b.push_bind(c.to_string());
                    }
                    None => {
                        b.push("NULL");
                    }
                }
                b.push_bind(true);
                match m.overridden_at {
                    Some(t) => {
                        b.push_bind(t);
                    }
                    None => {
                        // `is_user_override = true` requires
                        // `overridden_at IS NOT NULL` (CHECK). Snap to
                        // NOW() to keep the row coherent.
                        b.push("now()");
                    }
                }
            }
            None => {
                b.push("NULL");
                b.push("NULL");
                b.push_bind(false);
                b.push("NULL");
            }
        }
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
/// current output for this row, and **clear every override column on
/// both the Slice-3 FMV track and the Slice-3b sell-to-cover track**
/// per ADR-018 §2.
///
/// # Behaviour change from Slice 3 (ADR-018 §2 supersede)
///
/// In Slice 3 this helper preserved `fmv_at_vest` / `fmv_currency` on
/// the assumption that the narrow revert path did not exist (AC-8.7.1
/// original). Slice 3b introduces [`clear_sell_to_cover_override`] as
/// the narrow revert; `clear_override` now becomes the "nuclear"
/// revert and clears both tracks in full:
///
///   * `vest_date` + `shares_vested_this_event` ← algorithm output;
///   * `fmv_at_vest` + `fmv_currency` ← `NULL`;
///   * `tax_withholding_percent` + `share_sell_price` +
///     `share_sell_currency` ← `NULL`;
///   * `is_user_override` ← `false`, `overridden_at` ← `NULL`;
///   * `is_sell_to_cover_override` ← `false`,
///     `sell_to_cover_overridden_at` ← `NULL`.
///
/// Rationale (ADR-018 §3, AC-7.5.1): in Slice-3b the UI exposes two
/// distinct revert buttons — a narrow one that preserves FMV and a
/// "revert everything" one that does not. The DB-layer full-clear is
/// the latter; callers who want FMV preservation now route through
/// [`clear_sell_to_cover_override`] explicitly.
///
/// The algorithm output is derived from the parent grant via
/// [`orbit_core::derive_vesting_events`]. For T28 this uses the
/// Slice-1 signature (no override-aware branch); T29 extends
/// `derive_vesting_events` to accept existing overrides. The helper
/// matches the algorithm-row for this event by
/// `cumulative_shares_vested` (the stable anchor written once by
/// [`replace_for_grant`]), falling back to `vest_date` equality for
/// the degenerate case.
///
/// Returns [`ClearOutcome::NotFound`] if the row is not present or not
/// owned by `user_id` (RLS-filtered); [`ClearOutcome::Conflict`] if the
/// `expected_updated_at` predicate did not match at UPDATE time (AC-10.5
/// — matches [`apply_override`]'s OCC discipline);
/// [`ClearOutcome::NoAlgorithmicMatch`] if the algorithm produces no row
/// that can be reverted to (the override's `vest_date` is outside the
/// derivation window AND no positional match is possible — practically,
/// the grant has zero derived rows). Otherwise
/// [`ClearOutcome::Cleared(row)`].
pub async fn clear_override(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    event_id: Uuid,
    today: NaiveDate,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
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

    // 5. UPDATE in place, predicated on `updated_at = $expected` to
    // close the read-vs-update race (AC-10.5 — same OCC discipline as
    // `apply_override`). A zero-rows result is indistinguishable from
    // a cross-tenant RLS filter, but the prior `get_event` lookup
    // narrows that case to the stale-state branch.
    //
    // Per ADR-018 §2 supersede: the full-clear is the "nuclear"
    // revert. It clears every Slice-3 FMV-track column (fmv_at_vest,
    // fmv_currency, is_user_override, overridden_at) AND every
    // Slice-3b sell-to-cover-track column (tax_withholding_percent,
    // share_sell_price, share_sell_currency,
    // is_sell_to_cover_override, sell_to_cover_overridden_at).
    // Callers that want to preserve FMV while clearing only the
    // sell-to-cover triplet use `clear_sell_to_cover_override`.
    let row = sqlx::query(&format!(
        r#"
        UPDATE vesting_events
           SET vest_date                   = $3,
               shares_vested_this_event    = $4::numeric / 10000,
               fmv_at_vest                 = NULL,
               fmv_currency                = NULL,
               is_user_override            = false,
               overridden_at               = NULL,
               tax_withholding_percent     = NULL,
               share_sell_price            = NULL,
               share_sell_currency         = NULL,
               is_sell_to_cover_override   = false,
               sell_to_cover_overridden_at = NULL
         WHERE id = $1 AND user_id = $2 AND updated_at = $5
     RETURNING {RETURNING_COLUMNS}
        "#,
    ))
    .bind(event_id)
    .bind(user_id)
    .bind(algo_row.vest_date)
    .bind(algo_row.shares_vested_this_event)
    .bind(expected_updated_at)
    .fetch_optional(tx.as_executor())
    .await?;

    match row {
        Some(r) => Ok(ClearOutcome::Cleared(row_to_event(&r)?)),
        None => Ok(ClearOutcome::Conflict),
    }
}

/// Outcome of [`clear_override`]. See [`OverrideOutcome`] for the
/// rationale behind the `#[allow(clippy::large_enum_variant)]`
/// suppression.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum ClearOutcome {
    /// The revert landed; the returned row carries the algorithmic
    /// `vest_date` and `shares_vested_this_event`; every Slice-3
    /// FMV-track and Slice-3b sell-to-cover-track column is reset
    /// per ADR-018 §2 supersede.
    Cleared(VestingEventRow),
    /// The event id was not found or is not owned by `user_id`.
    NotFound,
    /// The `expected_updated_at` predicate did not match the DB row's
    /// current `updated_at`. The handler returns 409 with code
    /// `resource.stale_client_state`, matching `apply_override`
    /// (AC-10.5).
    Conflict,
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

// ---------------------------------------------------------------------------
// Slice 3b — sell-to-cover override surface (ADR-018 §1 + §2).
// ---------------------------------------------------------------------------

/// Patch supplied on a PUT `/grants/:grantId/vesting-events/:eventId`
/// that writes to the Slice-3b sell-to-cover track (AC-7.3 + AC-8.3b).
/// Every field is optional — the handler layer validates per-field
/// rules and composes the patch before calling
/// [`apply_sell_to_cover_override`]. The handler is also responsible
/// for enforcing the `sell_to_cover_triplet_coherent` invariant at
/// validation time; the DB CHECK is defense-in-depth.
///
/// `None` leaves the column untouched; `Some(Some(v))` writes `v`;
/// `Some(None)` clears the column. Mirrors the
/// [`VestingEventOverridePatch`] shape for the Slice-3 FMV track.
#[derive(Debug, Clone, Default)]
pub struct SellToCoverOverridePatch {
    /// Stringified `NUMERIC(5,4)` fraction in `[0, 1]` (e.g.,
    /// `"0.4500"`).
    pub tax_withholding_percent: Option<Option<String>>,
    /// Stringified `NUMERIC(20,6)` per-share sell price.
    pub share_sell_price: Option<Option<String>>,
    /// `USD | EUR | GBP`.
    pub share_sell_currency: Option<Option<String>>,
}

/// Outcome of [`apply_sell_to_cover_override`]. See [`OverrideOutcome`]
/// for the rationale behind the `#[allow(clippy::large_enum_variant)]`
/// suppression.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum SellToCoverOutcome {
    /// The patch landed. `updated_at` on the returned row advances;
    /// `is_sell_to_cover_override = true` and
    /// `sell_to_cover_overridden_at = now()`.
    Applied(VestingEventRow),
    /// The `expected_updated_at` predicate did not match the DB row's
    /// current `updated_at`. The handler returns 409 with code
    /// `resource.stale_client_state`.
    Conflict,
    /// The event id was not found or is not owned by `user_id`
    /// (RLS-filtered, or the row was deleted after the handler's
    /// read). The handler surfaces 404. The prior Slice-3-style
    /// `get_event` lookup distinguishes NotFound from Conflict; this
    /// outcome is returned for the sell-to-cover-specific path when
    /// the pre-read has already fired and produced `None`.
    NotFound,
}

/// Outcome of [`clear_sell_to_cover_override`]. See
/// [`OverrideOutcome`] for the rationale behind the
/// `#[allow(clippy::large_enum_variant)]` suppression.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum SellToCoverClearOutcome {
    /// The narrow revert landed. The returned row has the sell-to-
    /// cover triplet cleared, `is_sell_to_cover_override = false`,
    /// `sell_to_cover_overridden_at = NULL`; every Slice-3 FMV-track
    /// column is preserved verbatim (AC-7.5.2).
    Cleared(VestingEventRow),
    /// OCC mismatch (AC-10.5).
    Conflict,
    /// Row absent or RLS-filtered.
    NotFound,
}

/// Apply a sell-to-cover override to a single `vesting_events` row.
///
/// Semantics (AC-7.3.*, AC-8.3b, AC-7.4.4):
///
///   * The UPDATE is predicated on `updated_at = $expected` — on
///     mismatch we return [`SellToCoverOutcome::Conflict`] and the
///     handler surfaces 409 with `code = "resource.stale_client_state"`.
///     A zero-rows result on a row that was pre-verified to exist
///     (via [`get_event`]) is stale-state; a zero-rows result without
///     a prior read is ambiguous between stale-state and RLS-filtered.
///     Callers SHOULD pre-verify existence and treat the resulting
///     `None` as Conflict.
///   * Any field in `patch` whose outer `Option` is `Some` overwrites
///     the corresponding column; `None` leaves the column untouched.
///   * `is_sell_to_cover_override = true` and
///     `sell_to_cover_overridden_at = now()` are set unconditionally
///     — this is the write that claims sell-to-cover-manual-edit
///     ownership of the row. The Slice-3 `is_user_override` /
///     `overridden_at` pair is left untouched here; the handler is
///     responsible for composing an `apply_override` call when the
///     body also mutates FMV or vest_date or shares.
///   * The `vesting_events_touch_updated_at` trigger advances
///     `updated_at` as part of the same UPDATE, so the returned row
///     carries the new token the caller's next PUT must match.
///
/// Cross-field CHECKs (`sell_to_cover_triplet_coherent` and
/// `sell_to_cover_override_flag_coherent`) are defense-in-depth. The
/// handler's validator is expected to reject partial-triplet patches
/// before this path is reached.
///
/// Cross-tenant calls are filtered by RLS (USING) — the UPDATE
/// matches zero rows and the helper returns [`SellToCoverOutcome::Conflict`]
/// indistinguishably from a stale-state case. Callers that need the
/// 404-vs-409 split pre-read via [`get_event`].
pub async fn apply_sell_to_cover_override(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    event_id: Uuid,
    patch: &SellToCoverOverridePatch,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
) -> Result<SellToCoverOutcome, sqlx::Error> {
    // Compose the SET list dynamically: we only touch columns the
    // patch names. `is_sell_to_cover_override` +
    // `sell_to_cover_overridden_at` are always set. `updated_at` is
    // trigger-maintained, not written by the handler.
    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "UPDATE vesting_events \
         SET is_sell_to_cover_override = true, \
             sell_to_cover_overridden_at = now()",
    );

    if let Some(ref pct_opt) = patch.tax_withholding_percent {
        qb.push(", tax_withholding_percent = ");
        match pct_opt {
            Some(s) => {
                qb.push_bind(s.clone());
                qb.push("::numeric");
            }
            None => {
                qb.push("NULL");
            }
        }
    }
    if let Some(ref price_opt) = patch.share_sell_price {
        qb.push(", share_sell_price = ");
        match price_opt {
            Some(s) => {
                qb.push_bind(s.clone());
                qb.push("::numeric");
            }
            None => {
                qb.push("NULL");
            }
        }
    }
    if let Some(ref cur_opt) = patch.share_sell_currency {
        qb.push(", share_sell_currency = ");
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
        Some(r) => Ok(SellToCoverOutcome::Applied(row_to_event(&r)?)),
        None => {
            // Distinguish NotFound from Conflict by re-checking
            // row presence under RLS. Matches the discipline the
            // Slice-3 `clear_override` uses for the same case.
            let exists =
                sqlx::query("SELECT 1 AS n FROM vesting_events WHERE id = $1 AND user_id = $2")
                    .bind(event_id)
                    .bind(user_id)
                    .fetch_optional(tx.as_executor())
                    .await?;
            if exists.is_some() {
                Ok(SellToCoverOutcome::Conflict)
            } else {
                Ok(SellToCoverOutcome::NotFound)
            }
        }
    }
}

/// Narrow revert of the sell-to-cover track only (AC-7.5.2,
/// ADR-018 §3 `clearSellToCoverOverride`).
///
/// Clears `tax_withholding_percent`, `share_sell_price`,
/// `share_sell_currency`, and sets `is_sell_to_cover_override = false`
/// plus `sell_to_cover_overridden_at = NULL`. **Preserves every
/// Slice-3 FMV-track column verbatim** — `fmv_at_vest`,
/// `fmv_currency`, `is_user_override`, `overridden_at`, `vest_date`,
/// and `shares_vested_this_event` are all untouched. Callers that
/// want the full revert — both tracks, plus `vest_date` and
/// `shares_vested_this_event` reverted to algorithm output — use
/// [`clear_override`] instead.
///
/// Predicated on `updated_at = $expected` per AC-10.5 (same OCC
/// discipline as [`apply_override`] and [`apply_sell_to_cover_override`]).
pub async fn clear_sell_to_cover_override(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    event_id: Uuid,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
) -> Result<SellToCoverClearOutcome, sqlx::Error> {
    let row = sqlx::query(&format!(
        r#"
        UPDATE vesting_events
           SET tax_withholding_percent     = NULL,
               share_sell_price            = NULL,
               share_sell_currency         = NULL,
               is_sell_to_cover_override   = false,
               sell_to_cover_overridden_at = NULL
         WHERE id = $1 AND user_id = $2 AND updated_at = $3
     RETURNING {RETURNING_COLUMNS}
        "#,
    ))
    .bind(event_id)
    .bind(user_id)
    .bind(expected_updated_at)
    .fetch_optional(tx.as_executor())
    .await?;

    match row {
        Some(r) => Ok(SellToCoverClearOutcome::Cleared(row_to_event(&r)?)),
        None => {
            let exists =
                sqlx::query("SELECT 1 AS n FROM vesting_events WHERE id = $1 AND user_id = $2")
                    .bind(event_id)
                    .bind(user_id)
                    .fetch_optional(tx.as_executor())
                    .await?;
            if exists.is_some() {
                Ok(SellToCoverClearOutcome::Conflict)
            } else {
                Ok(SellToCoverClearOutcome::NotFound)
            }
        }
    }
}

/// Slice-3b alias of [`list_for_grant`]. The Slice-1/-3 helper
/// already selects the full row (including Slice-3b columns after
/// the migration); the alias documents intent at call sites that
/// specifically care about the sell-to-cover surface (AC-5.1.1 +
/// AC-7.2.1 dialog listing).
///
/// Kept as a separate entry point so future Slice-4+ specialization
/// (e.g., compute derived sell-to-cover values server-side and
/// return them alongside the row) has a natural home without
/// rewriting Slice-3 callers.
pub async fn list_with_sell_to_cover_for_grant(
    tx: &mut Tx<'_>,
    user_id: Uuid,
    grant_id: Uuid,
) -> Result<Vec<VestingEventRow>, sqlx::Error> {
    list_for_grant(tx, user_id, grant_id).await
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
    updated_at, \
    tax_withholding_percent::text AS tax_withholding_percent_text, \
    share_sell_price::text AS share_sell_price_text, \
    share_sell_currency, \
    is_sell_to_cover_override, \
    sell_to_cover_overridden_at\
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
        tax_withholding_percent: row.try_get("tax_withholding_percent_text")?,
        share_sell_price: row.try_get("share_sell_price_text")?,
        share_sell_currency: row.try_get("share_sell_currency")?,
        is_sell_to_cover_override: row.try_get("is_sell_to_cover_override")?,
        sell_to_cover_overridden_at: row.try_get("sell_to_cover_overridden_at")?,
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
