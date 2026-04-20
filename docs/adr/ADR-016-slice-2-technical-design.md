# ADR-016: Slice-2 technical design

- **Status:** Proposed
- **Date:** 2026-04-19
- **Deciders:** Ivan (owner)
- **Traces to:** `docs/requirements/slice-2-acceptance-criteria.md` (authoritative for this slice), `docs/requirements/v1-slice-plan.md` v1.3 (Slice 2 non-goals), ADR-005 (entities — `espp_purchases`, `art_7p_trips` outlines), ADR-009 (frontend), ADR-010 (API), ADR-011 (auth, session device list), ADR-014 (Slice-1 DDL — grants, residency_periods, vesting_events, sessions, audit_log, users), ADR-015 (local-only), SEC-020..SEC-026 (RLS), SEC-050 (log allowlist), SEC-054 (ip_hash HMAC), SEC-100..SEC-103 (audit log), SEC-160..SEC-163 (rate limit + validation), UX refs `grant-detail.html`, `dashboard.html`, `dashboard-slice-1.html`, `session-management.html`, `residency-setup.html`, `espp-purchase-form.html`, `art-7p-trip-form.html`, `modelo-720-inputs.html`, `multi-grant-dashboard.html`.

## Context

Slice 2's boundary is explicit: **multi-grant dashboard + stacked refresh view · ESPP purchase capture · Art. 7.p trip entry (with inline eligibility checklist) · Modelo 720 category inputs on Profile (time-series) · Session / device list UI in Account**. No CSV import. No ETrade PDF import. No tax numbers. No FX / EUR conversion. No Modelo 720 threshold alert. No scenarios. No sell-now. No billing, ever.

Per the Slice-2 acceptance-criteria document this is "implementation-ready" on the requirements side but leaves concrete DDL for three new user-scoped tables, the stacked-cumulative multi-grant algorithm, the ESPP-notes-to-purchase lift migration, the session-country storage decision, and the end-to-end sequence diagrams to the architect. This ADR produces all of those and a short list of what Slice 2 explicitly defers (so the implementation engineer is never ambiguous about a TBD).

Two load-bearing assumption sets from Slice 1 carry forward unchanged:

- C-4 (EUR conversion deferred to Slice 3): no FX path in Slice 2. Share counts and user-entered EUR totals are the only numeric surfaces.
- C-7 (session device list UI): the backend list + revoke endpoints shipped partial in Slice 1 per ADR-011 §"What Slice 1 actually ships". Slice 2 closes the UI and extends the backend shape where the acceptance criteria demand it (coarse location hint, bulk revoke-all-others).

One new architectural-compromise retirement carries forward: Slice 1 T13b packed `{"estimated_discount_percent": N}` into `grants.notes` for ESPP grants (see `backend/crates/orbit-api/src/handlers/grants.rs::merge_notes`). Slice 2 introduces `espp_purchases` as the long-term home for that field and specifies the non-lossy lift path. This ADR pins the path.

## Decision

### 1. Slice-2 DDL (concrete)

All migrations live under `migrations/`. Numbering is `YYYYMMDDHHMMSS_label.sql` and must sort strictly after the last-landed migration `20260509120000_harden_security_definer_search_path.sql`. Slice 2 appends one migration: `20260516120000_slice_2.sql` (ISO timestamp chosen one week after Slice-1 hardening).

Below is the **authoritative DDL for the tables Slice 2 adds** — `espp_purchases`, `art_7p_trips`, `modelo_720_user_inputs`, plus the one additive column `sessions.country_iso2`. All other Slice-1 tables (`grants`, `residency_periods`, `vesting_events`, `users`, `sessions`, `audit_log`, `email_verifications`, `password_reset_tokens`) are left untouched; the compromise `grants.notes` column is retained for read-compatibility and its JSON payload is cleared at first-purchase time per §2 below.

```sql
-- migrations/20260516120000_slice_2.sql (Slice 2 additions)
--
-- Traces to:
--   - ADR-016 §1 (authoritative DDL for espp_purchases, art_7p_trips,
--     modelo_720_user_inputs; additive column sessions.country_iso2).
--   - docs/requirements/slice-2-acceptance-criteria.md §4 (ESPP purchases),
--     §5 (Art. 7.p trips), §6 (Modelo 720 inputs), §7 (sessions UI).
--   - ADR-014 §1 for the reused touch_updated_at() function and the
--     tenant_isolation RLS policy-name convention.
--
-- Scope: three user-scoped tables + one column add. Every new table is
-- ENABLE ROW LEVEL SECURITY with a `tenant_isolation` policy keyed off
-- `app.user_id` (SEC-020..023). No new extensions required.

-- ESPP PURCHASES ---------------------------------------------------------
-- AC-4.1..AC-4.5. One row per ESPP purchase window; the parent grant
-- must have instrument='espp' — enforced via a BEFORE-INSERT/UPDATE
-- trigger (see grants_enforce_espp_parent below). PostgreSQL subqueries
-- are not allowed in CHECK constraints, so a trigger is the correct
-- mechanism for the "grant_id references an ESPP grant" assertion.
CREATE TABLE espp_purchases (
  id                          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  grant_id                    UUID NOT NULL REFERENCES grants(id) ON DELETE CASCADE,
  offering_date               DATE NOT NULL,
  purchase_date               DATE NOT NULL,
  fmv_at_purchase             NUMERIC(20,6) NOT NULL CHECK (fmv_at_purchase > 0),
  purchase_price_per_share    NUMERIC(20,6) NOT NULL CHECK (purchase_price_per_share > 0),
  shares_purchased            NUMERIC(20,4) NOT NULL CHECK (shares_purchased > 0),
  currency                    TEXT NOT NULL CHECK (currency IN ('USD','EUR','GBP')),
  fmv_at_offering             NUMERIC(20,6)
                              CHECK (fmv_at_offering IS NULL OR fmv_at_offering > 0),
  employer_discount_percent   NUMERIC(5,2)
                              CHECK (employer_discount_percent IS NULL
                                     OR (employer_discount_percent >= 0
                                         AND employer_discount_percent <= 100)),
  notes                       TEXT CHECK (notes IS NULL OR length(notes) <= 2048),
  created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
  CONSTRAINT purchase_after_offering CHECK (purchase_date >= offering_date)
);

CREATE INDEX espp_purchases_user_grant_date_idx
  ON espp_purchases (user_id, grant_id, purchase_date DESC);

ALTER TABLE espp_purchases ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON espp_purchases
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

CREATE TRIGGER espp_purchases_touch_updated_at
  BEFORE UPDATE ON espp_purchases
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- Trigger: parent grant must be an ESPP grant (AC-4.4.1).
-- Why a trigger, not a CHECK: PostgreSQL disallows subqueries in CHECK
-- constraints. A partial unique index cannot express the predicate
-- either. A trigger is the boring, correct mechanism and its cost is one
-- row lookup per insert/update, which is dominated by the FK check
-- Postgres is already performing. The function is STABLE and
-- SECURITY INVOKER (no definer bypass — the trigger fires under the
-- caller's privileges, which already hold `app.user_id` = owner).
CREATE OR REPLACE FUNCTION espp_purchases_enforce_grant_instrument()
RETURNS TRIGGER AS $$
DECLARE
  parent_instrument TEXT;
BEGIN
  SELECT instrument INTO parent_instrument
    FROM grants
   WHERE id = NEW.grant_id;
  IF parent_instrument IS NULL THEN
    RAISE EXCEPTION 'espp_purchases: parent grant % not found', NEW.grant_id
      USING ERRCODE = 'foreign_key_violation';
  END IF;
  IF parent_instrument <> 'espp' THEN
    RAISE EXCEPTION
      'espp_purchases: parent grant % has instrument=%, expected espp',
      NEW.grant_id, parent_instrument
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql
   STABLE;

CREATE TRIGGER espp_purchases_enforce_grant_instrument_trg
  BEFORE INSERT OR UPDATE OF grant_id ON espp_purchases
  FOR EACH ROW EXECUTE FUNCTION espp_purchases_enforce_grant_instrument();

-- ART. 7.P TRIPS ---------------------------------------------------------
-- AC-5.1..AC-5.3. Five fact fields + the five-criterion eligibility
-- checklist stored as JSONB to preserve Slice-4 flexibility (the
-- requirements-analyst may add a sixth criterion without a schema
-- migration). The column CHECK asserts object shape; the handler
-- validates keys and value types before writing (SEC-163). If the
-- `postgres-json-schema` extension is later added, the handler check
-- is augmented with `jsonb_matches_schema` and the column CHECK is
-- tightened; until then, application-layer validation is the source
-- of truth for shape.
CREATE TABLE art_7p_trips (
  id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  destination_country    TEXT NOT NULL CHECK (length(destination_country) = 2),
  from_date              DATE NOT NULL,
  to_date                DATE NOT NULL CHECK (to_date >= from_date),
  employer_paid          BOOLEAN NOT NULL,
  purpose                TEXT CHECK (purpose IS NULL OR length(purpose) <= 1024),
  eligibility_criteria   JSONB NOT NULL DEFAULT '{}'::jsonb
                          CHECK (jsonb_typeof(eligibility_criteria) = 'object'),
  created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX art_7p_trips_user_from_date_idx
  ON art_7p_trips (user_id, from_date DESC);

ALTER TABLE art_7p_trips ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON art_7p_trips
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

CREATE TRIGGER art_7p_trips_touch_updated_at
  BEFORE UPDATE ON art_7p_trips
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- MODELO 720 USER INPUTS -------------------------------------------------
-- AC-6.1..AC-6.3. Time-series with close-and-create semantics (same
-- pattern as residency_periods). Two categories, one row per
-- (user, category, from_date). Only the securities category is NOT
-- represented here — it is computed from grants via FX in Slice 3 and
-- the UI stubs it per AC-6.1.5.
--
-- `category` is an enum-like TEXT column; currently two values; a
-- future 'securities_manual_override' variant is easy to add.
CREATE TABLE modelo_720_user_inputs (
  id                         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  category                   TEXT NOT NULL
                              CHECK (category IN ('bank_accounts','real_estate')),
  amount_eur                 NUMERIC(20,2) NOT NULL CHECK (amount_eur >= 0),
  reference_date             DATE NOT NULL,
  from_date                  DATE NOT NULL,
  to_date                    DATE,
  created_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
  CONSTRAINT m720_to_after_from CHECK (to_date IS NULL OR to_date >= from_date)
);

-- One open row per (user, category) at a time.
CREATE UNIQUE INDEX modelo_720_user_inputs_open_idx
  ON modelo_720_user_inputs (user_id, category) WHERE to_date IS NULL;

-- Scan pattern: list history by user + category, newest first.
CREATE INDEX modelo_720_user_inputs_user_category_from_idx
  ON modelo_720_user_inputs (user_id, category, from_date DESC);

ALTER TABLE modelo_720_user_inputs ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON modelo_720_user_inputs
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);
-- No updated_at trigger: the row is append-only from the handler's POV
-- (close-and-create). Only `to_date` is ever mutated, by the same
-- transaction that inserts the successor row; that is an audited lift,
-- not an in-place edit.

-- SESSIONS — additive column for creation-time country lookup ------------
-- AC-7.1.3. The sessions UI surfaces a coarse geo hint; raw IP is never
-- displayed or serialized. Because `ip_hash` is HMAC-SHA256 and not
-- reversible, the country lookup must happen at session-creation time
-- (the only point in the pipeline where a raw IP still exists in RAM;
-- SEC-054). We store the ISO 3166-1 alpha-2 code on the session row and
-- expose it verbatim to the UI; the raw IP and the ip_hash are never
-- read by any list handler.
--
-- Privacy note: two-letter country code has markedly lower entropy than
-- raw IP and is commensurate with the user's own expectation (the UI
-- reads "Madrid, ES (aprox.)" — the city string is inferred from
-- country + locale, not from geoip). If the GeoIP database is
-- unavailable at session creation time (e.g., offline dev), the column
-- is left NULL and the UI renders `ubicación desconocida` /
-- `location unknown`; see the sequence diagram in §6.
ALTER TABLE sessions
  ADD COLUMN country_iso2 TEXT
    CHECK (country_iso2 IS NULL OR length(country_iso2) = 2);

-- Ownership (mirrors 20260425120000_slice_1.sql §Ownership).
ALTER TABLE espp_purchases            OWNER TO orbit_migrate;
ALTER TABLE art_7p_trips              OWNER TO orbit_migrate;
ALTER TABLE modelo_720_user_inputs    OWNER TO orbit_migrate;
ALTER FUNCTION espp_purchases_enforce_grant_instrument() OWNER TO orbit_migrate;

-- Grants — orbit_app (full DML; RLS constrains visible rows).
GRANT SELECT, INSERT, UPDATE, DELETE ON espp_purchases         TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON art_7p_trips           TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON modelo_720_user_inputs TO orbit_app;
-- sessions already grants DML to orbit_app per 20260418120000_init.sql §5;
-- the column add inherits those grants.
```

**RLS policy naming convention (inherited).** Every `[RLS]` table has exactly one policy named `tenant_isolation`. Per SEC-020, the existing CI `pg_policies` introspection test covers the three new tables automatically — no test code changes beyond listing the new table names in the expected-set fixture.

**Why `eligibility_criteria` is JSONB and not five `BOOLEAN NULL` columns.** See §9 Assumptions; the short answer is: Slice-4 is likely to add a sixth criterion (the analyst flagged the US-005 wording as non-final in §5.2.2) and the JSONB shape absorbs that addition without a schema migration. The cost is that application-layer validation is now the source of truth for the five-keys shape. The keys the handler requires in the object are exactly: `services_outside_spain`, `non_spanish_employer`, `not_tax_haven`, `no_double_exemption`, `within_annual_cap`. Each value is `true`, `false`, or `null` (unanswered). The handler rejects any other key; the handler rejects any other value type; the handler rejects a missing key as 422 per AC-5.2.3. A Slice-4 migration that promotes this into a first-class schema is a typed-column expansion + a backfill — see §11.

**Why `sessions.country_iso2` and not a sidecar table.** One column on the existing row is the simplest thing that works. A `session_metadata` table would add a JOIN on every sessions-list query for zero benefit: the only metadata we want right now is a two-letter country code, and it is immutable once written (session creation is the only write point). If Slice 7 adds e.g. `last_known_country_iso2` (refreshed on each refresh-rotation) we revisit.

**Why `modelo_720_user_inputs` uses one table with a `category` column rather than two tables.** Symmetry with `residency_periods` (same close-and-create shape), plus the list-by-user-and-category index satisfies both the "current value" lookup (`… WHERE to_date IS NULL`) and the "full history" lookup (`… ORDER BY from_date DESC`). The two categories behave identically at the DDL level; the policy difference (user-entered EUR for both) is a property of the data, not the schema. Adding a third category (securities manual override, if Slice 3's computed value ever grows a manual-override affordance) is a CHECK-constraint expansion.

### 2. Retire the Slice-1 `grants.notes` ESPP compromise

**Path: one-time lift on first purchase.** No schema migration retires the notes column. The existing Slice-1 helper `merge_notes` in `backend/crates/orbit-api/src/handlers/grants.rs` packs `{"estimated_discount_percent": N}` (optionally with a user `note` field) into `grants.notes` for ESPP grants; this is read-only compatible from Slice 2 onward. The lift fires exactly once per ESPP grant, at first-purchase-creation time, inside the same database transaction as the `espp_purchases` INSERT.

Pseudo-code (to be realized in `backend/crates/orbit-api/src/handlers/espp_purchases.rs::create`):

```text
fn create_espp_purchase(tx: Tx, user_id: Uuid, body: CreatePurchaseBody)
    -> Result<CreatePurchaseResponse>:
  // Verify the grant belongs to the user and is ESPP (404 if not — RLS
  // caught the cross-tenant case; the trigger below catches the wrong
  // instrument, and the handler checks explicitly for a cleaner 422).
  let grant = tx.query_one(
    "SELECT id, instrument, notes FROM grants WHERE id = $1",
    &[&body.grant_id]
  ).await?;

  if grant.instrument != "espp":
    return Err(err("grant.invalid.not_espp", 422))

  // Lift: if this is the first purchase for the grant AND grants.notes
  // carries estimated_discount_percent, pull it onto the purchase and
  // rewrite grants.notes to either NULL or the user's free-text note.
  let is_first_purchase = tx.query_one(
    "SELECT COUNT(*)::int FROM espp_purchases WHERE grant_id = $1",
    &[&grant.id]
  ).await? == 0;

  let (effective_discount, rewritten_notes, migrated) =
    if is_first_purchase and grant.notes is Some:
      match parse_slice1_espp_notes(grant.notes) {
        Some(LiftedNotes { discount, user_note }) =>
          (
            body.employer_discount_percent.or(Some(discount)),
            user_note,          // may be None → `notes = NULL`
            true
          )
        None =>
          (body.employer_discount_percent, grant.notes, false)
      }
    else:
      (body.employer_discount_percent, grant.notes, false)

  tx.execute(
    "INSERT INTO espp_purchases (..., employer_discount_percent, ...) \
     VALUES (..., $N, ...)",
    &[..., &effective_discount, ...]
  ).await?;

  if migrated:
    tx.execute(
      "UPDATE grants SET notes = $1 WHERE id = $2",
      &[&rewritten_notes, &grant.id]
    ).await?;

  tx.insert_audit("espp_purchase.create", target_id = purchase.id,
                  payload = json!({ "grant_instrument": "espp" }));

  if migrated:
    tx.insert_audit(
      "grant.update",
      target_id = grant.id,
      payload = json!({ "fields_changed": ["notes"] })
    );

  return Ok(CreatePurchaseResponse { purchase, migrated_from_notes: migrated });


fn parse_slice1_espp_notes(raw: &str) -> Option<LiftedNotes>:
  // Slice-1 shape A: {"estimated_discount_percent": N}
  // Slice-1 shape B: {"estimated_discount_percent": N, "note": "..."}
  // Anything else → None (leave as-is; do not touch the column).
  let v = serde_json::from_str::<serde_json::Value>(raw).ok()?;
  let pct = v.get("estimated_discount_percent")?.as_f64()?;
  let user_note = v.get("note").and_then(|n| n.as_str()).map(String::from);
  Some(LiftedNotes {
    discount: Decimal::from_f64_retain(pct)?.round_dp(2),
    user_note,
  })
```

**Determinism guarantees.**
- The lift fires at most once per ESPP grant because the `is_first_purchase` guard depends on the count of existing `espp_purchases` rows, which becomes ≥ 1 the moment this transaction commits. Subsequent purchases see `migrated = false`.
- If the user supplies `employer_discount_percent` explicitly, the handler honors the user's value — the Slice-1 JSON is treated as a default, not an override. This matches AC-4.5.2's "pre-fills" wording.
- If `grants.notes` is not parseable as a Slice-1 ESPP JSON blob (e.g., a user-entered free-text note), the lift is a no-op and the column is left alone.
- If the parse succeeds but extracts a discount the CHECK constraint rejects (e.g., 150 %), the INSERT fails and the transaction rolls back — the user sees a 422 and the JSON stays put. No silent data mutation.

**No destructive migration.** The `grants.notes` column survives unchanged; any existing ESPP grant with the JSON blob continues to load and render correctly until the user opens the "Record purchase" form on that grant. This makes the lift reversible in the purest sense: `UPDATE grants SET notes = '{"estimated_discount_percent": <value-from-purchase>}' WHERE id = ?` is the inverse, and it is available at any time.

### 3. API contract additions

Concrete for Slice 2. Path-relative to `/api/v1`. Notation inherited from ADR-010 §9: `[A]` = authenticated; `[V]` = CSRF-validated state change. All mutation endpoints go through `Tx::for_user(user_id)` per SEC-022.

**ESPP purchases**

| Method | Path | Notes |
|---|---|---|
| `POST`   | `/grants/:grant_id/espp-purchases` `[A]` `[V]` | Body: `{ offeringDate, purchaseDate, fmvAtPurchase, purchasePricePerShare, sharesPurchased, currency, fmvAtOffering?, employerDiscountPercent?, notes? }`. Validator: all CHECK constraints + the `purchase_date >= offering_date` cross-field + duplicate-purchase soft-warn triple (AC-4.2.8, handled by the frontend via a `forceDuplicate: bool` flag in the body — 422 with `code="espp_purchase.duplicate_warn"` on first attempt; 201 on second attempt with `forceDuplicate = true`). Response: `201 { purchase, migratedFromNotes: bool }`. Side-effects: Slice-1 notes lift per §2; audit row per G-32. |
| `GET`    | `/grants/:grant_id/espp-purchases` `[A]` | List purchases for one ESPP grant; `ORDER BY purchase_date DESC`; paginated (cursor-based per ADR-010 §6). Response: `{ items: [...], nextCursor: string|null }`. |
| `GET`    | `/espp-purchases/:id` `[A]` | Single purchase; RLS-scoped, 404 if not owned. |
| `PUT`    | `/espp-purchases/:id` `[A]` `[V]` | Full replace. Same validators as POST. Audit `espp_purchase.update` per G-32. |
| `DELETE` | `/espp-purchases/:id` `[A]` `[V]` | Hard delete. Returns 204. Audit `espp_purchase.delete` per G-32. |

**Art. 7.p trips**

| Method | Path | Notes |
|---|---|---|
| `POST`   | `/trips` `[A]` `[V]` | Body: `{ destinationCountry, fromDate, toDate, employerPaid, purpose, eligibilityCriteria: { servicesOutsideSpain: bool\|null, nonSpanishEmployer: bool\|null, notTaxHaven: bool\|null, noDoubleExemption: bool\|null, withinAnnualCap: bool\|null } }`. Validator: AC-5.2.3 rejects any null in the five checklist keys at save time; AC-5.2.4 rejects `to_date < from_date` (also enforced by the column CHECK); AC-5.2.6 rejects empty `destination_country`; AC-5.2.7 does NOT reject overlaps or `destination_country = 'ES'`. Audit `art_7p_trip.create` per G-32. |
| `GET`    | `/trips` `[A]` | List; `ORDER BY from_date DESC`. Response adds an `annualCapTracker` object: `{ year: 2026, dayCountDeclared: int, tripCount: int, employerPaidTripCount: int, capReferenceEur: "60100.00", capApplied: false }`. Purely informational, per AC-5.1.3 / AC-5.1.4 (Slice 2 **does not** compute a monetary amount; it counts days and trips). The year defaults to the current calendar year; a `?year=YYYY` query parameter overrides. |
| `GET`    | `/trips/:id` `[A]` | 404 if not owned. |
| `PUT`    | `/trips/:id` `[A]` `[V]` | Full replace. Audit `art_7p_trip.update`. AC-5.3.5: no background job ever mutates the checklist booleans. |
| `DELETE` | `/trips/:id` `[A]` `[V]` | Hard delete. 204. Audit `art_7p_trip.delete`. |

**Modelo 720 user inputs**

| Method | Path | Notes |
|---|---|---|
| `POST`   | `/modelo-720-inputs` `[A]` `[V]` | Body: `{ category, amountEur, referenceDate? }`. Close-and-create semantics per AC-6.2.2: inside one transaction, `UPDATE modelo_720_user_inputs SET to_date = today WHERE user_id = $1 AND category = $2 AND to_date IS NULL;` then `INSERT ... (from_date = today, to_date = NULL)`. **Same-day idempotency (AC-6.2.3, clarified):** if the currently-open row already has `from_date = today`, the handler **updates that row's `amount_eur`** in place rather than closing-and-creating again, avoiding a 1-day span row on accidental double-save. If the incoming `amountEur` equals the currently-open row's value (whether same-day or not), the handler no-ops and writes no audit row (AC-6.2.5). Audit `modelo_720_inputs.upsert` with `payload_summary = { categories_changed: [category] }`. |
| `GET`    | `/modelo-720-inputs/current` `[A]` | Returns a two-row array keyed by category (one for `bank_accounts`, one for `real_estate`; null-amount entries for categories the user has never saved — AC-6.1.4 distinguishes "zero" from "never-set"). |
| `GET`    | `/modelo-720-inputs` `[A]` | Full history; `ORDER BY category, from_date DESC`. Cursor-paginated. Not surfaced in Slice-2 UI (AC-6.2.6), but available to Slice-3's threshold-alert handler. |

**Sessions / device list**

The backend list + single-revoke endpoints shipped partial in Slice 1 (ADR-011 §"What Slice 1 actually ships"); Slice 2 extends the list response shape and adds the bulk-revoke endpoint. All three endpoints go through `Tx::for_user(user_id)`.

| Method | Path | Notes |
|---|---|---|
| `GET`    | `/auth/sessions` `[A]` | Lists the caller's active sessions (`WHERE user_id = app.user_id AND revoked_at IS NULL`). Response item shape: `{ id, userAgent, countryIso2, createdAt, lastUsedAt, isCurrent: bool }`. **Never** returns `session_id_hash`, `refresh_token_hash`, `ip_hash`, or `family_id`. `isCurrent` is computed by the handler by comparing the session row's `id` to the request's own session `id` (resolved via `lookup_session_by_hash`; see migration `20260502120000_t13a_session_lookup.sql`). |
| `DELETE` | `/auth/sessions/:session_id` `[A]` `[V]` | Revokes one other session. 403 with `code="session.cannot_revoke_current"` if the target is the request's own session (AC-7.2.3 — defense-in-depth mirror of the UI guard). 404 with `code="resource.not_found"` if the session does not exist or was already revoked (AC-10.6 stale-tab handling). On success, `UPDATE sessions SET revoked_at = now(), revoke_reason = 'admin' WHERE id = $1` (reason = `admin` is the user-initiated-from-UI case; `user_signout` stays reserved for the current-session signout path per ADR-011 §Signout). Audit `session.revoke` with `payload_summary = { revoke_kind: "single", initiator: "self" }`. Returns 204. |
| `POST`   | `/auth/sessions/revoke-all-others` `[A]` `[V]` | Bulk revoke of all non-current non-revoked sessions for the caller. `UPDATE sessions SET revoked_at = now(), revoke_reason = 'admin' WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL` (where `$2` is the current session id). Response: `200 { revokedCount: int }`. Audit `session.revoke` with `payload_summary = { revoke_kind: "all_others", initiator: "self", count: int }` — the `count` field is the only allowlisted numeric in any Slice-2 audit payload and is included because AC-7.2.2's demo script asserts it exists. |

**Error envelope.** Unchanged from ADR-010 §7. Every validator error surfaces via the `errors: [{ field, code, message, messageEn }]` multi-field shape (AC-10.4 — validator-caught and CHECK-constraint-caught errors are indistinguishable to the UI). `espp_purchases_enforce_grant_instrument_trg` raises ERRCODE `check_violation`, which the handler maps to `code="espp_purchase.invalid.parent_grant_instrument"` + 422.

**Rate-limit headers.** Unchanged (SEC-160).

### 4. Multi-grant stacked cumulative algorithm

Pseudocode for the function called by both the **backend** (on dashboard load; writes nothing new — it reads `vesting_events` that Slice 1 already materialized) and the **frontend** (live render of the stacked chart). Both implementations are deterministic and produce **identical** envelope and per-grant curves for the same input (AC-8.2.8 property test).

Implementation note: the backend is the source of truth. The frontend reimplements the same logic purely for the chart render; on cache refresh the server's output is authoritative and the frontend's computation is discarded. A property-based test (`proptest`) pins the backend; a Vitest test pins the frontend against the same fixture cases.

```text
type StackedPoint {
  date: Date,
  cumulative_shares_vested: Decimal,            // sum across all grants
  cumulative_time_vested_awaiting_liquidity: Decimal,
  per_grant_breakdown: Vec<PerGrantDelta>,
}

type PerGrantDelta {
  grant_id: Uuid,
  shares_vested_this_event: Decimal,
  cumulative_for_this_grant: Decimal,
}

fn stack_cumulative_for_employer(
  employer: String,
  grants: Vec<Grant>,                // already filtered to same employer
  events_by_grant: HashMap<Uuid, Vec<VestingEvent>>,
) -> Vec<StackedPoint>:
  // All grants passed in share the same normalized employer name
  // (case-insensitive compare, trimmed whitespace). The grouping step
  // is the caller's responsibility; see stack_all_grants below.

  // 1. Merge events across grants into a single sequence keyed by
  //    (vest_date, grant_created_at ASC, grant_id ASC). The
  //    deterministic tie-break (AC-8.2.8) prevents any per-run ordering
  //    difference between backend and frontend — the `grants.created_at`
  //    column is already populated for every Slice-1 grant.
  let merged = events_by_grant
    .iter()
    .flat_map(|(gid, evs)| evs.iter().map(|e| (gid, e)))
    .collect::<Vec<_>>();

  let grant_created_at = grants.iter()
    .map(|g| (g.id, g.created_at))
    .collect::<HashMap<_, _>>();

  merged.sort_by(|a, b|
    a.1.vest_date.cmp(&b.1.vest_date)
      .then(grant_created_at[a.0].cmp(&grant_created_at[b.0]))
      .then(a.0.cmp(b.0))
  );

  // 2. Walk the merged sequence, emitting one StackedPoint per distinct
  //    vest_date. At each date, per_grant_breakdown carries one entry
  //    per grant that vested on that date (not the grants that did
  //    not — the UI fills those in on render).
  let mut points = Vec::new();
  let mut running_vested: HashMap<Uuid, Decimal> = HashMap::new();
  let mut running_time_awaiting: HashMap<Uuid, Decimal> = HashMap::new();

  for (date, same_date_events) in group_by(merged, |(gid, e)| e.vest_date):
    let mut breakdown = Vec::new();
    for (gid, event) in same_date_events:
      match event.state {
        Vested =>
          *running_vested.entry(gid).or_default() += event.shares_vested_this_event;
        TimeVestedAwaitingLiquidity =>
          *running_time_awaiting.entry(gid).or_default() += event.shares_vested_this_event;
        Upcoming =>
          // Upcoming events are included in the chart's future axis but
          // do not contribute to the "vested-to-date" sum surfaces; the
          // per_grant_breakdown still carries them so the chart can
          // render the future segment.
          ()
      }
      breakdown.push(PerGrantDelta {
        grant_id: gid,
        shares_vested_this_event: event.shares_vested_this_event,
        cumulative_for_this_grant: running_vested[&gid] + running_time_awaiting[&gid],
      });

    points.push(StackedPoint {
      date,
      cumulative_shares_vested: running_vested.values().sum(),
      cumulative_time_vested_awaiting_liquidity: running_time_awaiting.values().sum(),
      per_grant_breakdown: breakdown,
    });

  return points;


fn stack_all_grants(grants: Vec<Grant>, events: HashMap<Uuid, Vec<VestingEvent>>)
    -> StackedDashboard:
  // Case-insensitive employer-name compare + whitespace trim per AC-8.2.1.
  let normalize = |s: &str| s.trim().to_lowercase();

  let mut by_employer: HashMap<String, Vec<Grant>> = HashMap::new();
  for g in grants:
    by_employer.entry(normalize(&g.employer_name)).or_default().push(g);

  let mut employer_curves: Vec<(String, Vec<StackedPoint>)> = Vec::new();
  let mut single_tiles: Vec<Grant> = Vec::new();

  for (normalized_employer, employer_grants) in by_employer:
    if employer_grants.len() == 1:
      // AC-8.2.7 — no stack of size 1.
      single_tiles.push(employer_grants.into_iter().next().unwrap());
    else:
      // Display name = the employer_name of the most-recently-created
      // grant (stable tie-break: `grants.created_at DESC, id DESC`).
      // This prevents "Acme" and "ACME" from being merged under a weird
      // canonicalized label while still honoring the case-insensitive
      // grouping rule.
      let display = employer_grants.iter()
        .max_by_key(|g| (g.created_at, g.id))
        .unwrap().employer_name.clone();
      let events_subset = employer_grants.iter()
        .map(|g| (g.id, events[&g.id].clone()))
        .collect();
      employer_curves.push((display,
        stack_cumulative_for_employer(display.clone(), employer_grants, events_subset)));

  // Flat "all grants" envelope: sum per_grant_breakdown across
  // employer_curves + single_tiles at each distinct date. Rendered as
  // an optional overlay in the UI (Slice-2 UX does not surface it
  // prominently; it is computed because AC-8.2.8's property test
  // asserts the per-employer sums equal the all-grants envelope at
  // every date).
  let all_grants_envelope = merge_envelopes(employer_curves, single_tiles, events);

  return StackedDashboard { employer_curves, single_tiles, all_grants_envelope };
```

**Mixed-instrument stacking (AC-8.2.4, AC-8.2.5).** RSU + NSO under one employer render together; their per-grant breakdowns are distinguishable in the chart legend by instrument. Double-trigger RSU contributions with `liquidity_event_date IS NULL` are summed into `cumulative_time_vested_awaiting_liquidity` (the dashed-fill envelope in the UI) rather than `cumulative_shares_vested` — the same `state_for` classification that Slice 1's `derive_vesting_events` already applied.

**Currency-aware stacking deferred (AC-8.2.6).** Each tile renders native currency per grant; EUR unification ships in Slice 3. The stacked chart's envelope is a share-count aggregate, not a monetary aggregate — so no FX is required in Slice 2.

**Deterministic ordering (AC-8.2.8 property).** Tie-break rule is `(vest_date ASC, grant_created_at ASC, grant_id ASC)` — three-level stable sort. The CI property test asserts: for any random legal portfolio, the stacked envelope's `cumulative_shares_vested` at date D equals the sum of its constituent grants' `vested_to_date(events, D).0` values (the Slice-1 helper from ADR-014 §2).

**Parity discipline.** Backend authoritative, frontend parity mirror. Shared JSON fixture file extends T15's `vesting_cases.json`: the new file is `stacked_grants_cases.json`, co-located with the Slice-1 fixture in `backend/crates/orbit-core/tests/fixtures/`. The Rust property test in `orbit-core` and the Vitest unit test in `frontend/src/lib/vesting` both consume it. A failed fixture asserts is a CI hard fail — no platform-specific floating-point drift is permitted (the algorithm is `rust_decimal` on the backend and `decimal.js` on the frontend; both are exact).

### 5. Sequence diagrams

Mermaid; matches the ADR-014 §4 shape. Two are worth writing in full — the rest (trip CRUD, M720 close-and-create, multi-grant dashboard load) are structurally identical to Slice-1 sequences with new endpoint names and are elided.

#### 5.1 Record ESPP purchase (with first-purchase notes lift)

```mermaid
sequenceDiagram
    autonumber
    participant U as User
    participant SPA as React SPA
    participant API as axum API
    participant PG as Postgres

    U->>SPA: opens /app/grants/:id/espp-purchases/new
    SPA->>API: GET /api/v1/grants/:id
    API->>PG: Tx::for_user — SELECT grants WHERE id = $1
    PG-->>API: grant (instrument='espp', notes='{"estimated_discount_percent":15}')
    API-->>SPA: 200 { grant }
    SPA->>SPA: parse notes JSON, prefill employer_discount_percent = 15 (AC-4.5.1)
    U->>SPA: fills offering_date, purchase_date, FMV, price, shares, currency; submits
    SPA->>API: POST /api/v1/grants/:id/espp-purchases  (x-csrf-token, camelCase body)
    API->>API: validator: purchase_date >= offering_date; positive numerics; currency in {USD,EUR,GBP}
    API->>PG: Tx::for_user (app.user_id = user)
    API->>PG: BEGIN
    API->>PG: SELECT id, instrument, notes FROM grants WHERE id = $1  -- confirm ESPP
    API->>PG: SELECT COUNT(*) FROM espp_purchases WHERE grant_id = $1  -- is_first_purchase?
    alt first purchase AND notes parses as Slice-1 ESPP JSON
        API->>API: parse_slice1_espp_notes → LiftedNotes { discount, user_note }
        API->>API: effective_discount = body.employer_discount_percent OR discount
        API->>API: rewritten_notes = user_note  (or NULL)
        API->>PG: INSERT INTO espp_purchases (... employer_discount_percent = effective_discount ...)
        API->>PG: BEFORE INSERT trigger: espp_purchases_enforce_grant_instrument_trg OK
        API->>PG: UPDATE grants SET notes = $1 WHERE id = $2
        API->>PG: INSERT audit_log (action='espp_purchase.create', payload={grant_instrument:'espp'})
        API->>PG: INSERT audit_log (action='grant.update',         payload={fields_changed:['notes']})
        API->>PG: COMMIT
        API-->>SPA: 201 { purchase, migratedFromNotes: true }
    else not first OR notes not parseable
        API->>PG: INSERT INTO espp_purchases (...)
        API->>PG: BEFORE INSERT trigger: espp_purchases_enforce_grant_instrument_trg OK
        API->>PG: INSERT audit_log (action='espp_purchase.create', payload={grant_instrument:'espp'})
        API->>PG: COMMIT
        API-->>SPA: 201 { purchase, migratedFromNotes: false }
    end
    SPA->>U: navigate back to /app/grants/:id with flash "Compra ESPP registrada."
    SPA->>API: GET /api/v1/grants/:id/espp-purchases  (refresh list)
    API->>PG: Tx::for_user — SELECT * FROM espp_purchases WHERE grant_id=$1 ORDER BY purchase_date DESC
    API-->>SPA: { items, nextCursor: null }
```

#### 5.2 Revoke a single non-current session

```mermaid
sequenceDiagram
    autonumber
    participant U as User
    participant SPA as React SPA
    participant API as axum API
    participant PG as Postgres

    U->>SPA: clicks "Cerrar esta sesión" on row id=$target
    SPA->>U: renders confirm modal
    U->>SPA: confirms
    SPA->>API: DELETE /api/v1/auth/sessions/$target  (x-csrf-token)
    API->>API: auth middleware: resolve current session via lookup_session_by_hash
    API->>API: current_session_id = resolved.id, current_user_id = resolved.user_id
    alt $target == current_session_id
        API-->>SPA: 403 { error: { code: "session.cannot_revoke_current" } }
        SPA->>U: inline advisory (should not reach — UI guard in AC-7.1.4)
    else
        API->>PG: Tx::for_user (app.user_id = current_user_id)
        API->>PG: BEGIN
        API->>PG: UPDATE sessions SET revoked_at = now(), revoke_reason = 'admin' \
                    WHERE id = $target AND user_id = app.user_id AND revoked_at IS NULL \
                    RETURNING id
        alt 0 rows updated
            API->>PG: ROLLBACK
            API-->>SPA: 404 { error: { code: "resource.not_found" } }
            SPA->>SPA: refetch session list (AC-10.6)
        else 1 row updated
            API->>PG: INSERT audit_log (action='session.revoke', target_kind='session', \
                       target_id=$target, payload={revoke_kind:'single', initiator:'self'})
            API->>PG: COMMIT
            API-->>SPA: 204
            SPA->>SPA: remove row from TanStack-Query cache; re-render
        end
    end
```

The `revoke-all-others` sequence is identical in shape to the single-revoke branch above except the `UPDATE` predicate is `id <> current_session_id AND revoked_at IS NULL`, the `RETURNING` clause counts rows, and the audit-log `payload_summary` carries `{ revoke_kind: "all_others", initiator: "self", count: <int> }`.

### 6. What Slice 2 explicitly defers (make TBD impossible)

The following are **designed but not implemented** in Slice 2. Each is listed here so the implementation engineer never sees a TBD.

| Deferred item | Slice | Note |
|---|---|---|
| EUR conversion / paper-gains tile | 3 | Per C-4 (inherited). Slice-2 dashboard tiles render native currency only; stacked chart is share counts only (AC-8.1.3, AC-8.2.6). |
| Rule-set chip in footer | 3 | Per AC-3.1 §G-5 re-confirmed. Slice-2 adds no calculation outputs → footer remains copy-only. |
| Modelo 720 threshold alert (the €50 000 passive banner) | 3 | Per AC-6.3.1. Slice-3 handler reads `modelo_720_user_inputs.to_date IS NULL` rows + FX. |
| Modelo 720 securities row (computed, with manual override) | 3 | Per AC-6.1.5 stub copy. Slice-3 computes from grants + FX. |
| Modelo 720 worksheet PDF | 6 | Per AC-6.3.3. |
| Tax numbers anywhere | 4 | Per §12. ESPP purchases render inputs, never a tax interpretation (AC-4.4.2). |
| Art. 7.p **calculation** (pro-rata, €60 100 cap application, 0-day omission, overlap-or-domestic rejection) | 4 | Per §5 scope reminder. Slice-2 stores the data only; AC-5.2.7 explicitly disables the Slice-4 rejections at capture time. |
| Art. 7.p eligibility-checklist evaluation (the chip currently says "Capturado — revisión pendiente (N/5)") | 4 | Per §12. Chip never says "exento" / "no exento" in Slice 2. |
| Scenario / sell-now CTAs beyond `próximamente` stubs | 4 / 5 | Per §12. |
| CSV Carta + Shareworks import; ETrade PDF import | 8 | Per v1.3 decision. "Tengo varios grants" copy updated per §9 of the AC doc; destination unchanged. |
| Export of any surface (ESPP list, trip list, M720 inputs, session list) | 6 | Per AC-6.3.3 + Slice-6 scope. |
| TOTP enrolment UI | 7 | Per AC-7.1.1. Sessions panel is unrelated to MFA state. |
| Full DSR self-service (export + erasure + rectify) | 7 | Slice-2 AC G-26 extends the Slice-1 data-minimization posture only. |
| Year-on-year Modelo-720 history in the UI | post-v1 | AC-6.2.6: rows accumulate in the DB; no UI surface before Slice-3 consumers. |
| "Recompute under current rules" action | 4 (dormant) / 5 (active) | No calculations in Slice 2. |
| Sensitivity ranges / Pattern-C range rail | 4 | No tax numbers → no ranges. |
| IP address rendered in cleartext (ever) | Never | SEC-054 boundary; the country code is the sole geo surface for v1. |
| Art. 7.p sixth eligibility criterion (if Slice 4 adds one) | 4 | JSONB shape absorbs an additive key; handler validator adds it to the allowlisted set. |

### 7. Performance and rate-limit targets applied to Slice 2

- `POST /grants/:grant_id/espp-purchases`, `POST /trips`, `POST /modelo-720-inputs` ≤ **200 ms P95** (dominated by validator + 1–3 INSERTs + 1 audit row; trivially within budget on a laptop-scale Postgres).
- `GET /grants` + stacked view full render on **20 grants** ≤ **2 s P75** for dashboard first paint on EU broadband (spec §7.8, inherited). The stacked algorithm is O(E log E) in total vesting events across all grants (E ≈ 20 × 48 = 960 in the worst Slice-2 case) — the sort dominates, and it is cheap.
- `DELETE /auth/sessions/:id` ≤ **150 ms P95**.
- `POST /auth/sessions/revoke-all-others` ≤ **300 ms P95** even at 20 active sessions (a single `UPDATE` + 1 audit row).
- Per-user rate limits (SEC-160; SQL-backed leaky bucket):
  - Write endpoints (`POST|PUT|DELETE` on purchases / trips / M720 / grants): **120 / user / hour**, unchanged from Slice 1.
  - `POST /auth/sessions/revoke-all-others`: **10 / user / minute** (tight, to prevent an accidental runaway from a misclick + retry).
  - `DELETE /auth/sessions/:id`: **60 / user / hour** (normal usage is a handful per week).

### 8. Test plan

Summary; detailed test work is `qa-engineer`'s (T23). The ADR pins what MUST be tested.

**Property / fixture tests (Rust + TS parity).**
- The multi-grant cumulative algorithm: random legal portfolio → assert `stacked_envelope(D) == Σ per_grant_vested_to_date(D)` at every distinct event date. Backend `proptest` + frontend `fast-check`; both consume `stacked_grants_cases.json`.
- Determinism: the same inputs fed in different insertion orders produce bit-identical output (three-level tie-break holds).

**Integration tests (backend, against a real Postgres).**
- **ESPP notes lift (AC-4.5.1).** Seed a Slice-1 ESPP grant with `grants.notes = '{"estimated_discount_percent": 15}'`; POST a first purchase with `employer_discount_percent` blank; assert the `espp_purchases` row has `employer_discount_percent = 15.00` and `grants.notes IS NULL` and the response carries `migratedFromNotes: true`.
- **Lift with user note (AC-4.5.1 edge).** Seed `grants.notes = '{"estimated_discount_percent": 12.5, "note": "April window"}'`; POST a first purchase; assert `grants.notes = 'April window'` (string, not JSON wrapping).
- **Lift no-op on non-JSON notes.** Seed `grants.notes = 'freeform user text'`; POST a first purchase; assert `grants.notes` is unchanged and `migratedFromNotes: false`.
- **Lift fires at most once.** Seed a Slice-1 ESPP grant with JSON notes; POST one purchase (lift fires), POST a second (lift does not fire); assert `grants.notes IS NULL` after the first call and no `grant.update` audit row for the second.
- **Parent-grant instrument guard.** POST a purchase for an RSU grant; assert 422 with `code="espp_purchase.invalid.parent_grant_instrument"` and zero rows in `espp_purchases`.
- **M720 close-and-create (AC-6.2.2).** Two same-day POSTs for `bank_accounts`; assert exactly one open row (`to_date IS NULL`) with the second value, zero 1-day spans, one audit row. Then one more POST on a later day; assert the prior row's `to_date = today` and a fresh open row.
- **M720 no-op on identical value (AC-6.2.5).** POST the same `amount_eur` twice (same day or not); assert one row and one audit row.
- **Cross-tenant probes (SEC-023).** User A creates a purchase / trip / M720 input; user B `GET` / `PUT` / `DELETE` on A's id returns 404 (not 403); every new surface covered.
- **Session revoke: cannot revoke current.** `DELETE /auth/sessions/{current_id}` returns 403 with the expected `code`; session row is unchanged.
- **Session revoke: can revoke other.** As user B (logged in from a second browser), `DELETE /auth/sessions/{A_other}` as user A; assert 204 + `sessions.revoked_at` set + audit row with `revoke_kind = 'single'`.
- **Revoke-all-others preserves current.** Seed user A with 3 active sessions (1 current + 2 others); POST `/auth/sessions/revoke-all-others`; assert response `{ revokedCount: 2 }`, the current row survives, both others have `revoked_at = now()`, one audit row with `payload = { revoke_kind: "all_others", initiator: "self", count: 2 }`.
- **Audit payload allowlist.** For each new audit action, assert the serialized `payload_summary` matches the allowlisted shape exactly (no FMVs, no share counts, no destination country, no dates, no checklist booleans, no totals, no raw IPs). Enforced in CI via a schema fixture `backend/crates/orbit-core/tests/fixtures/audit_payload_shapes.json` keyed by action.

**Frontend unit + E2E tests.**
- Vitest on `stackCumulativeForEmployer` with the shared fixture.
- Vitest on the eligibility-criteria serializer (round-trips the five-keys object through JSON with no loss).
- Playwright on the Slice-2 demo script (`docs/requirements/slice-2-acceptance-criteria.md` §13, 19 steps).
- `axe-core` on each surface in G-21 extended (ESPP form, purchases list, trip list, trip form with checklist open, M720 panel, sessions UI, multi-grant dashboard ≥ 2 tiles + stacked view).

### 9. Assumptions and escalations

Two items warrant explicit recording.

#### 9.1 Art. 7.p eligibility JSONB vs typed columns

Options considered:

- **Five `BOOLEAN NULL` columns** — type-safe; every criterion is a first-class, indexable field; most natural for AC-5.2.3 validation.
- **JSONB with `CHECK (jsonb_typeof = 'object')` + handler-layer shape validation** — flexible; the US-005 analyst may add a sixth criterion in Slice 4 with zero schema churn; JSONB is the shape Slice-4's tax engine will want to read anyway (it already consumes `scenarios.inputs` and `calculations.result` as JSONB per ADR-005).
- **JSONB with `jsonb_matches_schema` via the `postgres-json-schema` extension** — strictly typed at the DB, but adds a Postgres extension for a single shape.

**Chosen: JSONB + column CHECK + handler-layer schema validation.** Reasoning:

1. The five-criterion list is still-evolving per the analyst's notation in AC-5.2.2 ("help link explains how this is determined but the tool does not auto-resolve"). A sixth criterion ("equivalent-double-taxation-treaty countries") has been floated informally. Five BOOLEAN columns would force a migration for that addition; JSONB absorbs it.
2. SEC-163 already requires handler-layer validation for every user-entered shape; adding the five-keys + three-value-types check to that validator is a few lines of code and there is no backend branch that reads the column without going through the validator anyway.
3. Zero new extensions; the `postgres-json-schema` option introduces a distribution-level dependency that is hard to land on a stock Postgres and is overkill for a five-key object.

**Cost of the opposite decision** (five typed columns): one schema migration + one handler rewrite when Slice 4 adds its sixth criterion. Roughly 2 engineering hours, deterministic.

**Migration cost if Slice 4 decides to promote the JSONB to typed columns.** Straightforward: one additive migration with a backfill (`UPDATE art_7p_trips SET services_outside_spain = (eligibility_criteria ->> 'services_outside_spain')::bool` per criterion). No data loss; no Slice-2-era row is lost. Documented in §11.

#### 9.2 Country derivation for sessions (AC-7.1.3)

Options considered:

- **GeoIP at request time.** Every list-sessions call hits the GeoIP DB. Problem: we HMAC the IP at session-creation-time per SEC-054; the raw IP is not stored anywhere, so the list handler has nothing to look up. Would require persisting the raw IP (rejected — boundary-violates SEC-054) or re-deriving the hash (cannot; HMAC is one-way).
- **Lookup-table-in-DB** (IP → country, stored alongside the session row). Storing the raw IP is the SEC-054 boundary violation; storing just the country is what the chosen option does.
- **Country derived at session-creation-time and stored on the session row** (the new `sessions.country_iso2` column). Raw IP lives in RAM only during the auth handler (already the case); a `maxmind::geoip2::Country` lookup happens in the same handler; the two-letter code lands on the `sessions` row. Every subsequent list handler reads the code directly.
- **Sidecar table `session_country_lookups (session_id → country_iso2)`.** Same privacy posture as the chosen option; adds a JOIN on every list query for no benefit.

**Chosen: store `country_iso2` on the session row at creation time.**

Reasoning: it is the narrowest privacy footprint compatible with the acceptance criterion. Two-letter country code has markedly lower entropy than raw IP (≈ 250 possible values vs 2³² + 2¹²⁸); the UI's "Madrid, ES (aprox.)" rendering is a client-side concatenation of a locale-appropriate capital-city label + the country code (the server never serializes a city), so the re-identification risk is bounded at "which country the user signed in from."

Privacy implications, documented:

- The column is `NULL`-able; if the GeoIP lookup fails at session creation (offline dev, or a private network), the UI renders `ubicación desconocida` / `location unknown` and does not block signin.
- The column is **never** returned by any endpoint other than `GET /auth/sessions` for the caller's own rows.
- The column is excluded from the `audit_log` payload (G-32). `session.revoke` payloads carry `revoke_kind` + `initiator` only.
- On account deletion (Slice 7), the column is included in the cascade — no orphan.

**Cost of the opposite decision** (GeoIP at list-time with raw-IP storage): SEC-054 is violated; the new-device-notice email (SEC-010, shipped in Slice 1) would already need the raw IP to do a city lookup and we decided there to drop the city in favor of "new sign-in from Madrid (approx.)" — a country-level signal matching this decision. Keeping the two surfaces consistent is the right shape.

### 10. Alternatives considered

- **Separate `espp_grants` table vs. a column on `grants`.** Rejected. `grants` already has `instrument = 'espp'`; adding a parallel table would duplicate every ESPP grant's identity (employer, grant_date, share_count at offering), forcing a JOIN on every dashboard read. The ESPP-specific fields that made "separate table" attractive (lookback FMV, employer discount) live on the purchase, not on the grant — which is exactly what `espp_purchases` captures.
- **Art. 7.p trips as a JSONB array on `users`.** Rejected. Slice 4 needs range queries on `from_date` / `to_date` for the €60 100 cap application and the 0-day omission; a JSONB array forces either in-app iteration of unbounded arrays or tortured `jsonb_path_query` SQL. A first-class table with indexed date columns is boring and fast.
- **Modelo 720 inputs as a single row per user (snapshot).** Rejected per Q4 2026-04-20 product-owner decision. Time-series with close-and-create is the Slice-1 `residency_periods` pattern and is needed for year-on-year Modelo 720 history (even though Slice 2 does not surface history in the UI; Slice 3's threshold alert wants it, and Slice 6's worksheet export wants it).
- **Session revoke as cookie-purge without DB update.** Rejected. Server-side `sessions.revoked_at` is the authority (per ADR-011 §Signout); cookies are hints. A client-side purge without the DB write would allow a stolen cookie to stay live.
- **Typed columns for the Art. 7.p checklist (five BOOLEAN NULL).** See §9.1; rejected on evolution cost.
- **GeoIP at list-time.** See §9.2; rejected on SEC-054 boundary.
- **Storing the session's originating city (not just country).** Rejected. Higher entropy, lower value — the demo script (step 12) asserts the UI renders `Madrid, ES (aprox.)` which is a client-side locale-aware label derived from the country; there is no product surface that wants a precise city.
- **A new `sessions.geoip_lookup_failed_at` column to distinguish "lookup failed" from "never looked up".** Rejected. `country_iso2 IS NULL` carries both meanings; the UI treats them identically.
- **A cron job that squashes same-day Modelo 720 rows.** Rejected for Slice 2. AC-6.2.3 explicitly accepts the 1-day open-then-closed row as a natural consequence of close-and-create; the in-handler same-day-update logic in §3 avoids it on the happy path. A squashing job is a nice-to-have optimization, not a gate.
- **Dropping the `notes` column on `grants` in the Slice-2 migration.** Rejected. The Slice-1 JSON lives there for existing users; a destructive migration would force a backfill plan that must enumerate every row and is much more work than a lazy lift on first purchase.

## Consequences

**Positive:**

- Every Slice-2 AC traces to a concrete schema column, trigger, handler path, audit payload shape, or deferral note. No TBD.
- RLS enforced from the first commit on all three new tables via the inherited `tenant_isolation` policy; the SEC-020 CI introspection test extends with zero code changes beyond the expected-tables fixture.
- The Slice-1 `grants.notes` compromise has a documented, testable retirement path that is reversible and non-lossy.
- The stacked-cumulative algorithm is pinned in pseudocode and tested by a property suite + a shared parity fixture, removing the "two reimplementations drift apart" risk.
- `sessions.country_iso2` closes the AC-7.1.3 UI need with the narrowest privacy footprint compatible with SEC-054.
- The M720 close-and-create shape reuses the Slice-1 `residency_periods` pattern verbatim — one less novel thing for the implementation engineer to reason about.

**Negative / risks:**

- JSONB `eligibility_criteria` means the Slice-2 handler is the authoritative validator for the five-keys shape. A handler bug can silently persist an invalid object; mitigation is a CI test that round-trips every fixture trip through the serializer and a DB read-back.
- The `espp_purchases_enforce_grant_instrument_trg` trigger is a new surface for PlanetScale-style "triggers considered harmful" criticism. Mitigation: the trigger is trivial, `STABLE`, runs one `SELECT` per insert, and the cost is dominated by the FK check. Documented with its own CI test that confirms it fires for non-ESPP parents.
- `sessions.country_iso2` is `NULL`-able; the Slice-2 backfill for existing Slice-1 sessions leaves them `NULL` (the raw IP is not available). Mitigation: the UI renders `ubicación desconocida`; on next refresh-rotation the new session row gets a populated column, so the `NULL` window closes as sessions turn over. This is an accepted Slice-2 risk; no Slice-1 session row is re-derived post-hoc.
- Client and server each implement the stacked-cumulative algorithm; drift risk is real (same as Slice 1's vesting algorithm). Mitigation is the shared fixture + the property test, identical discipline.
- The annual-cap tracker in `GET /trips` is a sum of day counts that crosses year boundaries (a trip from 2025-12-28 to 2026-01-05 spans both years); the handler splits the trip's day count per year using inclusive-endpoints arithmetic. The implementation engineer must take care not to double-count the boundary day; the integration test exercises that exact shape.

**Tension with prior ADRs:**

- None. ADR-005 outlined `espp_purchases` and `art_7p_trips` with a narrower column set; this ADR is the authoritative expansion for the columns Slice 2 touches. ADR-011's sessions shape is extended with one additive column, not changed. ADR-014's `tenant_isolation` convention and `touch_updated_at` function are reused verbatim.

**Follow-ups (not blocking Slice 2):**

- **Slice 3.** Consume `modelo_720_user_inputs` + FX rates to compute the threshold banner (AC-6.3.1). Compute the securities row (AC-6.1.5) from grants + FX and present it read-only in the M720 panel with a Pattern-C range.
- **Slice 4.** If the analyst confirms a sixth Art. 7.p eligibility criterion, add the new key to the handler allowlist + extend the frontend form + bump the `eligibility_criteria` JSONB shape validator. Optionally: promote the JSONB to five (or six) typed columns with a one-shot backfill (see §9.1).
- **Slice 4.** Open the Modelo 720 securities line (requires Slice-3 FX).
- **Slice 6.** M720 worksheet PDF export consumes the full time-series; the Slice-2 history endpoint (`GET /modelo-720-inputs`) is the data source.
- **Slice 8.** Bulk import for ESPP purchases from Shareworks CSV; reuse the `POST /grants/:grant_id/espp-purchases` handler as a batch endpoint (N INSERTs in one transaction; the Slice-1 notes lift becomes a pre-import step, not a per-row one).
- **Implementation engineer (Slice 2).** Author the `stacked_grants_cases.json` fixture file and wire both backend and frontend to it. Co-locate with `vesting_cases.json`.
- **Implementation engineer (Slice 2).** Write the audit-payload-shape CI fixture + lint that rejects any new `audit_log.payload_summary` not in the allowlisted set.
- **Implementation engineer (Slice 2).** Extend the `Tx::for_user` cross-tenant probe suite (SEC-023) to cover every new `[A]` endpoint listed in §3 before Slice-2 sign-off.
- **Security-engineer (Slice 2).** Confirm that storing `country_iso2` on `sessions` is acceptable under SEC-054 (city is not stored; country is the coarsest meaningful signal) and that the `GET /auth/sessions` response shape excludes every field that could re-identify a user across sessions.
