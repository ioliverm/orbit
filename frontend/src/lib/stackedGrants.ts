// TypeScript mirror of `backend/crates/orbit-core/src/stacked_grants.rs`
// (ADR-016 §4, Slice 2 T22 parity deliverable).
//
// The server is authoritative — `/api/v1/dashboard/stacked` returns the
// fully-computed payload and the dashboard renders it directly. This
// mirror exists so client-side views (filters, overlays, previews) can
// run the same algorithm without a round-trip, and so the shared fixture
// (`backend/crates/orbit-core/tests/fixtures/stacked_grants_cases.json`)
// can be bitwise-parity-checked from Vitest.
//
// Determinism + tie-break (AC-8.2.8): merged events sort by
// `(vest_date ASC, grant.created_at ASC, grant.id ASC)`. All share-count
// math is bigint (scaled `Shares` = 1/10_000 of a share), matching the
// Rust `Shares` i64.

import type { VestingState } from './vesting';

/**
 * Minimal grant metadata the stacked view needs. Everything here is
 * already on the Slice-1 `grants` row — no new columns.
 */
export interface GrantMeta {
  id: string;
  employerName: string;
  /** `'rsu' | 'nso' | 'espp' | 'iso_mapped_to_nso'` as stored. */
  instrument: string;
  /** ISO-8601 timestamp (matches Rust `DateTime<Utc>` serialization). */
  createdAt: string;
}

/**
 * One vesting event tagged with its parent grant. Mirror of the
 * `(grant_id, VestingEvent)` pair the Rust impl flattens to internally.
 * Callers typically materialize these from the Slice-1
 * `/grants/:id/vesting` response (`vestingEvents[]`).
 */
export interface VestingEvent {
  grantId: string;
  /** ISO-8601 date string (YYYY-MM-DD). */
  vestDate: string;
  /** Scaled (1 share = 10_000). */
  sharesVestedThisEventScaled: bigint;
  /** Scaled. */
  cumulativeSharesVestedScaled: bigint;
  state: VestingState;
}

/**
 * One grant's contribution at a given event date. Mirror of
 * `orbit_core::stacked_grants::PerGrantDelta`.
 */
export interface PerGrantDelta {
  grantId: string;
  instrument: string;
  sharesVestedThisEvent: bigint;
  cumulativeForThisGrant: bigint;
  /** `"upcoming" | "time_vested_awaiting_liquidity" | "vested"`. */
  state: string;
}

/** Mirror of `orbit_core::stacked_grants::StackedPoint`.
 *
 * Field names match the wire DTO exactly (the Rust type applies
 * `#[serde(rename = …)]` on both sums) so fixture and API decoders
 * round-trip without a rename table (T25 / S4).
 */
export interface StackedPoint {
  /** ISO-8601 date string. */
  date: string;
  cumulativeSharesVested: bigint;
  cumulativeTimeVestedAwaitingLiquidity: bigint;
  perGrantBreakdown: PerGrantDelta[];
}

/**
 * A per-employer stacked curve + the grants that feed it. Alias name in
 * the DTO is `EmployerStack` on the wire; here we expose it as
 * `StackedCurve` to match the deliverable signature.
 */
export interface StackedCurve {
  employerName: string;
  employerKey: string;
  grantIds: string[];
  points: StackedPoint[];
}

/** Mirror of `orbit_core::stacked_grants::StackedDashboard`. */
export interface StackedDashboard {
  byEmployer: StackedCurve[];
  combined: StackedPoint[];
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Case-insensitive employer-name compare per AC-8.2.1: trim whitespace,
 * lowercase. Mirror of Rust `normalize_employer`. Only used as a join
 * key; never surfaced to users.
 */
export function normalizeEmployer(name: string): string {
  return name.trim().toLowerCase();
}

/** Lexicographic compare of ISO-8601 date strings == calendar compare. */
function cmpIsoDate(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Lexicographic compare of ISO-8601 timestamps == chronological compare. */
function cmpIsoTs(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function cmpString(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

// ---------------------------------------------------------------------------
// Public entrypoints (mirror of Rust crate's exports)
// ---------------------------------------------------------------------------

/**
 * Merge + walk the vesting events for one employer's grants. Events
 * outside `grants` (by `grantId`) are ignored — callers that filtered
 * server-side can pass the already-filtered stream; this fn filters
 * internally for robustness against broader streams.
 */
export function stackCumulativeForEmployer(
  _employer: string,
  grants: GrantMeta[],
  events: VestingEvent[],
): StackedPoint[] {
  const grantIds = new Set<string>(grants.map((g) => g.id));
  const scoped = events.filter((e) => grantIds.has(e.grantId));
  return stackInternal(grants, scoped);
}

/**
 * Top-level: group by normalized employer, stack each group, and also
 * compute the combined envelope across every grant. Mirror of Rust
 * `stack_dashboard`.
 */
export function stackAllGrants(
  grants: GrantMeta[],
  events: VestingEvent[],
): StackedDashboard {
  // 1. Group by normalized employer.
  const buckets = new Map<string, GrantMeta[]>();
  for (const m of grants) {
    const key = normalizeEmployer(m.employerName);
    const arr = buckets.get(key);
    if (arr) arr.push(m);
    else buckets.set(key, [m]);
  }

  // 2. Per-employer stacks.
  const byEmployer: StackedCurve[] = [];
  for (const [key, groupMetas] of buckets) {
    // Pick the display name from the most-recently-created grant (so a
    // later correction like "acme inc." wins over an earlier "ACME Inc.").
    const sorted = [...groupMetas].sort((a, b) => {
      const c = cmpIsoTs(b.createdAt, a.createdAt);
      if (c !== 0) return c;
      return cmpString(b.id, a.id);
    });
    const display = sorted[0]!.employerName;
    const ids = new Set<string>(groupMetas.map((m) => m.id));
    const groupEvents = events.filter((e) => ids.has(e.grantId));
    const points = stackInternal(groupMetas, groupEvents);
    byEmployer.push({
      employerName: display,
      employerKey: key,
      grantIds: groupMetas.map((m) => m.id),
      points,
    });
  }
  byEmployer.sort((a, b) => cmpString(a.employerName, b.employerName));

  // 3. Combined envelope across every grant (regardless of employer).
  const combined = stackInternal(grants, events);

  return { byEmployer, combined };
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

function stackInternal(grants: GrantMeta[], events: VestingEvent[]): StackedPoint[] {
  if (grants.length === 0 || events.length === 0) return [];

  const metaById = new Map<string, GrantMeta>();
  for (const m of grants) metaById.set(m.id, m);

  // Deterministic sort: vest_date ASC, then grant.created_at ASC,
  // then grant.id ASC. Mirror of Rust impl.
  const merged = [...events].sort((a, b) => {
    const d = cmpIsoDate(a.vestDate, b.vestDate);
    if (d !== 0) return d;
    const ma = metaById.get(a.grantId);
    const mb = metaById.get(b.grantId);
    if (ma && mb) {
      const c = cmpIsoTs(ma.createdAt, mb.createdAt);
      if (c !== 0) return c;
    }
    return cmpString(a.grantId, b.grantId);
  });

  const points: StackedPoint[] = [];
  const runningVested = new Map<string, bigint>();
  const runningAwaiting = new Map<string, bigint>();

  let i = 0;
  while (i < merged.length) {
    const currentDate = merged[i]!.vestDate;
    const breakdown: PerGrantDelta[] = [];

    while (i < merged.length && merged[i]!.vestDate === currentDate) {
      const ev = merged[i]!;
      const gid = ev.grantId;
      const instrument = metaById.get(gid)?.instrument ?? '';
      switch (ev.state) {
        case 'vested':
          runningVested.set(
            gid,
            (runningVested.get(gid) ?? 0n) + ev.sharesVestedThisEventScaled,
          );
          break;
        case 'time_vested_awaiting_liquidity':
          runningAwaiting.set(
            gid,
            (runningAwaiting.get(gid) ?? 0n) + ev.sharesVestedThisEventScaled,
          );
          break;
        case 'upcoming':
          // Upcoming events still emit a breakdown row but don't advance
          // the vested-to-date sums (AC-8.2.5).
          break;
      }
      const cumulativeForThisGrant =
        (runningVested.get(gid) ?? 0n) + (runningAwaiting.get(gid) ?? 0n);
      breakdown.push({
        grantId: gid,
        instrument,
        sharesVestedThisEvent: ev.sharesVestedThisEventScaled,
        cumulativeForThisGrant,
        state: ev.state,
      });
      i += 1;
    }

    let cumulativeVested = 0n;
    for (const v of runningVested.values()) cumulativeVested += v;
    let cumulativeAwaiting = 0n;
    for (const v of runningAwaiting.values()) cumulativeAwaiting += v;

    points.push({
      date: currentDate,
      cumulativeSharesVested: cumulativeVested,
      cumulativeTimeVestedAwaitingLiquidity: cumulativeAwaiting,
      perGrantBreakdown: breakdown,
    });
  }

  return points;
}
