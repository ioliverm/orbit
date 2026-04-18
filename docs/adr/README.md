# Orbit — Architecture Decision Records

This directory holds Orbit's ADRs. Each ADR captures a single architectural decision: context, decision, alternatives, consequences. ADRs are versioned in the repo; once an ADR is `Accepted`, it is not edited — corrections are made by writing a new ADR that supersedes it.

**Status as of 2026-04-18: all ADRs below are `Proposed`. ADR-001..ADR-008 await `security-engineer` sign-off; ADR-009..ADR-014 are the synthesis round (closing gaps between security, UX, and requirements reconciliation) and feed directly into the `implementation-engineer` Slice 0 / Slice 1 kick-off.**

## Index

### Foundations (ADR-001..ADR-008, first pass)

| # | Title | Summary |
|---|---|---|
| [ADR-001](./ADR-001-tech-stack-baseline.md) | Tech stack baseline | Rust (axum) backend + React SPA (Vite) frontend + PostgreSQL 16 single store + single-binary multi-role deploy. |
| [ADR-002](./ADR-002-cloud-provider-and-deployment-topology.md) | Cloud provider and deployment topology | **Hetzner Cloud (Falkenstein)**: 2× small VMs (CX22 API+worker, CX32 self-managed Postgres) + Storage Box backups + Object Storage + Bunny.net CDN. ~€60–70/mo all-in, well under €200/mo ceiling. |
| [ADR-003](./ADR-003-hybrid-tax-engine-architecture.md) | Hybrid tax-engine architecture | Shared `orbit-tax-core` primitives + per-country `TaxCalculator` trait impls + externalized versioned rule data (YAML → Postgres). UK paper-design acceptance gate passes structurally. |
| [ADR-004](./ADR-004-rule-set-versioning-and-traceability.md) | Rule-set versioning and calculation traceability | Immutable published rule sets keyed by semver-like ID + AEAT guidance date + content hash. Calculations stamp `(rule_set_id, content_hash, inputs_hash, result_hash, engine_version)`. Periodic replay sampler verifies determinism. |
| [ADR-005](./ADR-005-data-model-outline.md) | Data model outline | Entity outline + Postgres RLS shared-schema multi-tenancy. `users` cascades; `rule_sets`/`fx_rates`/`market_quotes_cache` are global. GDPR erasure = single CASCADE; audit log + calculations pseudonymized for retention. |
| [ADR-006](./ADR-006-market-data-vendor-selection.md) | Market-data vendor selection | **Finnhub** (free → Standard $50/mo) primary; **Twelve Data** standby. Wired behind `MarketDataProvider` trait. 15-min shared cache; honest staleness UX; no PII transmitted. Vendor-licensing verification is a launch-blocker follow-up. |
| [ADR-007](./ADR-007-fx-source-ecb-integration.md) | FX source — ECB integration | Daily ECB reference rate ingested by worker at 17:00 Madrid; non-publication-day fallback walks back ≤7 days with staleness indicator; user overrides stored per-calculation; sensitivity bands at 0% / user-spread (default 1.5%) / 3%. |
| [ADR-008](./ADR-008-export-traceability.md) | Export traceability | PDF per-page footer + XMP metadata + CSV header comments carry traceability ID, rule-set version, AEAT date, inputs hash, result hash, non-advice disclaimer. 7-year retention; GDPR erasure overrides retention. |

### Synthesis round (ADR-009..ADR-014 — closes the security / UX / slicing gaps)

| # | Title | Summary |
|---|---|---|
| [ADR-009](./ADR-009-frontend-architecture.md) | Frontend architecture | React 18 + Vite + TypeScript strict + React Router v6 (data-router) + TanStack Query + Zustand (tiny) + React Hook Form + Zod + LinguiJS (ES/EN) + plain CSS tokens + Lucide icons + Vitest/Playwright. Strict CSP from day one; no `'unsafe-inline'`. Hand-rolled vesting canvas in Slice 1 (no chart lib). |
| [ADR-010](./ADR-010-api-contract-shape.md) | API contract shape | REST over JSON, same-origin, path-versioned `/api/v1/`, cookie-auth (opaque session + refresh rotation + CSRF double-submit), uniform error envelope with stable `code`, OpenAPI 3.1 emitted by `utoipa` + drift-checked, types-only TS generation for the SPA, Money as decimal-string. Slice-1 endpoint list locked. |
| [ADR-011](./ADR-011-authentication-session-mfa.md) | Authentication, session, and MFA architecture | argon2id (OWASP 2024 params) + HIBP k-anonymity + opaque session cookies (`HttpOnly`+`Secure`+`SameSite=Lax`) + refresh rotation with family revoke-on-reuse + 32-byte tokens hashed at rest + TOTP scaffolded (501 in Slice 1, flip-switch for v1.1) + new-device email + Postgres-backed rate-limit. Signin + reset Mermaid sequences. |
| [ADR-012](./ADR-012-rule-set-pipeline-and-engine-contract.md) | Rule-set pipeline and tax-engine contract | Crate topology (`orbit-tax-core` trait + `orbit-tax-rules` canonicalizer/hasher + per-country `orbit-tax-spain`); `RuleSetLoader::active_for` as the only production path (SEC-087); two-step publish (PR → `proposed`, CLI → `active`, SEC-083); single `run_calculation` helper that stamps `(rule_set_id, content_hash, engine_version, inputs_hash, result_hash)` on every row (SEC-086); replay sampler contract. Scaffolded Slice 0, implemented Slice 4. |
| [ADR-013](./ADR-013-repository-and-deployment-scaffold.md) | Repository and deployment scaffold (Slice 0 blueprint) | Single monorepo (Cargo workspace + pnpm workspace); `sqlx` plain-SQL migrations; single `orbit` binary with `api`/`worker`/`migrate`/`rules` subcommands; GitHub Actions CI with required `gitleaks` / `cargo-deny` / `cargo-audit` / `pnpm audit` / OpenAPI drift / `axe` a11y smoke; `scp + systemctl restart` deploy (no Kamal/K8s) with atomic symlink swap; docker-compose-Postgres for local dev; Better Stack uptime + structured JSON logs via `orbit_log::event!`. Every S0-01..S0-30 item mapped. |
| [ADR-014](./ADR-014-slice-1-technical-design.md) | Slice-1 technical design | Authoritative DDL for `users`, `sessions`, `email_verifications`, `password_reset_tokens`, `audit_log`, `dsr_requests`, `rule_sets` (empty, trigger-armed), `rate_limit_buckets`, `residency_periods`, `grants`, `vesting_events`. Named RLS policies (`tenant_isolation`). Vesting-derivation pseudocode with AC-4.3.* invariants. Signup-wizard state machine (URL-first, onboarding-gate middleware). First-grant E2E sequence diagram. Explicit deferral list so no TBD survives Slice 1. |
| [ADR-015](./ADR-015-slice-0-local-first-scope-split.md) | Slice 0 local-first scope split (0a / 0b) | Amends ADR-013: Slice 0 splits into **0a (local-green)** — all CI, app-level controls, RLS/auth primitives, CSP-strict SPA shell, `orbit_support` role provisioned now — and **0b (deploy-green)** — Hetzner VMs, Caddy/Let's Encrypt, nftables, systemd `LoadCredential=`, offsite backups, uptime monitor, governance docs. 0a gate blocks Slice 1; 0b gate blocks first external user. Supersedes ADR-014 §"Upstream ambiguities" item 7 interim mitigation (G-1 resolved on the provision-now path). |

## Conventions

- Filename: `ADR-NNN-kebab-title.md`. NNN is a zero-padded sequence number; never reused.
- Status lifecycle: `Proposed → Accepted → Superseded` (or `Withdrawn`).
- Superseding ADRs link back to the predecessor; predecessors are not deleted or edited beyond a status flip.
- Decisions reversible in less than a week do not warrant an ADR — just decide in the design doc.

## Open follow-ups feeding into security-engineer review

- ADR-002: Verify Hetzner managed Postgres status; verify Bunny.net EU-only PoP enforcement; outbound-email provider EU-residency; restore-drill before paid launch.
- ADR-003: Rule-set YAML PR sign-off process appropriate for regulatory exposure.
- ADR-004: Confirm pseudonymization-for-audit-retention is the right GDPR posture vs full delete.
- ADR-005: Confirm `audit_log` pseudonymization defensibility under AEPD; `ip_hash` salt-management strategy.
- ADR-006: **Verify Finnhub commercial-tier ToS explicitly permits SaaS redistribution of delayed quotes** (launch-blocker); same for Twelve Data fallback; document Finnhub as US-headquartered processor in GDPR processor map.
- ADR-007: No security-engineer item; ECB is free, public, no PII.
- ADR-008: Confirm GDPR erasure overrides 7-year export retention vs retain-pseudonymized.
- ADR-011: Confirm HIBP k-anonymity outbound endpoint (`api.pwnedpasswords.com`) in the nftables allowlist (SEC-149); confirm Postmark EU as sub-processor if chosen over Scaleway TX Email.
- ADR-013: Confirm the three observability sinks (Better Stack, Scaleway Logs option, uptime monitor) are EU-residency compliant or SCCs+TIA-covered (SEC-121).
- ADR-014: Slice-1 `audit_log` grant split — acknowledge the documented compromise that `orbit_app` retains UPDATE/DELETE on `audit_log` until Slice 2–3 introduces the `orbit_support` role; confirm the code-level CI lint (INSERT-only) is acceptable as the interim SEC-102 mitigation.

## Next handoff

**Security-engineer** reviews ADR-009..ADR-014 for: (a) CSP strictness (ADR-009, SEC-180), (b) cookie-auth posture confirmation (ADR-010, SEC-006), (c) MFA cool-down + new-device email pattern (ADR-011, SEC-009/010), (d) the `internal-test-loader` feature-flag release guardrail (ADR-012, SEC-087), (e) the Slice-0 CI gate set (ADR-013, S0-01..S0-30), and (f) the Slice-1 `audit_log` mitigation (ADR-014, SEC-102).

**Implementation-engineer** can start Slice 0 against ADR-013 (repo scaffold + CI + deploy + secrets + backups) in parallel with the security review of ADR-009..ADR-014; Slice 1 (feature work per ADR-014) begins once the Slice-0 checklist is green. Backend DDL for Slice 1 is committed via `migrations/20260418120000_init.sql` (Slice 0) + `migrations/20260425120000_slice_1.sql` (Slice 1).
