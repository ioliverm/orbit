#!/usr/bin/env bash
# Orbit local Postgres — migrate + seed wrapper (Slice 0a).
#
# Thin wrapper around `sqlx migrate run`. Rationale (ADR-013 §Migration tool):
# plain .sql migrations owned by `orbit_migrate`, run from the repo root with
# DATABASE_URL_MIGRATE pointing at the local Postgres.
#
# T6 is authoring the first migration in parallel. While that PR is in
# flight, `sqlx migrate run` may have nothing to apply (or the migrations
# directory may be empty), which is fine — `sqlx migrate run` exits 0 on a
# clean slate. If `sqlx` is not installed yet, we fall back to raw `psql` so
# T6 can land `.sql` files and smoke-test them without blocking on the
# sqlx-cli dependency.
#
# Usage:
#   just migrate                  # preferred entry point
#   ./scripts/dev/seed.sh         # equivalent
#   ./scripts/dev/seed.sh --dry   # print what would be applied, do nothing
#
# Required env:
#   DATABASE_URL_MIGRATE          # set by `.env` — `postgres://orbit_migrate:...`

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${REPO_ROOT}"

# Load .env if present. The file is 0600-perms by convention (S0-19 local
# flavor); we read it but never echo its contents.
if [[ -f "${REPO_ROOT}/.env" ]]; then
  # shellcheck disable=SC1091
  set -a
  . "${REPO_ROOT}/.env"
  set +a
fi

: "${DATABASE_URL_MIGRATE:?DATABASE_URL_MIGRATE is required — cp .env.example .env and fill it in}"

DRY_RUN=false
if [[ "${1:-}" == "--dry" ]]; then
  DRY_RUN=true
fi

MIGRATIONS_DIR="${REPO_ROOT}/migrations"

# Case 0: migrations directory is absent or empty — T6 has not landed yet.
# Exit cleanly; `just db-up` should still work end-to-end.
if [[ ! -d "${MIGRATIONS_DIR}" ]] \
   || ! compgen -G "${MIGRATIONS_DIR}/*.sql" >/dev/null; then
  echo "[orbit-migrate] No migrations found in ${MIGRATIONS_DIR} — nothing to apply."
  echo "[orbit-migrate] (T6 is authoring the initial migration; re-run after it lands.)"
  exit 0
fi

# Case 1: sqlx-cli present — preferred path.
if command -v sqlx >/dev/null 2>&1; then
  echo "[orbit-migrate] Using sqlx migrate run against migrations/"
  if $DRY_RUN; then
    sqlx migrate info --source "${MIGRATIONS_DIR}" --database-url "${DATABASE_URL_MIGRATE}"
  else
    sqlx migrate run --source "${MIGRATIONS_DIR}" --database-url "${DATABASE_URL_MIGRATE}"
  fi
  exit 0
fi

# Case 2: sqlx-cli absent — fall back to raw psql. This is a temporary path
# for pre-T6-landing smoke tests; it does NOT track applied-migration state
# the way sqlx does, so it is destructive on re-run if migrations are not
# idempotent. Print a loud warning.
echo "[orbit-migrate] WARNING: sqlx-cli not found on PATH."
echo "[orbit-migrate] Falling back to psql -f for each migration in lexical order."
echo "[orbit-migrate] This does NOT track applied migrations — intended only for"
echo "[orbit-migrate] pre-T6 smoke testing. Install with: cargo install sqlx-cli --no-default-features --features postgres"

if ! command -v psql >/dev/null 2>&1; then
  echo "[orbit-migrate] ERROR: neither sqlx nor psql is on PATH. Install one of them."
  exit 1
fi

for f in "${MIGRATIONS_DIR}"/*.sql; do
  echo "[orbit-migrate] psql -f ${f}"
  if $DRY_RUN; then
    continue
  fi
  psql "${DATABASE_URL_MIGRATE}" \
       --no-psqlrc \
       --set ON_ERROR_STOP=1 \
       --single-transaction \
       --file "${f}"
done

echo "[orbit-migrate] Done."
