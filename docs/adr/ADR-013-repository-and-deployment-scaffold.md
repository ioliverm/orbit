# ADR-013: Repository and deployment scaffold (Slice 0 blueprint)

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)
- **Traces to:** ADR-001 (Rust + React + single binary), ADR-002 (Hetzner, Caddy, systemd), ADR-009 (frontend stack), ADR-011 (auth tables), ADR-012 (crate list), spec §7.8 / §7.9, SEC-030..SEC-035, SEC-050, SEC-060..SEC-066, SEC-100..SEC-103, SEC-140..SEC-150, SEC-180..SEC-190, SEC-203..SEC-205, `security-checklist-slice-0.md` S0-01..S0-30, `v1-slice-plan.md` Slice 0.

## Context

Slice 0 is the security envelope and the minimum bootable Orbit. Its scope is enumerated in `v1-slice-plan.md` and `security-checklist-slice-0.md` — 30 checklist items covering repo hygiene, CI, infrastructure, headers, auth primitives, audit logging, backups, and legal surface. Nothing here is product; everything here is scaffolding whose job is to make Slice 1 a feature change, not a platform change.

Existing ADRs fix the macro choices (Rust + React + single binary + Hetzner + systemd + Caddy). What's unspecified is the **concrete repo tree, the migration tool, the CI YAML shape, the deploy mechanism, the local-dev loop, and the observability sink**. ADR-013 closes these — at the depth Slice 0 needs, not beyond.

## Decision

### Repository layout (single monorepo)

**One Git repository**, Cargo workspace for the backend, `pnpm` workspace for the frontend. No separate backend/frontend repos; the cost of a coordinated change is lower in one repo at this scale.

```
orbit/
├── .github/
│   ├── CODEOWNERS
│   └── workflows/
│       ├── ci.yaml                 # PR + main: build, lint, test, audit, sbom
│       ├── deploy.yaml             # main-only: build & deploy to prod (manual approval)
│       ├── nightly-audit.yaml      # cron: cargo-audit, trivy, dep review
│       └── pre-merge-gitleaks.yaml # required check
├── docs/                           # specs, ADRs, design, security, architecture
├── rules/                          # rule-set YAML (ADR-012); empty at Slice 0
│   └── es/
├── migrations/                     # sqlx migrations (plain .sql, timestamp-prefixed)
│   └── 20260418120000_init.sql
├── backend/
│   ├── Cargo.toml                  # workspace root
│   ├── Cargo.lock
│   ├── rust-toolchain.toml         # pins stable-1.82 at Slice-0 authoring; bumped in-PR
│   ├── deny.toml                   # cargo-deny config
│   ├── api/
│   │   └── openapi.yaml            # generated + committed (ADR-010)
│   ├── crates/
│   │   ├── orbit-core/
│   │   ├── orbit-crypto/
│   │   ├── orbit-db/
│   │   ├── orbit-log/              # SEC-050 macro lives here
│   │   ├── orbit-auth/             # ADR-011
│   │   ├── orbit-api/              # axum handlers, extractors, error envelope
│   │   ├── orbit-worker/           # tokio-cron-scheduler jobs
│   │   ├── orbit-tax-core/         # ADR-012: stub types in Slice 0
│   │   ├── orbit-tax-rules/        # ADR-012: canonicalize+hash in Slice 0
│   │   ├── orbit-tax-spain/        # empty lib.rs in Slice 0
│   │   ├── orbit-market-data/      # empty in Slice 0
│   │   ├── orbit-fx/               # empty in Slice 0
│   │   └── orbit-export/           # empty in Slice 0
│   ├── binaries/
│   │   └── orbit/                  # the single binary (api / worker / migrate / rules subcommands)
│   └── xtask/                      # build-script-ish helper commands (openapi-dump, rules:canonicalize, rules:hash)
├── frontend/
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── lingui.config.ts
│   ├── src/
│   │   ├── main.tsx
│   │   ├── routes/                 # React Router data-router routes
│   │   ├── api/
│   │   │   ├── fetch.ts
│   │   │   └── generated.ts        # from openapi-typescript; committed
│   │   ├── components/
│   │   ├── styles/
│   │   │   ├── tokens.css          # copied from docs/design
│   │   │   └── primitives.css      # copied from docs/design
│   │   ├── locales/
│   │   │   ├── es-ES/messages.po
│   │   │   └── en/messages.po
│   │   └── testing/
│   └── e2e/                        # Playwright specs
├── infra/
│   ├── caddy/Caddyfile
│   ├── systemd/
│   │   ├── orbit-api.service
│   │   └── orbit-worker.service
│   ├── nftables/orbit.nft          # outbound allowlist (SEC-149)
│   └── runbooks/
│       ├── restore-postgres.md     # tested, dated (SEC-204)
│       ├── incident-response.md    # 72h AEPD (SEC-203)
│       └── deploy.md
└── scripts/
    ├── dev/
    │   ├── docker-compose.yaml     # Postgres 16 for local dev
    │   └── seed.sh
    └── ci/
        └── release.sh
```

### Migration tool: `sqlx` (plain-SQL migrations)

**`sqlx-cli` with plain `.sql` files, not Rust migrations.** `refinery` was the main alternative; `sea-orm-migrate` was rejected because we're not using SeaORM.

Rationale for plain SQL:

- RLS policies, triggers, and the `rule_sets` immutability trigger (SEC-082) are SQL constructs; writing them in SQL is clearer than in a Rust DSL.
- Reviewers who read a PR are reviewing the exact DDL that will run.
- `sqlx-cli migrate run` is a one-liner inside the `orbit migrate` subcommand.
- Rollbacks are handwritten if ever needed; forward-only by default.

The first migration (`20260418120000_init.sql`) ships in Slice 0 and contains: `users` (auth subset only — grants come Slice 1), `sessions`, `email_verifications`, `password_reset_tokens`, `audit_log`, `dsr_requests` (stub schema), `rate_limit_buckets`, `rule_sets` (empty, with the update-rejection trigger per SEC-082). Full DDL lives in ADR-014.

### Single binary with subcommands

`backend/binaries/orbit/src/main.rs`:

```rust
enum Cmd {
    Api,            // starts axum on $PORT
    Worker,         // starts tokio-cron-scheduler + long-lived worker tasks
    Migrate,        // runs sqlx migrations to latest
    Rules { sub: RulesSub }, // ingest, promote, list, diff (ADR-012)
    Version,
}
```

Dispatched by the first CLI arg. The same binary image runs in three systemd units:

| Unit | Command | Host |
|---|---|---|
| `orbit-migrate.service` | `orbit migrate` | VM-1, one-shot before `orbit-api.service` |
| `orbit-api.service` | `orbit api` | VM-1, `Restart=on-failure` |
| `orbit-worker.service` | `orbit worker` | VM-1, `Restart=on-failure`, `CPUQuota=40%` (ADR-002) |

### Deployment mechanism: GitHub Actions → SSH + systemd

**No Kamal, no Nomad, no Kubernetes, no Docker Swarm.** The deploy is a built Rust binary + the frontend's static `dist/` directory + a `systemctl restart`. Boring and within ADR-002's €-budget and operational-simplicity posture.

Concrete flow:

1. `main` push → GitHub Actions `deploy.yaml` job (requires manual approval in `production` environment per S0-06).
2. **Build job** (runs on `ubuntu-latest`):
   - `cargo build --release -p orbit` with `cargo-chef` cache layers.
   - `pnpm install --frozen-lockfile && pnpm build` in `frontend/`.
   - `cargo xtask openapi-dump` → verify no drift; build fails if `backend/api/openapi.yaml` is out of sync.
   - Trivy scans release artifacts.
   - CycloneDX SBOM generated (`cargo cyclonedx` + `cdxgen` for npm) and uploaded as an artifact (SEC-142).
   - **Artifact:** a `release.tar.gz` containing `orbit` binary + `frontend-dist/` + migrations/ + rules/.
3. **Deploy job** (requires `production` environment reviewer — Ivan — per S0-06):
   - Downloads `release.tar.gz`.
   - `scp` to VM-1 under a release path `/opt/orbit/releases/<git-sha>/`.
   - `ssh 'sudo -u orbit /opt/orbit/releases/<sha>/orbit migrate'`.
   - `sudo ln -sfn /opt/orbit/releases/<sha> /opt/orbit/current` (atomic symlink swap).
   - `sudo systemctl restart orbit-api.service orbit-worker.service`.
   - `curl -f https://app.orbit.<tld>/api/v1/healthz` — rollback via `ln -sfn` to prior release on failure.
   - Uploads frontend `dist/` to Bunny.net origin bucket (purges edge cache for `/index.html`).
4. Retains the last 5 release directories on VM-1 for quick rollback; older pruned by a cron.

Blue/green is not used at Slice 0. Rationale (ADR-002): short restart window is acceptable; user-visible impact is low at v1 validation traffic. Revisit at paid launch.

**Why not Kamal:** Kamal wraps this exact pattern with slightly better ergonomics. Adopting it is a reversible decision; Slice 0 goes without because the "shell script with `ssh`" version is transparent in CI logs and owns no extra runtime dependency. Revisit at Slice 3 if the deploy script grows above ~100 lines.

**Why not Docker on prod VMs:** we're running one binary. Docker layers add build cost and a runtime surface. The Rust binary is statically linked where feasible (`--target x86_64-unknown-linux-gnu` with musl for the truly static alternative; glibc is fine at ADR-002 scale). If a dependency forces dynamic linkage, we ship against Ubuntu LTS libc on the target VM (stable ABI). If this becomes painful, containerization is reversible.

### CI pipeline stages (in order)

Pulled from `security-checklist-slice-0.md`. A single `ci.yaml` workflow with these jobs, most in parallel; the job graph serializes only where dependencies require.

```yaml
# .github/workflows/ci.yaml — structural sketch
name: ci
on: [pull_request, push]
permissions: { contents: read }
concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }

jobs:
  gitleaks:                 # S0-01, SEC-150
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<SHA>
        with: { fetch-depth: 0 }
      - uses: gitleaks/gitleaks-action@<SHA>

  rust-lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<SHA>
      - uses: dtolnay/rust-toolchain@<SHA>
        with: { toolchain: stable, components: clippy, rustfmt }
      - uses: Swatinem/rust-cache@<SHA>
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo check --all-targets

  rust-test:
    needs: [rust-lint]
    services:
      postgres:
        image: postgres:16@sha256:<DIGEST>
        env: { POSTGRES_PASSWORD: test }
        options: --health-cmd pg_isready --health-interval 5s
    steps:
      - uses: actions/checkout@<SHA>
      - uses: dtolnay/rust-toolchain@<SHA>
      - uses: Swatinem/rust-cache@<SHA>
      - run: cargo sqlx migrate run --source ./migrations
      - run: cargo test --all --locked

  rust-audit:               # S0-02, SEC-140
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<SHA>
      - uses: EmbarkStudios/cargo-deny-action@<SHA>
      - run: cargo install cargo-audit --locked && cargo audit

  frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<SHA>
      - uses: pnpm/action-setup@<SHA>
      - uses: actions/setup-node@<SHA>
        with: { cache: pnpm, cache-dependency-path: frontend/pnpm-lock.yaml }
      - run: cd frontend && pnpm install --frozen-lockfile
      - run: cd frontend && pnpm audit --audit-level=high  # S0-03
      - run: cd frontend && pnpm run typecheck
      - run: cd frontend && pnpm run lint
      - run: cd frontend && pnpm run test
      - run: cd frontend && pnpm run build
      - run: cd frontend && pnpm run lingui:check          # catalog completeness (AC G-11)

  e2e-a11y:
    needs: [rust-test, frontend]
    steps:
      - uses: actions/checkout@<SHA>
      - run: ./scripts/ci/run-preview.sh     # boots orbit api + serves frontend/dist on ephemeral port
      - run: cd frontend && pnpm run e2e     # playwright + @axe-core/playwright smoke

  openapi-drift:            # ADR-010
    needs: [rust-lint]
    steps:
      - run: cargo xtask openapi-dump
      - run: git diff --exit-code backend/api/openapi.yaml

  sbom:                     # S0-10, SEC-142
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - run: cargo cyclonedx --format json
      - run: cdxgen -o sbom.frontend.cdx.json ./frontend
      - uses: actions/upload-artifact@<SHA>
        with: { name: sbom, path: '*.cdx.json' }

  container-scan:           # SEC-146 — only applicable if we add containers; kept off in Slice 0
    if: false
```

All Actions referenced are pinned to commit SHAs per SEC-144. CODEOWNERS covers `.github/workflows/` per S0-05 / S0-07. CI secrets (deploy SSH key, Postmark token) live in the `production` environment only (S0-06).

### Local dev ergonomics

**`docker-compose.yaml`** for Postgres only. Everything else runs natively.

```yaml
# scripts/dev/docker-compose.yaml
services:
  postgres:
    image: postgres:16@sha256:<DIGEST>
    ports: ["5432:5432"]
    environment:
      POSTGRES_USER: orbit
      POSTGRES_PASSWORD: orbit
      POSTGRES_DB: orbit
    volumes: [postgres-data:/var/lib/postgresql/data]
volumes: { postgres-data: {} }
```

Workflow:

1. `docker compose -f scripts/dev/docker-compose.yaml up -d` — starts Postgres.
2. `cargo run -p orbit -- migrate` — applies migrations.
3. `cargo run -p orbit -- api` — boots the API on `:8080` (reads `ORBIT__` env vars).
4. In another terminal: `cd frontend && pnpm dev` — Vite dev server on `:5173` with `/api/*` proxied to `:8080`.
5. `./scripts/dev/seed.sh` creates a test user with a known password for local usage.

No devcontainer in Slice 0. A devcontainer is a follow-up if a second engineer joins.

### Observability baseline

Three sinks. All cheap, all EU-hosted.

1. **Structured logs** → Better Stack (or Scaleway Logs) via a TCP syslog sink from `tracing-subscriber`.
   - `tracing-subscriber` with the JSON formatter and `EnvFilter=info`.
   - The `orbit_log::event!` macro (SEC-050) is the only call site; it compiles the allowlist at the field-type level via a proc macro. Non-allowlisted types fail to compile.
   - Every log line carries `request_id`, `route`, `method`, `status`, `latency_ms`, `db_tx_count`, and (where applicable) `user_id`, `traceability_id` (SEC-055).
   - Retention: 30 days hot; no cold tier in Slice 0.
2. **Metrics** → Prometheus-format `/metrics` endpoint on a **localhost-bound** port (not public), scraped by a simple nightly `curl → GitHub-Actions-issue` watchdog in Slice 0. A real Prometheus instance is a v1.1 concern.
3. **Uptime** → Better Stack uptime monitor pings `GET /healthz` every 60 s; `/readyz` every 5 min. Pages Ivan via email + Telegram webhook (S0-28).

Audit log (SEC-100..SEC-103) is distinct from application logs: it lives in the `audit_log` Postgres table, is append-only, and has a separate 6-year retention policy implemented by the retention worker. Audit entries are not shipped off-VM by Slice 0 — the DB is the system of record.

### Secrets

Per SEC-030..SEC-035. Concrete Slice-0 list of required secrets:

| Secret | Location | Rotation |
|---|---|---|
| `DATABASE_URL` (orbit_app role) | systemd `LoadCredential=` from `/etc/orbit/secrets/env` (mode 0600 orbit:orbit) | annual |
| `JWT_SIGNING_KEY_V1` (reserved for future; not used in Slice 0 since sessions are opaque) | same | annual |
| `SESSION_ID_HMAC_SALT` | same | never (salt is permanent) |
| `IP_HASH_SALT` | same | annual (old salt 90 d overlap; SEC-054) |
| `TOTP_SEED_ENC_KEY` (chacha20poly1305 32B) | same | annual, with a key-ID scheme |
| `BACKUP_ENC_KEY` (age) | same, **not on the Storage Box** | annual |
| `POSTMARK_TOKEN` | same | annual |
| `STRIPE_SECRET_KEY` (Slice 3) | same | on rotation |
| `FINNHUB_API_KEY` (Slice 5) | same | annual |
| `HIBP_NO_KEY_NEEDED` | — | n/a (k-anonymity) |
| Cloudflare/DNS/Hetzner API tokens | **not on the VM**; CI-only | — |

Secret loading:

```rust
// binaries/orbit/src/config.rs
#[derive(Deserialize)]
struct Config {
    database_url: Secret<String>,
    session_hmac_salt: Secret<Bytes<32>>,
    ip_hash_salt: Secret<Bytes<32>>,
    totp_seed_enc_key: Secret<Bytes<32>>,
    backup_enc_key: Secret<Bytes<32>>,
    postmark_token: Secret<String>,
    // ...
}
```

`Secret<T>` is a newtype whose `Debug`/`Display` impl prints `[REDACTED]`. Deserializes from env (`ORBIT__DATABASE_URL`) or from the loaded credential file. systemd `LoadCredential=env:/etc/orbit/secrets/env` is the production path.

### Dependency hygiene

- `Cargo.lock` committed. `pnpm-lock.yaml` committed.
- `deny.toml` forbids: known-malicious advisories (RUSTSEC), unmaintained crates, duplicate dependencies above a patch-level, and disallowed licenses (copyleft except MPL-2.0; no unknown licenses).
- `package.json` pins exact versions for security-critical libs (`@lingui/*`, `react`, `react-router-dom`); others are within caret ranges with Renovate managing PRs weekly (SEC-143).
- No `*` ranges anywhere (S0-04).
- Docker base images: not used in Slice 0; if introduced, `@sha256:...` digest pin + Trivy (SEC-146).

### Backup and restore

Per ADR-002 + SEC-065.

- `orbit-worker` runs `pg_basebackup` nightly at 03:00 Europe/Madrid, pipes through `age --encrypt --recipient <pubkey>` to the Hetzner Storage Box via `rclone`.
- WAL archiving (`archive_command = 'rclone copyto --config ... ...'`) to the same box, with `age` encryption.
- **Restore runbook** (`infra/runbooks/restore-postgres.md`) is a step-by-step that an engineer can follow cold. Drilled once before Slice 0 sign-off (S0-27).
- Keys: `BACKUP_ENC_KEY` sits on VM-1 in the secrets file, **not on the Storage Box**, per SEC-065. Restore requires (a) access to VM-1's secret file (or the operator's offline copy), (b) access to the Storage Box (separate credentials).

### Governance artifacts at Slice 0

- `CODEOWNERS` with the `/rules/**`, `/migrations/**`, `.github/workflows/**`, `backend/crates/orbit-auth/**`, `backend/crates/orbit-crypto/**` paths requiring Ivan's review (S0-07).
- `docs/privacy.md` + `docs/privacy/subprocessors.md` drafts (S0-29).
- `infra/runbooks/incident-response.md` with AEPD 72h timer (S0-30).

## Alternatives considered

- **Monorepo vs polyrepo.** Monorepo. A polyrepo split (backend vs frontend) costs more for cross-cutting changes than it saves in per-repo clarity at one-developer scale.
- **Cargo workspace vs per-crate repos.** Workspace. Crate boundary is still there; the repo boundary is cheap to collapse.
- **`sqlx` vs `refinery` vs `sea-orm`.** `sqlx` for the reasons above. `refinery` is fine; the tie-breaker is that `sqlx` query macros are already how we want to write DB code (compile-time SQL checking against a connected Postgres in dev).
- **Kamal for deploy.** Reversible to add later; Slice 0 goes with scp+systemd for transparency.
- **Docker containers in prod.** Rejected at this scale. Reversible.
- **Kubernetes.** ADR-002 already rejected; reaffirmed here.
- **Hetzner managed Postgres.** If it exists at launch at competitive pricing (ADR-002 follow-up), swap to it — ~3 days of work, pure gain. Not in Slice 0 by default.
- **devcontainer / Nix.** Deferred. `docker-compose for Postgres + cargo + pnpm` is a small enough local surface to ask developers to install natively.
- **Separate Prometheus / Grafana stack.** Over-engineered for Slice 0; `/metrics` endpoint + Better Stack uptime is enough.
- **Observability via OpenTelemetry (tempo/jaeger/etc.).** Defer to the slice where sampled tracing pays off (Slice 4+).
- **Different CDN (Cloudflare, Fastly).** Bunny.net locked by ADR-002; no reason to revisit.

## Consequences

**Positive:**
- One repo, one workspace, one binary, one deploy command. Cognitive load stays small.
- Every Slice-0 checklist item maps onto a concrete file or command. The implementation engineer has a check-off list, not a treasure hunt.
- CI is opinionated and a required-check wall; regressions on lint/audit/drift fail the merge, not the deploy.
- Local dev is `docker-compose up + cargo run + pnpm dev`, no surprises.
- Restore drill is a first-class artifact; a real one runs before Slice-0 sign-off.

**Negative / risks:**
- `scp + systemctl restart` deploy has a short restart window. Acceptable at 99.5% SLO (§7.8); becomes unacceptable at paid-launch scale — revisit at Slice 5/6.
- Single-VM topology means a Hetzner FSN1 incident takes Orbit down. ADR-002 accepts this.
- Worker on the same host as the API means a runaway job could degrade the API. `CPUQuota=40%` on the worker systemd unit is the hard ceiling; a memory cgroup is added if a memory issue ever materializes.
- `pnpm audit` false-positive noise is real; the policy is "high-severity fails CI; medium/low reviewed weekly via Renovate grooming." Documented in `docs/runbooks/` at Slice 0.
- `sqlx-cli` requires a live Postgres to verify query macros at compile time; Slice 0 adds a `scripts/dev/` helper plus a CI job that boots Postgres for the `rust-test` stage.

**Tension with prior ADRs:**
- None. ADR-001 and ADR-002 are the floor; this ADR is the bill of materials.

**Follow-ups:**
- Implementation engineer: scaffold every path above, one PR per major slice (repo skeleton PR, CI PR, migrations PR, auth-primitives PR). Each PR individually small; no big-bang scaffold.
- Implementation engineer: verify the restore drill before submitting S0-27 as done (SEC-204).
- Slice-3 follow-up: revisit scp+systemd deploy if release script size exceeds 100 lines, or if deploys start taking over a minute.
- Slice-5 follow-up: add outbound-allowlist entries for Finnhub (nftables rule).
- Security-engineer: confirm the three observability sinks (Better Stack, Scaleway Logs option, uptime) are EU-residency compliant or wrapped in SCCs + TIA per SEC-121.
