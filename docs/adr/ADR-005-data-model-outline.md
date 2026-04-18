# ADR-005: Data model outline

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

This ADR commits to the **entity outline + multi-tenancy enforcement model**. Detailed DDL (column types, indexes, constraint expressions, exhaustive enum variants) is deferred to the solution-architect second pass after security review. The outline must be sufficient for the implementation engineer to begin scaffolding migrations and for security-engineer to assess the GDPR posture.

Multi-tenancy is shared-schema with **Postgres Row-Level Security (RLS)** enforced at the database. This was an Ivan-locked input; the rationale is: smallest blast radius for tenant data leak (a forgotten `WHERE user_id = ?` in application code is caught at the DB), simplest GDPR erasure (single `DELETE FROM users WHERE id = ?` cascades), no schema migration cost per tenant.

## Decision

### Entity outline

Tables grouped by purpose. PII columns flagged `[PII]`; tenant-scoped tables flagged `[RLS]`.

**Identity & access**
- `users` `[RLS-root]` — id, email `[PII]`, password_hash, mfa_totp_secret (nullable, encrypted), locale, primary_currency, created_at, deleted_at (soft-delete grace per §7.2).
- `sessions` `[RLS]` — id, user_id, refresh_token_hash, ip_hash, user_agent, created_at, last_used_at, revoked_at.
- `subscriptions` `[RLS]` — id, user_id, billing_provider_customer_id, plan, status, current_period_end, lapsed_at.

**Equity portfolio**
- `grants` `[RLS]` `[PII-adjacent]` — id, user_id, instrument (`rsu | nso | espp | iso_mapped_to_nso`), grant_date, share_count, strike (nullable; required for option-shaped), vesting_start, vesting_total_months, cliff_months, vesting_cadence (`monthly | quarterly`), double_trigger (bool), liquidity_event_date (nullable), employer_name, ticker (nullable until IPO), notes.
- `vesting_events` `[RLS]` — derived/cached projection of grant vesting per share, used for fast portfolio rendering. Recomputed on grant edit.
- `espp_purchases` `[RLS]` — id, grant_id, purchase_date, shares, fmv_at_purchase (Money), purchase_price (Money), lookback_fmv (nullable Money). Backs US-008 + US-013 ESPP basis lookups.
- `nso_exercises` `[RLS]` — id, grant_id, exercise_date, shares, fmv_at_exercise (Money), strike_paid (Money). Stub in v1 (no realized-sale ledger), but the table exists so US-013 NSO same-day calculations can persist their inputs for audit/reproducibility without forcing the realized-sale ledger.

**Residency & profile**
- `residency_periods` `[RLS]` — id, user_id, jurisdiction (`ES | UK | ...`), sub_jurisdiction (autonomía code, nullable), from_date, to_date (nullable = current), regime_flags (array; e.g., `beckham_law`, `foral_pais_vasco`). Time-bounded as required by §7.3. v1 engine reads only the *current* row; the schema does not block future multi-jurisdiction.
- `art_7p_trips` `[RLS]` — id, user_id, destination_country, from_date, to_date, purpose, beneficiary_entity, beneficiary_is_foreign (bool). Backs US-005.

**Calculation & traceability** (couples with ADR-004)
- `rule_sets` — id (e.g., `es-2026.1.0`), jurisdiction, aeat_guidance_date, effective_from, effective_to, content_hash, status (`proposed | active | superseded | withdrawn`), supersedes_id (nullable FK), data (JSONB; canonicalized payload), published_at, published_by. **Not RLS-scoped** — global reference data. **Update-protected** by trigger when status='active'.
- `scenarios` `[RLS]` — id, user_id, name, inputs (JSONB; IPO date, IPO price, lockup, sell %, FX assumption, etc.), created_at. The persisted "what-if" the user can revisit.
- `calculations` `[RLS]` — id, user_id, scenario_id (nullable), kind (`scenario | sell_now | annual_irpf | modelo_720_check`), rule_set_id (FK), rule_set_content_hash, engine_version, inputs_hash, result (JSONB; line items + totals + formula trace + sensitivity inputs), result_hash, computed_at. Per ADR-004.
- `sell_now_calculations` `[RLS]` — id, user_id, session inputs (lots, overrides), market_quote_id (FK), fx_rate_id (FK), calculation_id (FK to `calculations`), computed_at. Persisted per US-013 acceptance for audit/reproducibility despite "stateless-ish" framing — *the inputs are persisted; nothing is treated as a realized sale*.
- `exports` `[RLS]` — id, user_id, calculation_id (FK), kind (`pdf | csv`), object_storage_key, traceability_id (UUID surfaced in PDF/CSV header per §7.9 + ADR-008), created_at, retained_until.

**Reference & cache**
- `fx_rates` — date, base_currency, quote_currency, rate (Decimal), source (`ecb_daily_reference | user_override | last_known_fallback`), fetched_at, ecb_publication_date (nullable). Append-only. Per ADR-007.
- `market_quotes_cache` — id, ticker, quote_price (Decimal USD), quote_timestamp, vendor (enum), intraday_high, intraday_low, fetched_at, ttl_until. 15-minute TTL per §7.6 + ADR-006. **Not user-scoped** — shared cache across users to minimize vendor calls.

**Audit & GDPR**
- `audit_log` — id, user_id (nullable; system actions exist), actor_kind (`user | system | worker`), action (e.g., `grant.create | scenario.run | export.generate | rule_set.publish | dsr.export | dsr.delete`), target_kind, target_id, ip_hash (nullable), occurred_at, traceability_id (nullable; for export reverse-lookup), payload_summary (JSONB; **never includes grant values, share counts, or tax outputs** per §7.2 data-minimization). Retained 6 years separately from PII (§7.9).
- `dsr_requests` `[RLS]` — id, user_id, kind (`access | rectification | erasure | restriction | portability`), submitted_at, sla_due_at, completed_at, archive_object_storage_key (nullable). Backs US-011.

### RLS enforcement model

For every `[RLS]` table:

```sql
ALTER TABLE <table> ENABLE ROW LEVEL SECURITY;

CREATE POLICY tenant_isolation ON <table>
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);
```

The application sets `app.user_id` per request via `SET LOCAL app.user_id = '<uuid>'` immediately after acquiring a connection from the pool, inside the request transaction. The policy is `USING + WITH CHECK` so both reads and writes are scoped — a forgotten `WHERE` clause cannot leak data, and a forgotten `SET LOCAL` causes the query to return zero rows (fail-closed) rather than all rows.

**Application role** is a non-superuser DB role (`orbit_app`) that does not have `BYPASSRLS`. The migration role is separate.

**Worker process** sets `app.user_id` per work item. Worker tasks that operate across users (e.g., scheduled ECB FX fetch, rule-set ingestion) operate against tables that are **not RLS-scoped** (`fx_rates`, `rule_sets`, `market_quotes_cache`); cross-user worker tasks against `[RLS]` tables (e.g., DSR exports) explicitly set `app.user_id` per user.

### GDPR erasure

A user-initiated erasure (US-011, "Delete my account"):

1. Soft-delete: set `users.deleted_at = now()`. 30-day grace period (§7.2) during which sign-in is blocked but data is recoverable.
2. Hard-delete worker job: `DELETE FROM users WHERE id = ? AND deleted_at < now() - interval '30 days'`. **All `[RLS]` tables CASCADE** via FK constraints. Single statement, no application-side enumeration.
3. **Pseudonymization in `audit_log` and `calculations`** (legal-retention exemption per §7.9): user_id is replaced with `00000000-0000-0000-0000-000000000000` (the tombstone) on the same transaction. The audit-log entries themselves are retained for the AEAT prescription window; they no longer link to a person.

This single-statement-cascade is the load-bearing reason for shared-schema RLS over schema-per-tenant.

### Open data-model questions deferred to second-pass solution-architect

- Exact `Money` storage (single Decimal+Currency columns vs JSONB).
- Exact `JSONB` shape for `calculations.result` and `scenarios.inputs` — needs to align with the `FormulaTrace` and `TaxResult` types from ADR-003.
- Whether `vesting_events` is materialized in a real table or a view.
- Indexing strategy (especially on `audit_log` — high-write, occasionally-queried).

## Alternatives considered

- **Schema-per-tenant.** Rejected. With <€200/mo budget and one Postgres VM, schema-per-tenant scales operationally poorly: thousands of schemas inflate `pg_class`, complicate migrations (run per schema), and `pg_dump` becomes painful. Tenant isolation gain over RLS is marginal.
- **Database-per-tenant.** Rejected outright at this cost tier; one Postgres instance per user is unaffordable.
- **Application-only enforcement (no RLS).** Rejected. The cost of one missed `WHERE user_id = ?` in a regulated EU SaaS is a notifiable breach. RLS is cheap defence-in-depth; the only operational cost is remembering `SET LOCAL`, and that has a fail-closed default.
- **Soft-delete only, no hard-delete.** Rejected. GDPR Art. 17 erasure is a right; soft-delete is not erasure. The 30-day grace period is the compromise.

## Consequences

**Positive:**
- GDPR erasure is a single cascade; no per-table deletion code to keep in sync.
- A bug in an application query cannot leak across tenants.
- Calculation reproducibility (ADR-004) has a clean home: every calc row carries its rule-set + inputs + result hashes.
- Reference data (`rule_sets`, `fx_rates`, `market_quotes_cache`) lives outside the RLS perimeter, so worker tasks against them are simple.

**Negative / risks:**
- RLS adds a small per-query overhead (Postgres re-evaluates the policy expression). Negligible at v1 scale; revisit if `EXPLAIN ANALYZE` shows policy cost on hot paths.
- `SET LOCAL app.user_id` discipline must be enforced via a single connection-acquisition helper. Mitigation: a `Tx::for_user(user_id)` wrapper in the data-access layer that is the only way to acquire a query handle.
- Pseudonymization of `audit_log` for retention may be challenged by AEPD as insufficient anonymization if cross-referencing with retained calculation data could re-identify. Security-engineer must confirm; fallback is a stricter scrub.
- JSONB-heavy storage of `calculations.result` makes ad-hoc analytical queries painful. Acceptable; v1 has no analytical query needs.

**Follow-ups:**
- Solution-architect (second pass): write the actual DDL with constraints, indexes, and trigger definitions (rule-set immutability, RLS policy bodies).
- Implementation engineer: build the `Tx::for_user(user_id)` helper and a CI lint that fails any direct `pool.acquire()` outside that helper.
- Security-engineer: confirm the audit-log pseudonymization posture and `ip_hash` salt-management strategy.
- Define the FK CASCADE topology explicitly; document which tables cascade from `users` and which do not (`rule_sets`, `fx_rates`, `market_quotes_cache` do not).
