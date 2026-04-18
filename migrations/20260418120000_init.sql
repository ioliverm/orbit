-- Slice 0a init migration.
--
-- Traces to:
--   - ADR-014 §1 (authoritative DDL for users, sessions, email_verifications,
--     password_reset_tokens, audit_log, rule_sets, rate_limit_buckets, dsr_requests).
--   - ADR-015 §Additional decisions (orbit_support role provisioned here;
--     audit_log grants: orbit_app INSERT only, orbit_support SELECT only,
--     neither UPDATE/DELETE). Supersedes ADR-014 §"Upstream ambiguities
--     resolved unilaterally" item 7.
--   - Security checklist slice-0: S0-16 (app-level portion), S0-17, S0-18, S0-24.
--
-- Scope: Slice 0 tables only. grants, vesting_events, residency_periods ship
-- in 20260425120000_slice_1.sql; do not add them here.

-- ---------------------------------------------------------------------------
-- 1. Extensions
-- ---------------------------------------------------------------------------
CREATE EXTENSION IF NOT EXISTS "pgcrypto";  -- gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS "citext";    -- case-insensitive email

-- ---------------------------------------------------------------------------
-- 2. Roles
--    orbit_migrate: schema owner, runs migrations.
--    orbit_app:     runtime role for the app. No BYPASSRLS, no superuser.
--    orbit_support: read-only operator role. SELECT on audit_log only (S0-24).
--
-- All three are LOGIN roles (the deploy assigns passwords out-of-band;
-- pg_hba.conf enforces TLS per S0-16). The `IF NOT EXISTS` shape is emulated
-- via pg_roles lookup because `CREATE ROLE ... IF NOT EXISTS` does not exist
-- in Postgres.
-- ---------------------------------------------------------------------------
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'orbit_migrate') THEN
    CREATE ROLE orbit_migrate LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
  END IF;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'orbit_app') THEN
    CREATE ROLE orbit_app LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
  END IF;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'orbit_support') THEN
    CREATE ROLE orbit_support LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
  END IF;
END
$$;

-- ---------------------------------------------------------------------------
-- 3. Tables (ADR-014 §1 DDL — copy faithfully)
-- ---------------------------------------------------------------------------

-- USERS ---------------------------------------------------------------------
CREATE TABLE users (
  id                             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  email                          CITEXT NOT NULL UNIQUE,
  password_hash                  TEXT NOT NULL,           -- argon2id PHC string
  password_changed_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
  email_verified_at              TIMESTAMPTZ,
  locale                         TEXT NOT NULL DEFAULT 'es-ES'
                                  CHECK (locale IN ('es-ES','en')),
  primary_currency               TEXT NOT NULL DEFAULT 'EUR'
                                  CHECK (primary_currency IN ('EUR','USD')),
  mfa_enrolled_at                TIMESTAMPTZ,
  mfa_totp_secret_ciphertext     BYTEA,                   -- chacha20poly1305
  mfa_recovery_codes_hashes      TEXT[] NOT NULL DEFAULT '{}',
  mfa_disable_pending_at         TIMESTAMPTZ,
  disclaimer_accepted_at         TIMESTAMPTZ,
  disclaimer_accepted_version    TEXT,                    -- e.g. 'v1-2026-04'
  deleted_at                     TIMESTAMPTZ,             -- 30-day soft-delete (ADR-005)
  created_at                     TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at                     TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- No RLS on users: the row is keyed by id and accessed only by the owning request.
-- Application enforces identity via the session lookup.
-- The CITEXT UNIQUE constraint above already yields a case-insensitive unique
-- index (auto-named `users_email_key`); no explicit lower(email) index needed.

-- SESSIONS ------------------------------------------------------------------
CREATE TABLE sessions (
  id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                 UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  session_id_hash         BYTEA NOT NULL UNIQUE,            -- sha256(cookie value)
  refresh_token_hash      BYTEA NOT NULL UNIQUE,            -- sha256(refresh value)
  family_id               UUID NOT NULL,                    -- rotation family
  ip_hash                 BYTEA NOT NULL,                   -- hmac-sha256 (SEC-054)
  user_agent              TEXT NOT NULL CHECK (length(user_agent) <= 512),
  created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_used_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
  revoked_at              TIMESTAMPTZ,
  revoke_reason           TEXT CHECK (revoke_reason IN (
                            'user_signout','refresh_reuse',
                            'password_change','mfa_change',
                            'refresh_rotation','admin'))
);
CREATE INDEX sessions_user_id_idx   ON sessions (user_id) WHERE revoked_at IS NULL;
CREATE INDEX sessions_family_id_idx ON sessions (family_id);

ALTER TABLE sessions ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON sessions
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- EMAIL VERIFICATIONS -------------------------------------------------------
CREATE TABLE email_verifications (
  id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash    BYTEA NOT NULL UNIQUE,
  expires_at    TIMESTAMPTZ NOT NULL,
  consumed_at   TIMESTAMPTZ,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX email_verifications_user_id_idx ON email_verifications (user_id);

ALTER TABLE email_verifications ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON email_verifications
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- PASSWORD RESET TOKENS -----------------------------------------------------
CREATE TABLE password_reset_tokens (
  id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash     BYTEA NOT NULL UNIQUE,
  expires_at     TIMESTAMPTZ NOT NULL,
  consumed_at    TIMESTAMPTZ,
  ip_hash        BYTEA NOT NULL,
  created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX password_reset_tokens_user_id_idx ON password_reset_tokens (user_id);

ALTER TABLE password_reset_tokens ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON password_reset_tokens
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- AUDIT LOG -----------------------------------------------------------------
-- Append-only. Grants applied in §6 below: orbit_app INSERT only;
-- orbit_support SELECT only; neither UPDATE/DELETE (S0-24, SEC-102).
-- ip_hash is declared here; the HMAC helper is S0-25 / T7.
CREATE TABLE audit_log (
  id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id           UUID,                                -- nullable for system actions
  actor_kind        TEXT NOT NULL CHECK (actor_kind IN ('user','system','worker','operator')),
  action            TEXT NOT NULL,                       -- e.g. 'grant.create'
  target_kind       TEXT,
  target_id         UUID,
  ip_hash           BYTEA,
  occurred_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  traceability_id   UUID,
  payload_summary   JSONB NOT NULL DEFAULT '{}'::jsonb   -- SEC-101 typed-allowlist validated in app
);
CREATE INDEX audit_log_user_id_idx     ON audit_log (user_id)    WHERE user_id IS NOT NULL;
CREATE INDEX audit_log_occurred_at_idx ON audit_log (occurred_at);
CREATE INDEX audit_log_action_idx      ON audit_log (action);

-- DSR REQUESTS --------------------------------------------------------------
CREATE TABLE dsr_requests (
  id                          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id                     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  kind                        TEXT NOT NULL CHECK (kind IN ('access','rectification','erasure','restriction','portability')),
  submitted_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
  sla_due_at                  TIMESTAMPTZ NOT NULL,
  completed_at                TIMESTAMPTZ,
  archive_object_storage_key  TEXT
);
ALTER TABLE dsr_requests ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON dsr_requests
  USING (user_id = current_setting('app.user_id', true)::uuid)
  WITH CHECK (user_id = current_setting('app.user_id', true)::uuid);

-- RULE SETS (empty in Slice 0; trigger armed per S0-18, SEC-082) ------------
CREATE TABLE rule_sets (
  id                  TEXT PRIMARY KEY,                   -- e.g. 'es-2026.1.0'
  jurisdiction        TEXT NOT NULL,                      -- 'ES', 'UK'
  aeat_guidance_date  DATE NOT NULL,
  effective_from      DATE NOT NULL,
  effective_to        DATE,
  content_hash        BYTEA NOT NULL,                     -- sha256
  status              TEXT NOT NULL CHECK (status IN ('proposed','active','superseded','withdrawn')),
  supersedes_id       TEXT REFERENCES rule_sets(id),
  data                JSONB NOT NULL,
  published_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
  published_by        UUID REFERENCES users(id)
);
CREATE INDEX rule_sets_jurisdiction_status_idx
  ON rule_sets (jurisdiction, status) WHERE status = 'active';

CREATE OR REPLACE FUNCTION rule_sets_reject_update_when_active()
RETURNS TRIGGER AS $$
BEGIN
  IF OLD.status = 'active' AND (
     NEW.id IS DISTINCT FROM OLD.id
     OR NEW.content_hash IS DISTINCT FROM OLD.content_hash
     OR NEW.data IS DISTINCT FROM OLD.data
     OR NEW.aeat_guidance_date IS DISTINCT FROM OLD.aeat_guidance_date
     OR NEW.effective_from IS DISTINCT FROM OLD.effective_from
  ) THEN
    RAISE EXCEPTION 'rule_sets: active rows are immutable (SEC-082)';
  END IF;
  -- Allow status transitions to 'superseded' (predecessor marker) or 'withdrawn'.
  IF OLD.status = 'active' AND NEW.status NOT IN ('active','superseded','withdrawn') THEN
    RAISE EXCEPTION 'rule_sets: invalid status transition from active';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER rule_sets_immutable_active
  BEFORE UPDATE ON rule_sets
  FOR EACH ROW EXECUTE FUNCTION rule_sets_reject_update_when_active();

-- RATE LIMIT BUCKETS --------------------------------------------------------
CREATE TABLE rate_limit_buckets (
  key                  TEXT PRIMARY KEY,
  tokens               DOUBLE PRECISION NOT NULL,
  last_refilled_at     TIMESTAMPTZ NOT NULL
);

-- ---------------------------------------------------------------------------
-- 4. Ownership
--    orbit_migrate owns every table and sequence created above so subsequent
--    migrations running as orbit_migrate can ALTER them.
-- ---------------------------------------------------------------------------
ALTER TABLE users                   OWNER TO orbit_migrate;
ALTER TABLE sessions                OWNER TO orbit_migrate;
ALTER TABLE email_verifications     OWNER TO orbit_migrate;
ALTER TABLE password_reset_tokens   OWNER TO orbit_migrate;
ALTER TABLE audit_log               OWNER TO orbit_migrate;
ALTER TABLE dsr_requests            OWNER TO orbit_migrate;
ALTER TABLE rule_sets               OWNER TO orbit_migrate;
ALTER TABLE rate_limit_buckets      OWNER TO orbit_migrate;
ALTER FUNCTION rule_sets_reject_update_when_active() OWNER TO orbit_migrate;

-- ---------------------------------------------------------------------------
-- 5. Grants — orbit_app
--    Full DML on user-scoped tables EXCEPT audit_log (INSERT only).
--    SELECT on rule_sets is needed for read-side rule lookups (no DML).
--    SELECT/INSERT/UPDATE on rate_limit_buckets for the leaky-bucket writer.
-- ---------------------------------------------------------------------------
GRANT SELECT, INSERT, UPDATE, DELETE ON users                 TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON sessions              TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON email_verifications   TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON password_reset_tokens TO orbit_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON dsr_requests          TO orbit_app;
GRANT SELECT                         ON rule_sets             TO orbit_app;
GRANT SELECT, INSERT, UPDATE         ON rate_limit_buckets    TO orbit_app;

-- audit_log: INSERT only. No SELECT, no UPDATE, no DELETE (SEC-102, S0-24).
GRANT INSERT ON audit_log TO orbit_app;

-- ---------------------------------------------------------------------------
-- 6. Grants — orbit_support
--    Slice 0a scope: SELECT on audit_log only. Nothing else.
-- ---------------------------------------------------------------------------
GRANT SELECT ON audit_log TO orbit_support;
