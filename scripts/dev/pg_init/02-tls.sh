#!/usr/bin/env bash
# Orbit local Postgres — TLS cert generation on first boot.
#
# Context: this script runs as the `postgres` OS user inside the container,
# during the /docker-entrypoint-initdb.d phase. That phase executes after
# initdb has populated PGDATA and a temporary local server has come up on a
# Unix socket, but BEFORE the entrypoint exec's the real `postgres` command
# (which specifies `ssl=on`). By the time Postgres restarts for real, the
# cert and key MUST exist at the paths named in docker-compose.yaml.
#
# We generate a self-signed cert valid for 10 years, covering localhost +
# 127.0.0.1 + ::1 + the service DNS name used inside the compose network.
# A self-signed cert for a loopback-bound local dev stack is appropriate for
# 0a; 0b switches to private-network binding and a real CA.
#
# The cert is generated once (on first boot, when PGDATA is empty) and then
# persists inside the named volume. `just db-reset` wipes the volume, which
# triggers regeneration on the next `just db-up`.

set -euo pipefail

PGDATA="${PGDATA:-/var/lib/postgresql/data}"
CRT="${PGDATA}/server.crt"
KEY="${PGDATA}/server.key"

echo "[orbit-init] Generating self-signed TLS cert at ${CRT}..."

# If for any reason this runs twice (init re-entry), do not overwrite.
if [[ -f "${CRT}" && -f "${KEY}" ]]; then
  echo "[orbit-init] TLS material already present — leaving as-is."
  exit 0
fi

# SAN extension: localhost + 127.0.0.1 + ::1 + container service name.
# Clients connecting from outside the container (`host=127.0.0.1`) will see
# 127.0.0.1 in the SAN list and match it; callers on the compose network
# (none today, but reserved for 0b) get the service name.
SAN_CONFIG="$(mktemp)"
cat > "${SAN_CONFIG}" <<'EOF'
[req]
distinguished_name = req_distinguished_name
x509_extensions    = v3_req
prompt             = no

[req_distinguished_name]
C  = ES
ST = Madrid
L  = Madrid
O  = Orbit Local Dev (NOT FOR PRODUCTION)
CN = orbit-postgres-dev

[v3_req]
basicConstraints = CA:FALSE
keyUsage         = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName   = @alt_names

[alt_names]
DNS.1 = localhost
DNS.2 = orbit-postgres-dev
DNS.3 = postgres
IP.1  = 127.0.0.1
IP.2  = ::1
EOF

# 2048-bit RSA is sufficient for a local dev stack. ECDSA (P-256) would be
# smaller/faster but introduces a client-compat footnote we don't need here.
openssl req \
  -new \
  -x509 \
  -nodes \
  -days 3650 \
  -newkey rsa:2048 \
  -keyout "${KEY}" \
  -out "${CRT}" \
  -config "${SAN_CONFIG}" \
  -extensions v3_req \
  >/dev/null 2>&1

rm -f "${SAN_CONFIG}"

# Postgres refuses to start if server.key is group- or world-readable.
chmod 600 "${KEY}"
chmod 644 "${CRT}"

# The entrypoint already runs as `postgres`, so the files are owned by it,
# but chown defensively in case that invariant ever changes.
if command -v chown >/dev/null 2>&1; then
  chown postgres:postgres "${KEY}" "${CRT}" 2>/dev/null || true
fi

# Also export the cert to a known location inside PGDATA so developers can
# grab it for `sslmode=verify-full` from the host. They can copy it with:
#   docker cp orbit-postgres-dev:/var/lib/postgresql/data/server.crt \
#             ./scripts/dev/.server.crt
# (`.server.crt` is git-ignored via the top-level `.gitignore` rule on
# `scripts/dev/.*.crt`.)

echo "[orbit-init] TLS cert generated:"
openssl x509 -in "${CRT}" -noout -subject -issuer -dates -ext subjectAltName 2>/dev/null || true
