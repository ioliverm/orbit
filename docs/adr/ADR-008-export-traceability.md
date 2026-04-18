# ADR-008: Export traceability

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

§7.9 + US-009 require every PDF/CSV export to be **fully traceable** back to its inputs and rule set, and **reproducible** — a stored export's inputs + rule-set version must re-run to byte-identical numbers. §4.1 + US-013 + §7.4 require the **non-advice disclaimer** + **rule-set version + AEAT guidance date** to appear visibly on every output. Spanish tax record-keeping convention (and the AEAT prescription window of 4–6 years) drives a **7-year retention** floor on exports the user has produced for a gestor.

This ADR is downstream of ADR-004 (rule-set versioning) and ADR-005 (data model for `exports`); it specifies the user-visible and metadata-level traceability scheme.

## Decision

### Visible footer (every PDF page; CSV header comments)

Every exported document carries this footer, rendered identically across PDF and CSV (CSV-as-comment-lines prefixed with `#`):

```
Orbit calculation export
Traceability ID: <UUID>
Calculated under rule set: es-2026.1.0 (AEAT guidance as of 2026-03-15)
Engine version: orbit-engine v0.4.2
Inputs hash: sha256:9f3c...e1b2
Result hash: sha256:7a04...c92d
Computed at: 2026-04-18T14:23:11+02:00
ECB FX as of: 2026-04-17 (1 EUR = 1.0823 USD)  [or "User-overridden" where applicable]

Esto no es asesoramiento fiscal ni financiero. Consulta con tu asesor fiscal.
This is not tax or financial advice. Consult your tax advisor.
```

The visible footer appears on **every page** of a PDF (not just the last page). The disclaimer line is non-removable from the export template; CSV variant has it in the header comment block.

### PDF metadata

PDF documents embed the same fields in the document's structured metadata so they survive copy/print/forward:

- `Title`: `Orbit — <calculation kind> — <user identifier> — <date>`
- `Author`: `Orbit`
- `Subject`: `Tax calculation export — rule set <id>, AEAT guidance <date>`
- `Keywords`: `traceability:<UUID>;rule_set:<id>;inputs_hash:<short>;result_hash:<short>`
- `Creator`: `Orbit engine <version>`
- Custom XMP fields: `orbit:traceability_id`, `orbit:rule_set_id`, `orbit:rule_set_content_hash`, `orbit:inputs_hash`, `orbit:result_hash`, `orbit:computed_at`, `orbit:engine_version`.

This means a gestor or auditor receiving the PDF can read the traceability metadata programmatically without parsing the visible footer.

### Traceability ID

A `UUID v4` minted at calculation time, persisted on:

- `calculations.traceability_id` (ADR-005) — actually the same column may live on `exports`; second-pass solution-architect to finalize whether the ID is per-calculation or per-export. Initial decision: **per-export**, so the same calculation re-exported produces a new traceability ID linked to a new audit-log entry, but the underlying calculation's `inputs_hash` + `result_hash` link the two for replay.
- `exports.traceability_id`
- `audit_log` — every export generation creates an `export.generate` audit entry carrying the traceability ID, the calculation it's derived from, and the user actor.

This satisfies the US-009 acceptance criterion: *"a traceability ID that matches an entry in the user's audit log."*

### Reproducibility test

A CI test (and a periodic production sampler, per ADR-004's replay-sampler discipline) takes a stored export and re-runs the calculation engine against:

- the persisted `inputs` (or the snapshot referenced by `inputs_hash`),
- the pinned `rule_set_id`,

and asserts:

- `new_result_hash == export.result_hash` — byte-identical numeric output.

PDF rendering itself is not asserted byte-identical (PDF generation includes timestamps and font subset randomness); only the *numbers* are. The export's `result_hash` is computed over the canonicalized result JSON, not the PDF bytes.

### Storage and retention

- Generated PDFs and CSVs stored in Hetzner Object Storage (ADR-002), keyed by `users/<user_id>/exports/<traceability_id>.<ext>`.
- **Retention: 7 years** from generation (`exports.retained_until = created_at + interval '7 years'`). This covers the AEAT prescription window with a margin and meets the Spanish documentation-retention convention for tax records.
- A worker job (weekly) deletes objects whose `retained_until` is past.
- **GDPR erasure:** when a user is hard-deleted (US-011 + ADR-005), their export objects are deleted *immediately* despite the 7-year retention, on the basis that the **right to erasure overrides the convenience-retention** unless a legal-obligation retention applies. The underlying `calculations` row is pseudonymized (ADR-005), not deleted, satisfying audit-trail requirements; the user's PDF copies are removed because the *user* requested it. This is the security-engineer's call to confirm; default is "erasure overrides export retention."

### Free-tier behavior

Per US-009 + US-012, exports are a paid feature. Free-tier users see the upgrade CTA and never produce a stored export object. No traceability ID is minted for a free-tier preview.

### Rule-set-version-on-old-export UX

Per US-009: when an export generated under rule-set `es-2026.1.0` is viewed six months later under `es-2026.2.0`:

- The export itself is immutable; its footer continues to display `es-2026.1.0`.
- Within Orbit, the calculations list shows the export with a "computed under superseded rule set" badge.
- The user can click "recompute under current rules" → new calculation → new export → new traceability ID. The old export remains accessible for the retention window.

### CSV format

CSV exports follow the same traceability scheme:

- File begins with a comment block (lines prefixed `#`) carrying the same footer fields as the PDF.
- A blank line separates the header from data.
- `result_hash` and `inputs_hash` in the comment block are computed over the **same canonicalized result/input JSON** as the PDF — i.e., a PDF and CSV exported from the same calculation share `inputs_hash` and `result_hash` (only `traceability_id` differs because they are separate `exports` rows).

## Alternatives considered

- **Footer only on first page of PDF.** Rejected — pages get separated, screenshotted, forwarded; per-page footer is cheap and meets the §7.4 "every output carries the disclaimer" requirement.
- **Embed full inputs in the PDF metadata for self-contained reproducibility.** Considered. Rejected because grant values + share counts in PDF metadata could leak more than intended if the PDF is shared; the hash + ID scheme is sufficient because Orbit retains the inputs server-side.
- **Sign exports with a cryptographic signature (e.g., embedded PGP).** Considered for tamper-evidence. Deferred to v1.1; introduces key-management overhead the security-engineer should weigh in on. The hash + audit-log scheme is sufficient for v1's "trace back to what was computed" need.
- **Store exports in the database (BLOB) instead of object storage.** Rejected on cost and on Postgres-unfriendliness for binary blobs at scale; object storage is purpose-built.

## Consequences

**Positive:**
- An export carries everything a gestor or auditor needs to verify it: the rule set, the AEAT guidance date, the inputs hash, the result hash, the disclaimer, and a traceability ID that ties back into the audit log.
- The reproducibility property is structurally guaranteed by ADR-004's hashing discipline; this ADR just surfaces the hashes.
- Retention is explicit (7 years) and tied to a real regulatory anchor (AEAT prescription).
- GDPR erasure has a defined posture: PDF blobs go, calculation row is pseudonymized.

**Negative / risks:**
- 7-year retention on object storage at scale is a real future cost line, but the per-export size is small (PDFs ≈ 50–200 KB, CSVs smaller); v1 cost is negligible.
- PDF rendering library choice (deferred to implementation pass) must support custom XMP metadata. Both `typst` and headless Chromium routes can do this; trivial constraint.
- The decision to delete export blobs on GDPR erasure (overriding the 7-year retention for that user) needs security-engineer sign-off; the alternative (retain pseudonymized PDFs) is awkward because PDFs themselves contain the user's email in `Title`.

**Follow-ups:**
- **Security-engineer:** confirm GDPR erasure overrides 7-year export retention (default decision in this ADR) vs retain-pseudonymized.
- **Implementation engineer:** select PDF rendering library (likely `typst`); confirm XMP metadata support; build the per-page footer template.
- **Implementation engineer:** wire the reproducibility CI test that re-runs a stored export's calculation and asserts hash equality.
- **Solution-architect (second pass):** finalize whether `traceability_id` lives on `calculations` or `exports` (this ADR's tentative answer: per-export, with FK back to calculation).
- **Operational:** weekly retention-sweep worker job.
