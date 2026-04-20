//! `vesting_events` repository (Slice 1 T12).
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
//! Traces to:
//!   - ADR-014 §1 (vesting_events DDL, UNIQUE on (grant_id, vest_date)).
//!   - docs/requirements/slice-1-acceptance-criteria.md §4.3.

use orbit_core::{Shares, VestingEvent, VestingState};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

use crate::Tx;

/// A `vesting_events` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VestingEventRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub grant_id: Uuid,
    pub vest_date: chrono::NaiveDate,
    pub shares_vested_this_event: Shares,
    pub cumulative_shares_vested: Shares,
    pub state: VestingState,
    pub computed_at: chrono::DateTime<chrono::Utc>,
}

/// DELETE every existing `vesting_events` row for `grant_id`, then INSERT
/// the supplied `events` as a single batch. Both operations run inside the
/// caller's transaction so a mid-batch failure rolls back the delete too.
///
/// A `grant_id` owned by another user is filtered by RLS: the DELETE matches
/// zero rows and the INSERT's `WITH CHECK` rejects the row with an RLS
/// violation error.
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
    let rows = sqlx::query(
        r#"
        SELECT
            id, user_id, grant_id, vest_date,
            (shares_vested_this_event * 10000)::bigint AS shares_vested_this_event_scaled,
            (cumulative_shares_vested * 10000)::bigint AS cumulative_shares_vested_scaled,
            state, computed_at
          FROM vesting_events
         WHERE grant_id = $1 AND user_id = $2
         ORDER BY vest_date ASC
        "#,
    )
    .bind(grant_id)
    .bind(user_id)
    .fetch_all(tx.as_executor())
    .await?;

    rows.iter().map(row_to_event).collect()
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
