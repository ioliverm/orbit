# ADR-004: Rule-set versioning and calculation traceability

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

§7.1 of the spec is non-negotiable: every IRPF bracket, ahorro-base tier, autonomía rate table, Art. 7.p cap, and Modelo 720 threshold lives in a versioned rule set; every calculation, scenario, and export is stamped with the rule-set version under which it was computed; users can recompute under current rules; a public changelog is maintained. R-3 (tax-rule volatility) and R-6 (accuracy liability) both depend on this mechanism working correctly.

The reproducibility requirement is sharper than "stamp a version": **a calculation produced in 2026 must replay byte-identically in 2029** by re-running it against the pinned rule set. This requires (a) immutable rule-set artefacts, (b) deterministic engine behaviour against pinned rules, and (c) preservation of the rule data itself for the AEAT prescription window (6 years per §7.9).

## Decision

### Rule-set lifecycle: published rule-sets are immutable

A rule set is a tuple of:

- **Identifier:** semver-like `es-2026.1.0`. Major = jurisdiction-year boundary; minor = within-year additive change (e.g., a mid-year autonomía table revision); patch = corrigendum (typo, formatting, *never* a numeric fix to a published rule).
- **AEAT guidance date:** the `YYYY-MM-DD` anchor for the AEAT publication this rule set encodes.
- **Effective-from / effective-to:** date range during which this rule set is the *default* for new calculations in that jurisdiction. Multiple published rule sets coexist; only one is the active default at any time.
- **Content hash:** SHA-256 over the canonicalized YAML (sorted keys, normalized whitespace, no comments). Stamped at publish time and never recomputed.
- **Status:** `proposed | active | superseded | withdrawn` (state machine; see below).

**Once a rule set has status `active`, its content is frozen.** Errors are corrected only by publishing a *new* rule set (e.g., `es-2026.1.1`) with `supersedes` pointing back. The original record remains in the database forever and remains usable for replay. No `UPDATE` on rule-set content; only `INSERT` of the successor and `UPDATE` of the predecessor's status to `superseded`.

### Status state machine

```
proposed --(sign-off)--> active --(successor published)--> superseded
proposed --(rejected)--> withdrawn
```

Transitions are recorded with timestamp + actor in an `audit_log` entry. `proposed` rule sets are not used for user-visible calculations; they exist for internal validation against historical test cases.

### Storage

Two-layer storage, both write-once:

1. **Source of truth: YAML files in repo at `/rules/<jurisdiction>/<id>.yaml`.** Code-reviewed via PR. Each PR that publishes a rule set must include: the YAML file, a regression test suite (a set of `(events, expected_result)` cases), and a changelog entry. The implementation engineer will define the YAML schema; this ADR commits only to "YAML, canonicalizable, hashable."
2. **Runtime: Postgres `rule_sets` table.** On deploy, a migration ingests new YAML files into the table, computes content hash, and inserts. **A migration that would change the content of an `active` rule set fails the build.** Promotion from `proposed` to `active` is a separate, explicit operational step (a CLI command on the worker binary), not a side effect of deploy.

### Calculation stamping

Every persisted calculation row carries:

- `rule_set_id` (FK to `rule_sets`)
- `rule_set_content_hash` (denormalized copy — protects against the unthinkable case of a rule-set row being mutated in violation of policy)
- `engine_version` (Rust binary semver — secondary discriminator; unlikely to matter but cheap to record)
- `inputs_hash` (SHA-256 over the canonicalized input event set + context)
- `result_hash` (SHA-256 over the canonicalized output, for replay verification)

A calculation is considered **replayable** if re-running the current binary against the pinned `rule_set_id` and the persisted inputs produces a result whose hash matches `result_hash`. A periodic worker job (monthly) samples old calculations and re-runs them; mismatches trigger an alert and an engineering investigation. This is a structural defence against accidental engine non-determinism (e.g., HashMap iteration order leaking into `FormulaTrace`).

### User-facing surface (UI + exports)

- Every calculation result page displays the active rule-set ID and AEAT guidance date in a tooltip: *"Calculated under rule set es-2026.1.0, AEAT guidance as of 2026-03-15."* (per §7.1).
- "Recompute under current rules" action: takes a stored calculation, runs it again with the currently-active rule set for the same jurisdiction, persists the new result alongside the old one, and surfaces a diff. The original is never mutated.
- Public changelog at `/changelog/tax-rules` lists rule sets with status, AEAT guidance date, supersedes link, and a human-readable summary of what changed (e.g., "Cataluña 2026.2.0: revised autonomía rate per DOGC 2026-09-12").

### Export traceability hand-off to ADR-008

Exports inherit rule-set stamping; the visible footer + PDF metadata + traceability ID scheme are specified in ADR-008. This ADR commits to: *the data needed to render those exports is always present on the calculation row.*

### Retention

- `rule_sets` table: **never purged.** Storage cost is negligible (kilobytes per rule set, dozens per year per jurisdiction).
- `calculations` table: per §7.9, audit log retained 6 years (AEAT prescription); calculation rows referenced by exports retained at least as long. User-deletion under GDPR (US-011) hard-deletes the user's PII but the **calculation rows are pseudonymized** (user_id replaced with a tombstone) and retained for the audit window — this is consistent with GDPR's exemption for legal-obligation retention. Security-engineer to confirm this posture is defensible.

## Alternatives considered

- **Mutable rule sets with an audit-log-only history.** Rejected: replay verification is impossible if a rule set's content can change after a calculation cited it. Even with audit-log history, restoring "the rule set as it existed at time T" becomes a reconstruction exercise rather than a primary-key lookup.
- **Rule sets as Git tags / commits, no Postgres table.** Tempting (Git is already content-addressable) but couples the running binary to a Git checkout at runtime and complicates the worker/API binary boundary. Postgres rows with content hash give the same immutability guarantee with no runtime Git dependency.
- **Per-calculation snapshot of rule data inline (denormalized).** Considered — store the full rule-set blob on every `calculations` row. Rejected on storage cost (rule sets are kilobytes, calculations are millions over time) and on the existence of `content_hash` as a cheap integrity check.
- **Semantic versioning where patch = numeric correction.** Rejected. A correction to a published number is a *new rule set*, not a patch. Otherwise replay is meaningless.

## Consequences

**Positive:**
- Replayability is structural: `(rule_set_id, inputs_hash) → result_hash` is a deterministic function the system can verify.
- AEAT mid-year publication is absorbed by publishing a successor rule set; no race against deploy windows.
- The "recompute under current rules" UX (US-009 acceptance criterion) falls out trivially.
- Engine non-determinism bugs are detectable by the periodic replay sampler before they cause user-visible drift.

**Negative / risks:**
- Operational discipline required: nobody, ever, manually edits an `active` rule-set row. Mitigation: deploy migration check + Postgres trigger that rejects `UPDATE` on `rule_sets` rows where `status = 'active'`.
- Two-step publish (PR-merge → worker CLI promotion to `active`) is more friction than one-step. Acceptable; the stakes warrant it.
- Engine non-determinism is a real risk in Rust if any calculation accidentally depends on `HashMap` iteration order or system locale. Mitigation: lint for `HashMap` in calculation crates (use `BTreeMap`); set `LC_ALL=C` in the worker; the periodic replay sampler catches escapes.

**Follow-ups:**
- Implementation engineer: define the YAML canonicalization spec (sorted keys, UTF-8, LF line endings, `Decimal` rendered as decimal-string not float).
- Implementation engineer: build the migration-time content-hash check and the runtime trigger preventing mutation of `active` rule sets.
- Solution-architect (second pass): specify the exact `inputs_hash` canonicalization for `TaxableEvent` lists (sort order, money serialization).
- Define the periodic replay-sampler schedule and alert routing.
- Security-engineer: confirm pseudonymization-for-audit-retention is the right GDPR posture (vs full delete with calculation-row deletion).
