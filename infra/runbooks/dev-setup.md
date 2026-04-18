# Local development setup — Slice 0a

Scope: stand up a first-time local Orbit dev environment on macOS or Linux,
end-to-end, in ~15 minutes. This runbook covers the 0a (local-green) shape
per [ADR-015](../../docs/adr/ADR-015-slice-0-local-first-scope-split.md). The
deploy (0b) path is in `deploy.md`.

> **Who this is for.** A single developer with sudo on their machine, access
> to this repo, and nothing else pre-installed beyond git. If you already
> have `rustup`, `nvm`, `pnpm`, `docker`, and `just`, jump to
> [§2 Bootstrap the stack](#2-bootstrap-the-stack).

## 1. Install prerequisites

### 1.1 Rust 1.82

`rust-toolchain.toml` at the repo root pins `1.82`; `rustup` picks it up
automatically when you `cd` into the repo. Install `rustup` once:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Restart shell; then verify:
rustup show
```

The first `cargo` invocation inside the repo will download the pinned
toolchain. This is expected.

### 1.2 Node 20 via `nvm`

`.nvmrc` at the repo root pins `20`. Install `nvm`:

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/master/install.sh | bash
# Restart shell.
cd <orbit repo>
nvm install    # reads .nvmrc
nvm use        # activates 20 in this shell
```

Make `nvm use` automatic on `cd` via your shell config (see the `nvm`
README — not orbit-specific).

### 1.3 pnpm via corepack

pnpm ships as a Node corepack shim; no separate install needed beyond
enabling corepack:

```bash
corepack enable
corepack prepare pnpm@latest --activate
pnpm --version
```

### 1.4 Docker

Install Docker Desktop (macOS) or the Docker Engine + Compose plugin
(Linux). Minimum versions: Docker 24+, Compose v2+.

- macOS:   <https://docs.docker.com/desktop/install/mac-install/>
- Linux:   `sudo apt install docker.io docker-compose-plugin` (or your
  distro's equivalent). Add your user to the `docker` group and re-login.

Verify:

```bash
docker --version
docker compose version
```

### 1.5 `just`

```bash
# macOS
brew install just

# Linux (via cargo)
cargo install just

# Or via apt (>= 1.14 required):
sudo apt install just
```

Verify:

```bash
just --version
```

### 1.6 Optional: `sqlx-cli` (recommended)

Once T6 has landed the first migration, `just migrate` uses `sqlx migrate run`.
Install ahead of time so your first run doesn't fall back to the raw-`psql`
path:

```bash
cargo install sqlx-cli --no-default-features --features postgres
```

## 2. Bootstrap the stack

From the repo root:

```bash
# 1. Copy and lock down the env file.
cp .env.example .env
chmod 600 .env                 # required by S0-19 local flavor; do not skip

# 2. Generate strong passwords and paste them into .env.
for var in POSTGRES_PASSWORD \
           POSTGRES_ORBIT_MIGRATE_PASSWORD \
           POSTGRES_ORBIT_APP_PASSWORD \
           POSTGRES_ORBIT_SUPPORT_PASSWORD; do
  printf '%s=%s\n' "$var" "$(openssl rand -base64 32 | tr -d '\n')"
done
# Paste the four values into .env, and update the `CHANGE_ME_*` placeholders
# inside DATABASE_URL / DATABASE_URL_MIGRATE to match the generated
# _APP_ / _MIGRATE_ passwords.
#
# (A follow-up ticket will automate this. For 0a, manual paste is fine.)

# 3. Mark the dev scripts executable (one-time; git preserves the bit, but
# a fresh clone on Windows-bridged filesystems can lose it).
chmod +x scripts/dev/pg_init/00-render-roles.sh \
         scripts/dev/pg_init/02-tls.sh \
         scripts/dev/seed.sh

# 4. Boot Postgres.
just db-up

# 5. Copy the self-signed cert out so host clients can verify-full against it.
just db-cert

# 6. Apply migrations (no-op if T6 has not landed yet — that's expected).
just migrate

# 7. Start the app (see `just dev` for the in-repo two-terminal dance).
just dev
```

## 3. Verify the TLS posture

These checks back the 0a flavor of S0-16 (TLS-required `pg_hba`). They
assume `just db-up` ran cleanly and `scripts/dev/.server.crt` exists.

### 3.1 TLS is required

```bash
# Expected: REFUSED — the server rejects non-TLS connections per pg_hba.conf.
PGPASSWORD="$POSTGRES_ORBIT_APP_PASSWORD" \
  psql "host=127.0.0.1 port=5432 user=orbit_app dbname=orbit sslmode=disable" \
  -c 'select 1'
# Look for: "no pg_hba.conf entry for host ... no encryption"
```

### 3.2 TLS with full verification works

```bash
# Expected: connects, prints `?column? = 1`.
PGPASSWORD="$POSTGRES_ORBIT_APP_PASSWORD" \
  psql "host=127.0.0.1 port=5432 user=orbit_app dbname=orbit \
        sslmode=verify-full sslrootcert=scripts/dev/.server.crt" \
  -c 'select 1'
```

### 3.3 Roles exist with the right posture

```bash
# Expected: three rows, all with (f,f,f,f,f) for super/bypassrls/createdb/
# createrole/replication.
just db-shell -c "select rolname, rolsuper, rolbypassrls, rolcreatedb, rolcreaterole, rolreplication from pg_roles where rolname in ('orbit_migrate','orbit_app','orbit_support') order by rolname;"
```

## 4. Troubleshooting

### `just db-up` hangs at "Waiting for Postgres to report healthy"

- Check logs: `docker logs orbit-postgres-dev`.
- Most common cause: one of `POSTGRES_*_PASSWORD` is unset — the
  compose file will stop the container rather than start with a blank
  password. Confirm with `cat .env` (but do NOT paste its contents into
  any chat or ticket).

### `psql` says `SSL error: certificate verify failed`

- Run `just db-cert` again; the cert was probably not copied.
- If you wiped the volume (`just db-reset`), the cert changed on next boot
  — re-run `just db-cert`.

### "password authentication failed for user orbit_app"

- The password in `DATABASE_URL` doesn't match `POSTGRES_ORBIT_APP_PASSWORD`.
  They must match exactly.
- After changing passwords in `.env`, run `just db-reset && just db-up` —
  password changes only take effect on first-init.

### "role orbit_migrate already exists — refusing to re-init"

- Someone tampered with PGDATA while the init scripts were still running.
  Run `just db-reset && just db-up` to restart clean.

### Docker not available

- 0a requires docker. If you can't install it, raise with the operator —
  the workaround (local Postgres install + manual TLS config) is not
  supported by these scripts.

## 5. Deferred to 0b

These items are deliberately NOT part of local setup and will land in the
0b deploy runbook:

- Private-network binding (S0-16 deploy half).
- systemd `LoadCredential=` secret loading (S0-19 deploy half).
- Let's Encrypt TLS on the public edge (S0-12).
- nftables outbound deny-default (S0-15).
- Offsite encrypted backups + restore drill (S0-26, S0-27).

See [ADR-015](../../docs/adr/ADR-015-slice-0-local-first-scope-split.md) for
the full 0a/0b split.
