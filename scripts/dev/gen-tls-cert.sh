#!/usr/bin/env bash
# Orbit local Postgres — dev PKI generator.
#
# Generates a minimal two-tier PKI inside $TLS_DIR (default /certs):
#   ca.crt / ca.key           — root CA, handed to clients as sslrootcert
#   server.crt / server.key   — leaf, signed by the CA, served by Postgres
#
# Why two certs instead of one self-signed cert acting as both root and
# leaf: rustls (sqlx's TLS stack) rejects the self-is-root shape even when
# it's technically valid, with "certificate was not trusted". A distinct
# CA → leaf is what rustls + libpq + openssl all accept uniformly.
#
# Idempotent: if all four files already exist, exits without regenerating.
# `just db-reset` (or wiping the orbit-postgres-certs named volume)
# triggers regeneration on the next `just db-up`.

set -euo pipefail

TLS_DIR="${TLS_DIR:-/certs}"
CA_CRT="${TLS_DIR}/ca.crt"
CA_KEY="${TLS_DIR}/ca.key"
SRV_CRT="${TLS_DIR}/server.crt"
SRV_KEY="${TLS_DIR}/server.key"

mkdir -p "${TLS_DIR}"

if [[ -f "${CA_CRT}" && -f "${CA_KEY}" && -f "${SRV_CRT}" && -f "${SRV_KEY}" ]]; then
  echo "[orbit-tls-init] TLS material already present in ${TLS_DIR} — leaving as-is."
  exit 0
fi

echo "[orbit-tls-init] Generating dev CA + server cert in ${TLS_DIR}..."

# -----------------------------------------------------------------------
# 1. Root CA — CA:TRUE, keyCertSign + cRLSign, no SAN (not a leaf).
# -----------------------------------------------------------------------
openssl req -new -x509 -nodes \
  -newkey rsa:2048 \
  -days 3650 \
  -keyout "${CA_KEY}" \
  -out "${CA_CRT}" \
  -subj "/C=ES/ST=Madrid/L=Madrid/O=Orbit Local Dev CA (NOT FOR PRODUCTION)/CN=Orbit Local Dev Root CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" \
  >/dev/null 2>&1

# -----------------------------------------------------------------------
# 2. Server cert signed by the CA — CA:FALSE, serverAuth, SAN covers
#    localhost + 127.0.0.1 + ::1 + the compose service DNS name.
# -----------------------------------------------------------------------
SAN_CONFIG="$(mktemp)"
cat > "${SAN_CONFIG}" <<'EOF'
[req]
distinguished_name = req_distinguished_name
prompt             = no

[req_distinguished_name]
C  = ES
ST = Madrid
L  = Madrid
O  = Orbit Local Dev (NOT FOR PRODUCTION)
CN = orbit-postgres-dev

[v3_server]
basicConstraints = critical, CA:FALSE
keyUsage         = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName   = @alt_names

[alt_names]
DNS.1 = localhost
DNS.2 = orbit-postgres-dev
DNS.3 = postgres
IP.1  = 127.0.0.1
IP.2  = ::1
EOF

SRV_CSR="$(mktemp)"
openssl req -new -nodes \
  -newkey rsa:2048 \
  -keyout "${SRV_KEY}" \
  -out "${SRV_CSR}" \
  -config "${SAN_CONFIG}" \
  >/dev/null 2>&1

openssl x509 -req -days 3650 \
  -in "${SRV_CSR}" \
  -CA "${CA_CRT}" -CAkey "${CA_KEY}" -CAcreateserial \
  -extfile "${SAN_CONFIG}" \
  -extensions v3_server \
  -out "${SRV_CRT}" \
  >/dev/null 2>&1

rm -f "${SAN_CONFIG}" "${SRV_CSR}" "${TLS_DIR}/ca.srl"

# Keys 0600; certs 0644.
chmod 600 "${CA_KEY}" "${SRV_KEY}"
chmod 644 "${CA_CRT}" "${SRV_CRT}"

# Ownership must match the postgres user inside the postgres container
# (image-provided UID/GID 999). The tls-init service runs as root; chown
# by name works because this script runs inside the postgres:16-bookworm
# image which has the postgres user.
if command -v chown >/dev/null 2>&1; then
  chown postgres:postgres "${CA_KEY}" "${CA_CRT}" "${SRV_KEY}" "${SRV_CRT}" 2>/dev/null || true
fi

echo "[orbit-tls-init] TLS material generated:"
echo "  CA:"
openssl x509 -in "${CA_CRT}" -noout -subject -dates 2>/dev/null || true
echo "  Server:"
openssl x509 -in "${SRV_CRT}" -noout -subject -issuer -dates -ext subjectAltName 2>/dev/null || true
