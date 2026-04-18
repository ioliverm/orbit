# Orbit v1 — Security checklist: slice 0 (foundations)

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 0.1-draft                                                  |
| Date        | 2026-04-18                                                 |
| Status      | Draft                                                      |
| Owner       | security-engineer                                           |
| Scope       | Slice 0 — must be true before the first user-facing feature ships. |
| Companion   | `threat-model.md`, `security-requirements.md`               |

This is the **non-negotiable floor**. These items do not depend on product features; they are the security envelope inside which every subsequent slice is built. The implementation-engineer checks these off as they scaffold the repo, infrastructure, and CI.

## Repo and CI

- [ ] **S0-01** `gitleaks` runs as a pre-commit hook and as a required CI check on every PR; full-history scan added to the deploy pipeline. (SEC-030, SEC-150)
- [ ] **S0-02** `cargo audit` + `cargo deny` required checks on every PR; nightly scheduled run on `main`. (SEC-140)
- [ ] **S0-03** `npm audit` (or `pnpm audit`) required check on every PR. (SEC-141)
- [ ] **S0-04** Lockfiles (`Cargo.lock`, `package-lock.json` / `pnpm-lock.yaml`) committed; no `*` / `latest` ranges in manifests. (SEC-140, SEC-141)
- [ ] **S0-05** All GitHub Actions pinned to commit SHA, with CODEOWNERS covering `.github/workflows/`; `GITHUB_TOKEN` `permissions:` block defaults to minimum per job. (SEC-144)
- [ ] **S0-06** CI secrets housed in GitHub Environments; `production` environment requires manual review on deploy. (SEC-145)
- [ ] **S0-07** CODEOWNERS covers `/rules/**`, `/migrations/**`, `.github/workflows/`, and any auth/crypto module; branch protection on `main` enforces a CODEOWNER approving review. (SEC-080, SEC-200)
- [ ] **S0-08** Clippy lint or custom cargo check forbids `std::collections::HashMap` in calculation crates and forbids raw `pool.acquire()` outside `Tx::for_user`. CI fails on violation. (SEC-022, SEC-085)
- [ ] **S0-09** `orbit_log::event!` (or equivalent) wrapper in place, with a `Display`/`Debug`-deny attribute on Money / Grant / Scenario / Calculation / SellNowInput / Export; fixture compile-fail test confirms. (SEC-050)
- [ ] **S0-10** CycloneDX SBOM generation step in the release workflow. (SEC-142)

## Infrastructure and hosting

- [ ] **S0-11** VMs provisioned in Hetzner Falkenstein (FSN1); Hetzner at-rest volume encryption enabled on both VMs' data volumes. (SEC-063, ADR-002)
- [ ] **S0-12** Caddy TLS config sets `protocols tls1.3` and rejects TLS 1.2; Let's Encrypt certificate issued and auto-renewing. (SEC-060)
- [ ] **S0-13** Response headers present on the SPA HTML shell: CSP, HSTS (no `preload` yet), X-Content-Type-Options, X-Frame-Options, Referrer-Policy, Permissions-Policy, COOP. Verified via `curl -I`. (SEC-180–186)
- [ ] **S0-14** CORS on the API: explicit origin `https://app.orbit.<tld>`, `Allow-Credentials: true`, whitelisted methods and headers. (SEC-187)
- [ ] **S0-15** Host firewall (nftables) deny-default outbound; explicit allowlist for ECB, Finnhub, Let's Encrypt, Hetzner Object Storage, email provider, billing provider, GitHub deploy. (SEC-149)
- [ ] **S0-16** Postgres bound to the Hetzner private network only; `pg_hba.conf` requires TLS; `orbit_app` role exists without `BYPASSRLS` or superuser. (SEC-021, SEC-066)
- [ ] **S0-17** Postgres RLS **enabled by default** for every user-scoped table created in slice 0 (sessions, users stub); policy using `current_setting('app.user_id')::uuid`. (SEC-020)
- [ ] **S0-18** Row-level trigger on `rule_sets` rejecting `UPDATE` where `status='active'`, scaffolded in the initial migration (even before the first rule set exists). (SEC-082)
- [ ] **S0-19** Secrets loaded from an OS-level secret file (0600, orbit user) via systemd `LoadCredential=`. Secret file not in Git. (SEC-031)
- [ ] **S0-20** MFA enabled on Ivan's GitHub, Hetzner Cloud, domain registrar, DNS host, Bunny.net, email provider, and billing provider accounts. (SEC-205)

## Application and data-handling baseline

- [ ] **S0-21** Password hashing via argon2id at OWASP-2024 params; unit test pins parameters. (SEC-001)
- [ ] **S0-22** Session cookie factory produces `HttpOnly; Secure; SameSite=Lax` in all code paths; CSRF double-submit token scaffolded. (SEC-006, SEC-188, SEC-189)
- [ ] **S0-23** `Tx::for_user(user_id)` helper is the only query-handle acquisition API exposed to the handler layer. (SEC-022)
- [ ] **S0-24** Append-only `audit_log` table migrated; `orbit_app` has `INSERT` but not `UPDATE`/`DELETE`. Retention worker scaffolded (empty schedule is fine in slice 0). (SEC-100, SEC-102)
- [ ] **S0-25** IP-hash salt generated (32 bytes CSPRNG), stored in the secret file; HMAC-SHA256 helper used in the audit-log write path. (SEC-054)

## Backup, recovery, observability

- [ ] **S0-26** Nightly `pg_basebackup` + WAL archive job configured to Hetzner Storage Box; backup bundles encrypted with `age` using a key in the secret file. (SEC-065)
- [ ] **S0-27** At least one end-to-end restore drill executed and dated in the runbook before slice 0 is signed off. (SEC-204, ADR-002)
- [ ] **S0-28** Uptime monitor (Better Stack or equivalent) pings `GET /healthz`; alert route tested end-to-end (page Ivan). (ADR-002)

## Governance and legal

- [ ] **S0-29** Privacy policy + sub-processor register drafted, reviewed by legal, and published at `/privacy` and `/privacy/subprocessors` (can be placeholder-light until features exist, but structure in place). (SEC-120, SEC-121)
- [ ] **S0-30** Incident-response runbook drafted with AEPD 72-hour timer and ES/EN templates; initial drill scheduled before paid launch. (SEC-203)

---

**Sign-off**: checking all 30 items promotes slice 0 to "security-envelope complete." Slice 1 (first user-visible feature: grant CRUD, US-001) may begin only after sign-off. Every subsequent slice must not regress any item on this list; if it does, the regression is a Blocker per `security-requirements.md`.

*End of slice-0 checklist.*
