# Orbit — developer command runner (Slice 0a per ADR-015).
#
# Prereq: `just` (https://just.systems). Install:
#     macOS:  brew install just
#     Linux:  cargo install just   # or: sudo apt install just (>= 1.14)
#
# Conventions:
#   - Every recipe is runnable on macOS + Linux; Windows is out of scope.
#   - Docker is assumed at `db-*` recipes. The rest run natively.
#   - `.env` is loaded implicitly by each recipe that needs it. `.env` is
#     git-ignored and expected at mode 0600 per S0-19 local flavor.

set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load := true
set dotenv-filename := ".env"
set dotenv-required := false

# Default target prints the recipe list.
default:
    @just --list --unsorted

# ---------------------------------------------------------------------------
# Development loop
# ---------------------------------------------------------------------------

# Boot the backend (API + worker combined today) + the frontend dev server.
# Assumes `just db-up && just migrate` has been run at least once.
dev:
    @echo "[dev] Starting backend API and frontend dev server..."
    @echo "[dev] TODO: wire once backend/binaries/orbit compiles + frontend scaffold lands."
    @echo "[dev] For now: run in two terminals:"
    @echo "        Terminal A:  cargo run -p orbit -- api"
    @echo "        Terminal B:  cd frontend && pnpm dev"

# ---------------------------------------------------------------------------
# Database (local Docker Postgres, Slice 0a)
# ---------------------------------------------------------------------------

# Boot the local Postgres container. The `tls-init` sidecar generates the
# self-signed cert into a named volume on first boot, then Postgres starts.
# `--wait` blocks until all services reach their healthy / completed state;
# once that returns we copy the cert out to scripts/dev/.server.crt so host
# tools (sqlx, psql) with sslmode=verify-full can trust it.
db-up:
    docker compose -f scripts/dev/docker-compose.yaml up -d --wait
    @just db-cert
    @echo "[db-up] Postgres is healthy and scripts/dev/.server.crt is in place."

# Tear down the container (keeps the named volume — data persists).
db-down:
    docker compose -f scripts/dev/docker-compose.yaml down

# Open a psql shell as the SUPERUSER (inside the container over the local
# Unix socket — no TLS required for this path; see pg_hba.conf). Use this
# only for ops work. Application-shaped debugging should go through
# `psql $DATABASE_URL`.
#
# Passes any trailing arguments to psql, e.g. `just db-shell -c 'select 1'`.
db-shell *args:
    docker exec -it orbit-postgres-dev psql -U postgres -d orbit {{args}}

# Full reset: tear down + delete the named volume. First boot on the next
# `db-up` regenerates the TLS cert, re-runs role creation, and wipes all
# data. Destructive. Requires confirmation.
db-reset:
    @echo "[db-reset] This will DELETE the Postgres data volume. Ctrl-C to abort."
    @read -p "Type 'yes' to confirm: " ans; [ "$ans" = "yes" ] || exit 1
    docker compose -f scripts/dev/docker-compose.yaml down -v
    @echo "[db-reset] Volume deleted. Run \`just db-up\` to re-init."

# Copy the dev CA cert to scripts/dev/.ca.crt so host tools (psql, sqlx)
# can use sslmode=verify-full against the server cert issued by that CA.
# Git-ignored. (The server's leaf cert stays inside the cert volume.)
db-cert:
    docker cp orbit-postgres-dev:/etc/postgresql/certs/ca.crt scripts/dev/.ca.crt
    @chmod 644 scripts/dev/.ca.crt
    @echo "[db-cert] Wrote scripts/dev/.ca.crt"

# Apply pending migrations via scripts/dev/seed.sh (sqlx-cli preferred;
# psql fallback). T6 owns the actual migration content.
migrate:
    ./scripts/dev/seed.sh

# ---------------------------------------------------------------------------
# Build + quality gates
# ---------------------------------------------------------------------------

# Run the whole test suite — cargo tests + pnpm tests.
test:
    cargo test --locked --all
    cd frontend && pnpm run test

# Fast compile-and-lint check. Use before pushing.
check:
    cargo check --all-targets --locked
    cd frontend && pnpm run typecheck

# Format everything. Writes changes to disk.
fmt:
    cargo fmt --all
    cd frontend && pnpm run format

# Run all linters in check mode. CI runs the same commands.
lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features --locked -- -D warnings
    cd frontend && pnpm run lint

# Install the local pre-commit hook (gitleaks + fast Rust lints). The
# pre-commit script itself lives at scripts/dev/pre-commit.sh (T3 owns it);
# this recipe is the just-flavored entry point.
hooks-install:
    ./scripts/dev/install-hooks.sh
