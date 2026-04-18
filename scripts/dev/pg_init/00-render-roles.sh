#!/usr/bin/env bash
# Orbit local Postgres — create app roles on first boot.
#
# Runs on first boot only (when PGDATA is empty), as part of the official
# postgres image's /docker-entrypoint-initdb.d/ sequence, after initdb and
# before the final restart.
#
# The three role passwords are handed to psql via -v flags; psql's
# `:'var'` substitution (standard since Postgres 8.4) expands them inline
# with proper single-quote escaping. No rendered SQL ever touches disk.
# This avoids depending on `envsubst` / `gettext-base`, which is NOT
# shipped in the postgres:16-bookworm image.
#
# Required env vars (validated upstream by docker-compose.yaml `?` syntax):
#   POSTGRES_ORBIT_MIGRATE_PASSWORD
#   POSTGRES_ORBIT_APP_PASSWORD
#   POSTGRES_ORBIT_SUPPORT_PASSWORD
#
# Exit non-zero on any failure; the image treats that as a fatal init error.
set -euo pipefail

: "${POSTGRES_ORBIT_MIGRATE_PASSWORD:?must be set}"
: "${POSTGRES_ORBIT_APP_PASSWORD:?must be set}"
: "${POSTGRES_ORBIT_SUPPORT_PASSWORD:?must be set}"

# Source SQL — mounted outside /docker-entrypoint-initdb.d/ specifically so
# the postgres image does NOT try to psql it directly with the `:'var'`
# placeholders unbound. See docker-compose.yaml volumes block.
ROLES_SQL="/opt/orbit/init/01-roles.sql"
if [[ ! -r "${ROLES_SQL}" ]]; then
  echo "[orbit-init] ERROR: ${ROLES_SQL} is not readable. Check compose volume mount."
  exit 1
fi

echo "[orbit-init] Creating orbit_migrate, orbit_app, orbit_support roles..."

# ON_ERROR_STOP aborts the init on first failed statement; failure
# propagates up and aborts container startup (the image treats an init
# script exit != 0 as fatal).
psql --username "${POSTGRES_USER:-postgres}" \
     --dbname "${POSTGRES_DB:-orbit}" \
     --no-psqlrc \
     --set ON_ERROR_STOP=1 \
     --quiet \
     -v orbit_migrate_password="${POSTGRES_ORBIT_MIGRATE_PASSWORD}" \
     -v orbit_app_password="${POSTGRES_ORBIT_APP_PASSWORD}" \
     -v orbit_support_password="${POSTGRES_ORBIT_SUPPORT_PASSWORD}" \
     -f "${ROLES_SQL}"

echo "[orbit-init] Roles created."
