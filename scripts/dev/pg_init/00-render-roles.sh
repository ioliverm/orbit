#!/usr/bin/env bash
# Orbit local Postgres — render and apply role DDL on first boot.
#
# Runs on first boot only (when PGDATA is empty), as part of the official
# postgres image's /docker-entrypoint-initdb.d/ sequence, after initdb and
# before the final restart that starts TLS.
#
# The postgres image does NOT interpolate ${VAR} inside .sql files. This
# script does the interpolation explicitly via `envsubst` and then pipes the
# rendered SQL into `psql`, never writing rendered passwords to disk.
#
# Required env vars (all validated upstream by docker-compose.yaml `?` syntax):
#   POSTGRES_ORBIT_MIGRATE_PASSWORD
#   POSTGRES_ORBIT_APP_PASSWORD
#   POSTGRES_ORBIT_SUPPORT_PASSWORD
#
# Exit non-zero on any failure; the image treats that as a fatal init error.
set -euo pipefail

echo "[orbit-init] Creating orbit_migrate, orbit_app, orbit_support roles..."

: "${POSTGRES_ORBIT_MIGRATE_PASSWORD:?must be set}"
: "${POSTGRES_ORBIT_APP_PASSWORD:?must be set}"
: "${POSTGRES_ORBIT_SUPPORT_PASSWORD:?must be set}"

# Source SQL — mounted outside /docker-entrypoint-initdb.d/ specifically so
# the postgres image does NOT try to psql it directly with the `${VAR}`
# placeholders still literal. See docker-compose.yaml volumes block.
ROLES_SQL="/opt/orbit/init/01-roles.sql"
if [[ ! -r "${ROLES_SQL}" ]]; then
  echo "[orbit-init] ERROR: ${ROLES_SQL} is not readable. Check compose volume mount."
  exit 1
fi

# `envsubst` (gettext-base) is present in the postgres:16-bookworm image.
# We restrict the var-list so nothing beyond the three expected password
# placeholders is ever substituted — this prevents accidental expansion of
# PostgreSQL `$$`-quoted function bodies or `$foo` identifiers in the SQL.
if ! command -v envsubst >/dev/null 2>&1; then
  echo "[orbit-init] ERROR: envsubst not found. Expected in postgres:16-bookworm. Install gettext-base."
  exit 1
fi

# Pipe-only flow: no rendered file is ever written to disk. `psql
# ON_ERROR_STOP=1` aborts the init on first failed statement, which aborts
# container startup.
envsubst '$POSTGRES_ORBIT_MIGRATE_PASSWORD $POSTGRES_ORBIT_APP_PASSWORD $POSTGRES_ORBIT_SUPPORT_PASSWORD' \
    < "${ROLES_SQL}" \
  | psql --username "${POSTGRES_USER:-postgres}" \
         --dbname "${POSTGRES_DB:-orbit}" \
         --no-psqlrc \
         --set ON_ERROR_STOP=1 \
         --quiet

echo "[orbit-init] Roles created."
