# ADR-012: Rule-set pipeline and tax-engine contract

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)
- **Traces to:** ADR-003 (hybrid engine), ADR-004 (rule-set versioning + calc stamping), ADR-005 (`rule_sets` + `calculations` tables), ADR-013 (repo layout — this ADR specifies the crate list ADR-013 scaffolds), spec §7.1 / §7.3 / §7.4, SEC-080..SEC-087, SEC-094.

## Context

ADR-003 fixes the engine shape (shared `orbit-tax-core` + per-country `TaxCalculator` impls + externalized rule data). ADR-004 fixes rule-set lifecycle (immutable publish, content hash, replay). ADR-005 fixes the `rule_sets` and `calculations` tables. What is still implicit is **the contract between the engine and the rest of the app**:

- What is the engine crate's public API surface? What does a caller import?
- What types flow at the boundary (inputs, context, result, errors)?
- How does the loader enforce `status='active'` (SEC-087)?
- How is the canonicalization-and-hash function shared between the CI hasher and the runtime loader (SEC-081)?
- How does the two-step publish (PR-merge to `proposed` → operator CLI promote to `active`, SEC-083) materialize?
- Where exactly does `rule_set_id`, `content_hash`, `engine_version`, `inputs_hash`, `result_hash` get stamped onto a `calculations` row (SEC-086)?

**None of this is used in Slice 1.** Slice 1 has no tax math. But Slice 0 must stand up the crate skeleton because (a) the `Tx::for_user` helper and the `orbit_log::event!` macro the whole app depends on live in the same workspace, (b) the `rule_sets` table and its immutability trigger ship in Slice 0 (S0-18), and (c) the rule-set loader abstraction is the largest v1 architectural risk — designing it at Slice 4 under calendar pressure is the wrong time.

This ADR is a **paper design frozen in Slice 0**, implemented in Slice 4. Every choice here should cost nothing to scaffold now and pay off when the calculator crate lands.

## Decision

### Crate topology

A Cargo workspace. Crates with ★ are **scaffolded in Slice 0** (empty impls / stubs, public API declared); ☆ are **implemented in Slice 4+**.

```
orbit-workspace/
├── crates/
│   ├── orbit-core          ★  Shared primitives used everywhere: Money, Decimal wrappers, ID newtypes.
│   ├── orbit-crypto        ★  Argon2id, chacha20poly1305, HMAC helpers (used by auth + column-encrypt).
│   ├── orbit-db            ★  sqlx helpers, Tx::for_user, migration runner, connection pool.
│   ├── orbit-log           ★  orbit_log::event! macro + field allowlist (SEC-050).
│   ├── orbit-auth          ★  Argon2 wrappers, sessions, email verification, password reset (ADR-011).
│   ├── orbit-api           ★  axum handlers, extractors, error envelope (ADR-010).
│   ├── orbit-worker        ★  Scheduled jobs (ECB fetch from Slice 3+, retention sweeps, replay sampler from Slice 4+).
│   ├── orbit-tax-core      ☆  TaxableEvent, TaxCalculator trait, RuleSet, RuleData enum, FormulaTrace, TaxResult.
│   ├── orbit-tax-rules     ☆  YAML loader + canonicalizer + hasher (used by CI *and* runtime per SEC-081).
│   ├── orbit-tax-spain     ☆  SpainTaxCalculator implementing orbit_tax_core::TaxCalculator.
│   ├── orbit-market-data   ☆  MarketDataProvider trait, FinnhubProvider, TwelveDataProvider (Slice 5).
│   ├── orbit-fx            ☆  ECB ingestion + lookup_rate (Slice 3).
│   └── orbit-export        ☆  PDF/CSV rendering + traceability (Slice 6).
├── binaries/
│   └── orbit               ★  Single binary, subcommand dispatch (api / worker / migrate / rules).
├── migrations/             ★  sqlx migrations (ADR-013 picks sqlx).
├── rules/                  ★  jurisdictional YAML rule sets (directories exist; no content in Slice 0).
│   └── es/
└── frontend/
```

Rationale for the split:

- **`orbit-tax-core` vs `orbit-tax-spain`.** ADR-003 is explicit: shared primitives and the trait are country-agnostic; each country is its own crate. A future `orbit-tax-uk` depends on `orbit-tax-core`, not on `orbit-tax-spain`. The UK paper-design acceptance gate stays structural.
- **`orbit-tax-rules` separate from `orbit-tax-core`.** The YAML loader / canonicalizer / hasher is used in **two** places: at CI (validate a YAML file before the PR merges) and at runtime (validate a `rule_sets` row's content hash matches the canonicalized YAML). SEC-081 explicitly requires this is the same function. Packaging it in its own crate makes the "called from CI" use case not drag in the full engine.
- **`orbit-tax-spain` does not depend on `orbit-market-data` or `orbit-fx` directly.** The calculator consumes a `CalcContext` that holds references to an `FxProvider` and a `MarketDataProvider` trait object; the impls are wired at the binary-crate level. Keeps the calculator testable with stubs.

### Public surface of `orbit-tax-core` (the contract)

```rust
// Stable types every calculator consumes. Nothing country-specific here.

pub struct RuleSetId(pub String); // e.g., "es-2026.1.0"
pub struct ContentHash([u8; 32]); // SHA-256
pub struct EngineVersion(pub &'static str); // compile-time: env!("CARGO_PKG_VERSION")

pub struct Money { pub amount: Decimal, pub currency: Currency }

pub enum JurisdictionCode { ES, UK /* extensible */ }

pub struct CalcContext<'a> {
    pub user_id: Uuid,
    pub as_of: DateTime<Utc>,            // injected; engine never calls now()
    pub residency: ResidencyPeriod,
    pub fx: &'a dyn FxProvider,          // from orbit-fx; stub in tests
    pub market: &'a dyn MarketDataProvider, // from orbit-market-data; optional (None in scenario compute)
}

pub enum EventKind { Vest, Exercise, Purchase, Sale, LiquidityTrigger }

pub enum Instrument { Rsu, Nso, Espp, IsoMappedToNso /* UK additions later */ }

pub struct TaxableEvent {
    pub id: Uuid,
    pub kind: EventKind,
    pub occurred_on: NaiveDate,
    pub instrument: Instrument,
    pub quantity: Decimal,
    pub fmv_at_event: Money,
    pub strike_paid: Option<Money>,
    pub jurisdiction_at_event: JurisdictionCode,
    pub source_grant_id: Uuid,
    pub grant_metadata: GrantMetadata, // free-form per-instrument bag (plan type for UK EMI/CSOP, lookback flag for ESPP)
}

pub struct RuleSet {
    pub id: RuleSetId,
    pub jurisdiction: JurisdictionCode,
    pub aeat_guidance_date: NaiveDate,
    pub effective_from: NaiveDate,
    pub effective_to: Option<NaiveDate>,
    pub content_hash: ContentHash,
    pub status: RuleSetStatus,         // Proposed | Active | Superseded | Withdrawn
    pub data: RuleData,                // enum; one variant per jurisdiction
}

pub enum RuleData {
    Spain(rule_data::spain::SpainRuleData),
    // Uk(...) added when orbit-tax-uk is authored.
}

pub trait TaxCalculator: Send + Sync {
    fn jurisdiction(&self) -> JurisdictionCode;
    fn compute(&self, ctx: &CalcContext, events: &[TaxableEvent], rules: &RuleSet) -> Result<TaxResult, TaxError>;
}

pub struct TaxResult {
    pub rule_set_id: RuleSetId,
    pub rule_set_content_hash: ContentHash,
    pub engine_version: EngineVersion,
    pub line_items: Vec<TaxLineItem>,
    pub totals: TaxTotals,
    pub formula_trace: FormulaTrace,
    pub sensitivity_inputs: Vec<SensitivityInput>,
}

pub enum TaxError {
    UnsupportedRegime(RegimeCode),       // Beckham, foral
    MissingInput(InputKey),              // ESPP FMV-at-purchase missing
    RuleSetNotActive(RuleSetId),         // per SEC-087 path
    RuleDataShapeMismatch(RuleSetId),    // RuleData enum variant not appropriate for jurisdiction
    EngineInternal(String),              // last resort; no PII
}
```

The contract is deliberately narrow: **inputs in, typed result out, errors are enum-valued**. Nothing here says anything about Spain. `SpainTaxCalculator::compute` is the only code allowed to know about IRPF, autonomía, or Art. 7.p.

### Input hash, result hash, canonicalization

Per SEC-086, every `calculations` row carries `inputs_hash` and `result_hash`. Canonicalization rules (locked here, implemented in `orbit-tax-core::canonicalize`):

- JSON with sorted keys, UTF-8, LF, no insignificant whitespace.
- `Decimal` serialized as decimal-string with no trailing zeros beyond 4 fractional digits unless natively provided by the rule data (ECB FX rates are 4 fractional digits; AEAT rates go up to 6).
- Dates as `YYYY-MM-DD`; timestamps as RFC 3339 UTC with millisecond precision.
- `BTreeMap` everywhere (never `HashMap`) — enforced by a lint per SEC-085.

The same canonicalizer is used for YAML rule-set hashing (after YAML → JSON lossless conversion) so the CI hasher and the runtime loader are guaranteed to agree on `content_hash` (SEC-081).

### Rule-set loader: how `status='active'` is enforced (SEC-087)

`orbit-tax-rules` exposes exactly two loader methods:

```rust
pub struct RuleSetLoader<'db> { db: &'db dyn Db }

impl<'db> RuleSetLoader<'db> {
    /// Production path. Returns only rule sets with status='active' for the
    /// given jurisdiction, matching effective_from/to for the as-of date.
    /// Verifies runtime-computed canonical hash == stored content_hash;
    /// returns RuleSetNotActive if mismatch.
    pub fn active_for(&self, j: JurisdictionCode, as_of: NaiveDate) -> Result<RuleSet, RuleSetError>;

    /// Test/CI path. Behind a Cargo feature flag `internal-test-loader` that is
    /// compiled out of the production binary.
    #[cfg(feature = "internal-test-loader")]
    pub fn any_status(&self, id: &RuleSetId) -> Result<RuleSet, RuleSetError>;
}
```

- `active_for` is the only method the `orbit-api` handler layer can call. It filters `status='active'` at the SQL level.
- `any_status` exists only for the rule-set regression fixture suite (SEC-084); its visibility is compile-gated. Shipping the production binary with this feature enabled fails CI (a `release-build-no-test-features` check greps the built binary for a sentinel string set when the feature is on).
- Runtime hash verification: the loader reads `content_hash` from the row, re-canonicalizes `data` via `orbit-tax-core::canonicalize`, SHA-256s, and compares in constant time. Mismatch → `RuleSetError::HashMismatch`, engine refuses compute. Per SEC-081.

### Two-step publish pipeline

**Step 1 — merge to `proposed`** (fully automatic, triggered by merge to `main`):

1. PR touches `/rules/es/es-2026.1.0.yaml`. CODEOWNERS requires CODEOWNER approval (SEC-080).
2. CI (on the PR) runs:
   - `cargo xtask rules:canonicalize /rules/es/es-2026.1.0.yaml` — produces the canonical JSON form, writes to `/rules/es/.generated/es-2026.1.0.canonical.json`, diff-reviewable.
   - `cargo xtask rules:hash /rules/es/es-2026.1.0.yaml` — computes SHA-256, writes to `/rules/es/.generated/es-2026.1.0.sha256`.
   - `cargo test -p orbit-tax-spain` — runs the snapshot fixture regression suite (SEC-084). Snapshot fixture failures require a deliberate ack in the PR body.
3. On merge, the deploy pipeline runs `orbit rules:ingest` as part of the migrate step. This:
   - Parses the YAML, re-canonicalizes, re-hashes.
   - Inserts into `rule_sets` with `status='proposed'`.
   - Refuses to insert if a row with the same `id` already exists with different `content_hash` (immutability, SEC-082).
   - Writes an `audit_log` entry `rule_set.ingested`.

**Step 2 — operator CLI promotion** (manual, MFA-gated operator console in v1.1; in Slice 4 this is an SSH-to-the-worker + CLI command):

```
orbit rules:promote es-2026.1.0 --confirm
```

The command:
1. Re-runs the regression suite against the proposed rule set in-process. Refuses promotion on any failure.
2. Re-computes the content hash and verifies it matches the stored row (defence against a tampered DB row between ingest and promote).
3. Wraps in a transaction:
   - `UPDATE rule_sets SET status='active', ...`
   - Marks any prior `active` rule set for the same jurisdiction+effective-from-overlap as `status='superseded'` and writes `supersedes_id` on the new row.
   - `INSERT audit_log(action='rule_set.promoted', target_id=<id>)`.
4. Emits an operational log line with the numeric-diff summary vs the superseded rule set (S50 defence: an anomalous rate change surfaces for human review even if the PR review missed it).

The prod binary refuses `orbit rules:promote` unless invoked by a user authenticated via an operator flow. In Slice 4 the operator flow is: SSH to VM-1, `sudo -u orbit orbit rules:promote ...`. In v1.1+ a browser-based operator console with WebAuthn replaces this.

### Calculation stamping (SEC-086)

Every calculation path ends in a single helper:

```rust
pub async fn run_calculation<C: TaxCalculator>(
    ctx: &CalcContext<'_>,
    calculator: &C,
    events: &[TaxableEvent],
    kind: CalculationKind,               // Scenario | SellNow | AnnualIrpf | Modelo720Check
    scenario_id: Option<Uuid>,
    db: &dyn Db,
) -> Result<CalculationId, CalcError> {
    let rule_set = RuleSetLoader::new(db).active_for(ctx.residency.jurisdiction, ctx.as_of.date_naive())?;
    let inputs_hash = canonicalize_and_hash(&events, &ctx.minimized())?;
    let result = calculator.compute(ctx, events, &rule_set)?;
    let result_hash = canonicalize_and_hash(&result)?;

    let id = db.insert_calculation(CalculationRow {
        user_id: ctx.user_id,
        scenario_id,
        kind,
        rule_set_id: rule_set.id.clone(),
        rule_set_content_hash: rule_set.content_hash,
        engine_version: EngineVersion(env!("CARGO_PKG_VERSION")),
        inputs_hash,
        result: serde_json::to_value(&result)?,
        result_hash,
        computed_at: ctx.as_of,
    }).await?;
    Ok(id)
}
```

This is **the only public path that writes to `calculations`**. Every calculation kind from Scenario modeler (Slice 4) to Sell-now (Slice 5) goes through this helper. It is impossible to insert a calculation without the five stamping fields, because the helper is the only code with `INSERT INTO calculations` access (the `orbit_app` DB role's `INSERT` grant on `calculations` is narrowed via a view or stored procedure if needed; simpler: the helper is the only call site in code and CI lint forbids raw `INSERT` into `calculations` anywhere else).

### Replay sampler (SEC-094, ADR-004)

`orbit-worker` runs a weekly job:

1. `SELECT` N=100 random rows from `calculations` older than 1 week.
2. For each: load stored inputs, `active_for(jurisdiction, computed_at.date())` to get the **historical** rule set it was stamped against (pinned by `rule_set_id`, so we re-fetch by id not by date), re-run the calculator.
3. Canonicalize the new result, SHA-256, compare to stored `result_hash`.
4. Mismatch → alert + pin the offending row in a `replay_alerts` table (schema: `calculation_id`, `observed_hash`, `stored_hash`, `investigated_at NULL`).

The engine version at replay time may differ from the engine version that produced the original calc; the sampler records both and the alert carries a "this may be an intentional engine change" marker. Genuine regressions (not explained by an engine delta) are treated as incident-worthy.

### Two-currency discipline (early-warning for multi-jurisdiction)

`orbit-core::Money` rejects arithmetic across currencies at compile time (operator overloads are only implemented for same-currency Money). Cross-currency conversions go through `FxProvider::convert(&Money, target: Currency) -> Result<Money, FxError>`, which records the rate + date + spread on the resulting `Money`'s provenance chain. This is paper-design work paid forward to Slice 3 (FX pipeline) and Slice 5 (sell-now compute).

### Slice-0 concrete scaffolding (what lands before there are any calculators)

- All ★ crates exist with empty but well-typed stubs.
- `orbit-tax-core` declares the types above; types compile; one integration test instantiates a no-op `StubCalculator` and round-trips a `TaxResult` through `canonicalize_and_hash`.
- `rule_sets` table migrated with the immutability trigger (S0-18).
- `calculations` table migrated with the 5 stamping columns non-null-constrained.
- No rule-set YAML files, no calculator impls. First content lands in Slice 4.

## Alternatives considered

- **Put canonicalization in `orbit-tax-core` instead of a separate `orbit-tax-rules` crate.** Workable, but CI tooling (a build step that hashes a YAML file) would then depend on the full tax-core crate including future jurisdictional RuleData variants; a shared canonicalization crate that both `orbit-tax-core` and the CI xtask depend on is cleaner.
- **Load rule data as JSON instead of YAML.** YAML is reviewer-friendly for rate tables (comments allowed, trailing commas forgiven). The `serde_yaml → serde_json` path is one line and the canonical hash is computed over the JSON form anyway; YAML is just the authoring surface.
- **Embed rule data in the binary at compile time.** Tempting (no DB read path, trivially fast). Rejected: ADR-004 wants runtime loading so that a rule-set publish is a data operation, not a redeploy; also, replay-against-historical-rule-set needs older rule data present in the DB.
- **Use a declarative DSL (rhai, rune) for the calculation itself.** Rejected by ADR-003. This ADR doubles down: every calculator is pure Rust.
- **Skip the two-step publish and let `active` happen on deploy.** Rejected: operational-error blast radius is too large. Two-step costs an extra CLI invocation per rule-set publish (rare event) and gains a separate review gate.
- **Separate "promoter" role in Postgres for the `UPDATE ... SET status='active'` statement.** Considered; simpler v1 is "the operator CLI has DB creds with the migration role; normal `orbit_app` cannot promote." Distinct role adds operational complexity without improving the audit story (the `audit_log` row tells us who promoted).

## Consequences

**Positive:**
- The engine↔app boundary is a trait method with typed inputs and a typed result; every calculation flow in v1 reduces to one call site of `run_calculation`.
- `RuleSetLoader::active_for` is the single production entry point; `status='active'` enforcement is a one-line SQL predicate that can't be accidentally bypassed.
- CI hasher and runtime loader share canonicalization, closing SEC-081 at the code level.
- Two-step publish exists at Slice 0 level as a CLI command; no retrofit cost when Slice 4 ships.
- UK paper-design gate (ADR-003) holds: a future `orbit-tax-uk` is a new crate + a new `JurisdictionCode::UK` + a new `RuleData::Uk` variant. No Spain code touched.

**Negative / risks:**
- Grant metadata as a free-form bag (`GrantMetadata`) is a weak spot — if UK's `CSOP` carries a field Spain's `RSU` doesn't, today's contract stores it as JSON and the calculator pattern-matches on expected keys. Acceptable because UK is paper-design only in v1; the first real UK implementation may tighten this to an enum.
- `BTreeMap` everywhere loses constant-time hash lookups. At calculator scale (hundreds of events) this is negligible; lint wins over perf.
- The `internal-test-loader` feature flag is a small footgun (shipping a release with it on defeats SEC-087). Mitigation: a release CI job greps the built binary for a sentinel.
- Replay sampler alerting depends on engine-version diffs being small and rare. Mitigation: `engine_version` is tracked in every `calculations` row, and the sampler excludes cross-version comparisons from the alert set automatically.

**Tension with prior ADRs:**
- None. ADR-003 and ADR-004 are the paper; this ADR is the ink.

**Follow-ups:**
- Implementation engineer: scaffold the ★ crates in Slice 0; types-only `orbit-tax-core` with one passing doc test.
- Slice-4 follow-up: author `es-2026.1.0.yaml`, implement `orbit-tax-spain::SpainTaxCalculator`, implement the regression fixture suite (SEC-084) with synthetic personas whose totals are pinned.
- Slice-4 follow-up: wire `orbit rules:promote` CLI command.
- Security-engineer: confirm the numeric-diff summary that `orbit rules:promote` prints is not a sensitive surface (it shows rate deltas, not user data — should be fine).
- Solution-architect (v1.1): revisit `GrantMetadata` shape when UK calculator lands.
