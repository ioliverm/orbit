#!/usr/bin/env bash
# Orbit local Postgres — self-signed TLS cert generator.
#
# Runs in the `tls-init` sidecar container before Postgres starts. Writes
# server.crt + server.key into $TLS_DIR (default /certs) which is a named
# volume mounted read-only by the Postgres service. This sequence avoids
# the chicken-and-egg where the postgres image's initdb phase starts a
# temporary server with `ssl=on` before /docker-entrypoint-initdb.d/*.sh
# has a chance to run — by the time Postgres boots, the cert is already
# present in the cert volume.
#
# Self-signed cert valid for 10 years; SAN covers localhost + 127.0.0.1 +
# ::1 + the compose service DNS name. Appropriate for a loopback-bound
# dev stack (ADR-015 Slice 0a); 0b switches to private-network binding
# and a real CA.
#
# Idempotent: if both files already exist in $TLS_DIR, exits without
# regenerating. `just db-reset` deletes the cert volume, which triggers
# regeneration on the next `just db-up`.

set -euo pipefail

TLS_DIR="${TLS_DIR:-/certs}"
CRT="${TLS_DIR}/server.crt"
KEY="${TLS_DIR}/server.key"

mkdir -p "${TLS_DIR}"

if [[ -f "${CRT}" && -f "${KEY}" ]]; then
  echo "[orbit-tls-init] TLS material already present in ${TLS_DIR} — leaving as-is."
  exit 0
fi

echo "[orbit-tls-init] Generating self-signed TLS cert in ${TLS_DIR}..."

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

# Ownership must match the postgres user inside the postgres container
# (image-provided UID/GID 999). The tls-init service runs as root; chown
# by name works because this script runs inside the postgres:16-bookworm
# image which has the postgres user.
if command -v chown >/dev/null 2>&1; then
  chown postgres:postgres "${KEY}" "${CRT}" 2>/dev/null || true
fi

echo "[orbit-tls-init] TLS cert generated:"
openssl x509 -in "${CRT}" -noout -subject -issuer -dates -ext subjectAltName 2>/dev/null || true
