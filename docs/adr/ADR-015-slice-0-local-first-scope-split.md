# ADR-015: Slice 0 local-first scope split (0a / 0b)

- **Status:** Accepted (amended 2026-04-19 — see "Amendment 2026-04-19" below)
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)
- **Traces to:** ADR-013 (amends; original Slice 0 blueprint), ADR-014 (supersedes the §"Upstream ambiguities resolved unilaterally" item 7 interim mitigation path), `docs/implementation-handoff.md` §2/§6 (G-1), `docs/security/security-checklist-slice-0.md` S0-01..S0-30, `docs/requirements/v1-slice-plan.md` Slice 0 / Slice 8.

## Amendment 2026-04-19 — 0b deferred to Slice 8

**What changed.** The product owner decided on 2026-04-19 to defer cloud deployment to the end of the v1 build, after every feature slice is complete and polished. Concretely:

- **0b is no longer a "before Slice 2 or 3" checkpoint.** It becomes a self-contained final slice — **Slice 8 — Production deployment & launch gate** — authored in `docs/requirements/v1-slice-plan.md` v1.1.
- **The per-item mapping below (0a / 0b columns) is unchanged.** Every ticked 0b item still needs to happen; only the *when* moves.
- **The 0b trigger in the "Slice 0b trigger" section below is superseded by** the Slice 8 trigger: 0b work executes as Slice 8, begins once Slice 7 is green, and closes before any external user (free or paid) is onboarded. There is no earlier trigger.
- **Slice 7's scope narrows** to match: third-party pen-test, DPA publication, sub-processor list publication, and breach-notification tabletop all move from Slice 7 to Slice 8 because each requires the production stack or a public surface that does not exist before Slice 8.
- **Slice 3 Stripe** runs in test mode through Slice 7; live-mode cutover and the production webhook endpoint are a Slice 8 step.
- **Slice 5 Finnhub** runs on the free/dev tier through Slice 7; the commercial-tier contract + ToS-confirmed-for-SaaS-redistribution is a Slice 8 launch-blocker (same for the Twelve Data standby contract).

**What did not change.**

- The 0a / 0b scope split itself. Every app-level control Slice 0a covered is still covered by Slice 0a; every deploy-level control 0b owned is still owned by 0b.
- The ADR-014 §"Upstream ambiguities" item 7 supersession (the `orbit_support` role is provisioned in the 0a init migration, and the `audit_log` grants shipped in 0a are the final shape).
- The threat-model posture — the Consequences note below about "threat envelope incomplete until 0b" still applies: the envelope is incomplete until Slice 8 closes, and the mitigation remains "no external user exposed to the stack until that gate closes."

**Why this is safe.** 0a already covers every app-level control the threat model names. The deferred set is entirely deploy-level — Hetzner VMs, TLS on a public domain, nftables, offsite backups, uptime monitor, published legal surface, and the pen-test against real infra. None of it is exercised while only the product owner uses the app on `localhost`. Moving the checkpoint to the end of v1 does not change the work required; it changes when the work is scheduled, matched to the first moment the deploy-level envelope is load-bearing (i.e., when an external user is imminent).

**Operational implication.** The "amber" 0b items called out in the Consequences section remain amber for longer. The security-engineer sign-off record must continue to enumerate them and re-confirm them at Slice 8 close rather than at the previously scoped "before Slice 2 or 3" checkpoint.

---


## Context

ADR-013 treated Slice 0 as a single atomic block: repo hygiene + CI + Hetzner VM deploy + backups + governance, all landing before Slice 1 begins. The 30-item security checklist was scoped against that plan.

The product owner (Ivan, sole operator) has since decided to **deprioritize cloud deploy**. The new shape: land every Slice-0 control that can be verified against a local development stack first, and defer the deploy-level controls (Hetzner provisioning, production TLS, host firewall, offsite backups, uptime monitoring, published governance docs) to a second checkpoint that fires before any real user touches the system.

Two reasons for the split:

1. **Capacity.** Ivan is the only operator. Standing up Hetzner + Caddy + nftables + pg_basebackup + Storage Box + Better Stack is ~2–3 days of deploy-shaped work that blocks every feature slice. Pushing it right doesn't remove the work; it lets Slice 1 ship in parallel with deploy preparation instead of behind it.
2. **Validation.** The threat model (`threat-model.md`) assumes deploy-level controls (private network, TLS 1.3, nftables deny-default). Those controls can be designed and skeleton-committed before they are executed. The app-level controls (RLS, argon2id, `Tx::for_user`, `orbit_log::event!`, audit-log write-only) can be fully exercised against local Postgres and the Vite dev server. The two checkpoints have independent value.

The risk is that "defer until later" silently becomes "forgotten." This ADR exists so the deferred set is explicit, the re-evaluation trigger is named, and every downstream agent works from a published contract.

## Decision

Split Slice 0 into two checkpoints with independent exit gates:

- **Slice 0a — local-green.** Everything verifiable against a local Docker Compose Postgres + a local Vite dev server. Exit gate blocks Slice 1.
- **Slice 0b — deploy-green.** Everything requiring provisioned infrastructure, a public domain, published legal surface, or an offsite backup target. Exit gate blocks **first external user**, paid or free.

### Per-item mapping (S0-01 .. S0-30)

Seeded from the checklist; refined where a single item splits across both checkpoints.

| # | Item | 0a | 0b | Notes |
|---|---|---|---|---|
| S0-01 | gitleaks pre-commit + CI + full-history in deploy pipeline | ✓ (pre-commit + PR check) | ✓ (full-history in deploy pipeline) | PR check lands in 0a; the full-history scan tied to `deploy.yaml` runs once that workflow activates. |
| S0-02 | `cargo audit` + `cargo deny` on PR + nightly | ✓ | — | |
| S0-03 | `pnpm audit` on PR | ✓ | — | |
| S0-04 | Lockfiles committed, no `*`/`latest` ranges | ✓ | — | |
| S0-05 | Actions pinned to SHA + CODEOWNERS on workflows + min `permissions:` | ✓ | — | |
| S0-06 | CI secrets in GitHub Environments; `production` requires manual review | **Partial** | ✓ | Environment + reviewer list configured in 0a; `deploy.yaml` committed but disabled (`if: false` / `workflow_dispatch`-only) until 0b. |
| S0-07 | CODEOWNERS covers `/rules/**`, `/migrations/**`, workflows, auth/crypto; branch protection | ✓ | — | |
| S0-08 | Clippy/cargo-check forbidding `HashMap` in calc crates + raw `pool.acquire()` | ✓ | — | |
| S0-09 | `orbit_log::event!` wrapper + `Display`/`Debug` deny on sensitive types + compile-fail fixtures | ✓ | — | |
| S0-10 | CycloneDX SBOM in release workflow | ✓ (CI artifact) | ✓ (release-store upload) | SBOM is generated and uploaded as a CI artifact in 0a; long-term retention to a release store is deployed with the release pipeline at 0b. |
| S0-11 | Hetzner VMs (FSN1) + at-rest encryption | — | ✓ | |
| S0-12 | Caddy TLS 1.3 + Let's Encrypt | — | ✓ | |
| S0-13 | Response headers (CSP, HSTS, X-CTO, X-Frame, Referrer, Permissions, COOP) | ✓ (all except HSTS) | ✓ (HSTS end-to-end) | Verified against the local dev server via `curl -I`; HSTS requires real HTTPS so it's declared but only observable at 0b. |
| S0-14 | CORS explicit origin + Allow-Credentials + allowlist | ✓ | — | Local origin `http://localhost:<port>` in 0a; prod origin added at 0b. |
| S0-15 | nftables deny-default outbound + allowlist | — | ✓ | |
| S0-16 | Postgres private-network + `pg_hba` TLS + `orbit_app` without BYPASSRLS | **Partial** | ✓ | `orbit_app` + `orbit_support` roles, no BYPASSRLS, no superuser, and `pg_hba.conf` TLS-required shipped in the local Compose image in 0a. Private-network binding lands at 0b. |
| S0-17 | RLS enabled by default on user-scoped tables | ✓ | — | |
| S0-18 | `rule_sets` UPDATE-where-status=active trigger | ✓ | — | |
| S0-19 | Secrets via systemd `LoadCredential=` | **Pattern in 0a** | ✓ | In 0a, secrets live in a git-ignored `.env` at mode 0600 loaded by the local stack. The `LoadCredential=` shape is committed in the systemd unit template but activates with the deploy. |
| S0-20 | MFA on GitHub, Hetzner, registrar, DNS, Bunny, email, billing | ✓ (GitHub only) | ✓ (remainder) | Only GitHub exists today; the rest are provisioned lazily as 0b creates the accounts. |
| S0-21 | argon2id at OWASP-2024 params + pinning test | ✓ | — | |
| S0-22 | Session cookie factory + CSRF double-submit | ✓ | — | |
| S0-23 | `Tx::for_user(user_id)` sole query-handle API | ✓ | — | |
| S0-24 | Append-only `audit_log`; `orbit_app` INSERT-only | ✓ | — | See §"Additional decisions" — G-1 resolved on the "provision `orbit_support` now" path, so the interim CI-lint mitigation is not needed. |
| S0-25 | IP-hash salt (32-byte CSPRNG) + HMAC-SHA256 helper | ✓ | — | |
| S0-26 | Nightly `pg_basebackup` + WAL archive + `age` encryption to Storage Box | — | ✓ | |
| S0-27 | End-to-end restore drill dated in runbook | — | ✓ | |
| S0-28 | Uptime monitor on `/healthz` + alert route tested | — | ✓ | |
| S0-29 | Privacy policy + sub-processor register published | — | ✓ | Drafted in 0a as an internal doc only if convenient; publication is 0b. |
| S0-30 | Incident-response runbook + AEPD 72-hour timer + ES/EN templates | — | ✓ | |

**Count:** 0a fully owns 18 items, partially owns 4 (S0-06, S0-13, S0-16, S0-19), and picks up S0-20 in its live-account form. 0b owns the remaining 8 plus the deferred halves of the partials.

### Additional decisions baked in

1. **`orbit_support` Postgres role provisioned in the 0a init migration.** This resolves G-1 (documented in `docs/implementation-handoff.md` §6) on the "provision now" path rather than the CI-lint interim. The `audit_log` grants in the 0a migration are therefore the final shape: `orbit_app` holds `INSERT` only; `orbit_support` holds `SELECT` only; neither holds `UPDATE`/`DELETE`. **This strictly supersedes ADR-014 §"Upstream ambiguities resolved unilaterally" item 7**, which documented a CI-lint-only mitigation on the assumption that `orbit_support` would not exist until Slice 2/3. ADR-014's DDL is unchanged; only the mitigation path is superseded.

2. **Frontend scaffolded in 0a.** React 18 + Vite + TypeScript strict + LinguiJS (ES primary, EN fallback) per ADR-009. Rationale: a CSP-strict SPA shell is the only surface against which the S0-13 header set can be meaningfully verified. Deferring the frontend leaves the CSP posture untested until Slice 1 mid-flight, which is the wrong shape for a security envelope.

3. **E-2 (EUR conversion in Slice 1 dashboard) remains open.** Not reopened by this ADR. Will be re-raised at Slice 1 kickoff per the product owner's instruction.

### Slice 0b trigger

> **Superseded 2026-04-19** by the amendment at the top of this ADR. 0b work executes as **Slice 8 — Production deployment & launch gate** in `docs/requirements/v1-slice-plan.md` v1.1. The original trigger text below is retained for historical context.

*Original text (2026-04-18):* 0b closes **before any external user** (free or paid) is onboarded, and **no later than before Slice 2 or 3 ships**, whichever is sooner. Re-evaluate at the end of Slice 1 acceptance:

- If Slice 1 demos internally only, 0b can continue in parallel with Slice 2 planning.
- If an external user is imminent, 0b blocks onboarding.

The product owner is the authority on this gate. The security-engineer signs off 0b against the deferred items listed above.

*Current trigger (post-amendment):* 0b work executes as a single concentrated slice at the end of v1 (Slice 8), begins once Slice 7 is green, and closes before any external user is onboarded. The "no later than before Slice 2 or 3" condition is removed because the product owner commits not to onboard external users until Slice 8 closes.

## Consequences

**Upsides.**

- Slice 1 can begin once the 0a gate is green, without standing up Hetzner.
- The app-level security envelope (RLS, `Tx::for_user`, `orbit_log`, argon2id, audit-log write-only, CSP-strict SPA) is fully exercised and testable in CI from day one.
- The cost of 0b is unchanged — it is the same work, scheduled later. Nothing new is added by the split.

**Downsides and what we accept.**

- **Partial security sign-off.** The security-engineer can tick 0a items against landed code and migrations, but 0b items sit amber until 0b closes. The sign-off record must explicitly enumerate the amber set so it cannot silently age.
- **Threat envelope incomplete until 0b.** `threat-model.md` assumes private-network binding, TLS 1.3, nftables deny-default, and offsite encrypted backups. Until 0b, Orbit runs only on developer machines and the threat model's deploy-level assumptions are not yet satisfied. This is safe *because no user is exposed to the stack in the 0a window*; it becomes unsafe the moment that changes.
- **"Works locally, not on prod" debt compounds per slice.** Each feature slice built on a 0a-only foundation accumulates assumptions that are not exercised against the production stack. The 0b trigger — before Slice 2 or 3 or first external user — exists specifically to cap this.
- **HSTS and private-network policies are declared before they are observable.** S0-13 and S0-16 have 0a-declared values that will be lint-checked in configuration but only observable against the deploy. A 0b task explicitly re-verifies them end-to-end once the stack is up.

## Alternatives considered

- **Ship Slice 0 end-to-end as originally planned (reject).** Safest posture, but blocks Slice 1 on ~2–3 days of deploy work with no parallelism for a solo operator. Deferred delivery of a demo-able Slice 1 outweighs the marginal safety of finishing deploy first, given that Slice 0a with `orbit_support` covers every app-level control the threat model names.
- **Local-only with no 0b plan (reject).** Lets deploy debt accumulate indefinitely. Discarded on principle — an unnamed deferral is the failure mode this ADR exists to prevent.
- **Per-slice deploy (reject).** Deploying after each feature slice would amortize the work but multiplies the validation effort per slice and contradicts ADR-013's single-binary, atomic-symlink model.

## Links

- ADR-013 — Repository and deployment scaffold (this ADR amends its scope boundary; ADR-013 remains the repo/CI/deploy blueprint).
- ADR-014 — Slice-1 technical design (supersedes the §"Upstream ambiguities resolved unilaterally" item 7 interim mitigation path only).
- ADR-011 — Authentication, session, and MFA architecture (unchanged; consumed by 0a).
- ADR-009 — Frontend architecture (unchanged; consumed by 0a).
- `docs/implementation-handoff.md` §2, §6 (G-1 resolution).
- `docs/security/security-checklist-slice-0.md` — authoritative per-item list; this ADR adds the 0a/0b column.
