-- Orbit local Postgres — role provisioning (Slice 0a local flavor of S0-16).
--
-- Contract (from ADR-015 "orbit_support provisioned now", ADR-014 §Roles):
--
--   orbit_migrate   Schema owner. Owns tables + indexes + policies. Used ONLY
--                   by `orbit migrate`. NOT a superuser. NO BYPASSRLS.
--   orbit_app       Application runtime role. Holds DML privileges granted by
--                   migrations. NOT a superuser. NO BYPASSRLS — RLS policies
--                   apply to every query this role runs.
--   orbit_support   Read-only support/reporting role. SELECT-only on the
--                   tables the audit/support workflow needs (grants come with
--                   migrations). NOT a superuser. NO BYPASSRLS.
--
-- None of the three roles is a superuser. None has BYPASSRLS. None has
-- CREATEROLE, CREATEDB, or REPLICATION. The superuser used here is the
-- bootstrap `postgres` account that only exists to run this init script.
-- T6 owns migrations (DDL + grants + policies) — this file stops at roles.
--
-- NOTE ON PASSWORDS: `:'orbit_migrate_password'` style placeholders use
-- psql's built-in `:'var'` substitution (standard since Postgres 8.4).
-- `00-render-roles.sh` passes the three values via `-v var=value`; psql
-- expands them with proper single-quote escaping inline. The rendered
-- SQL never touches disk. We avoid envsubst / gettext-base because those
-- are not shipped in the postgres:16-bookworm image.

-- Defensive: fail if any of the three roles already exists. The init scripts
-- only run on first boot so this should never trigger, but if somebody has
-- tampered with PGDATA we'd rather stop than quietly continue.
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'orbit_migrate') THEN
    RAISE EXCEPTION 'orbit_migrate already exists — refusing to re-init';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'orbit_app') THEN
    RAISE EXCEPTION 'orbit_app already exists — refusing to re-init';
  END IF;
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'orbit_support') THEN
    RAISE EXCEPTION 'orbit_support already exists — refusing to re-init';
  END IF;
END$$;

-- orbit_migrate: owner of the schema. Can CREATE in public (we don't rely on
-- public; migrations create `orbit` or keep things in `public` per T6). Gets
-- no cluster-wide attributes beyond LOGIN.
CREATE ROLE orbit_migrate
  WITH LOGIN
       NOSUPERUSER
       NOCREATEDB
       NOCREATEROLE
       NOREPLICATION
       NOBYPASSRLS
       INHERIT
       CONNECTION LIMIT 4
       PASSWORD :'orbit_migrate_password';

-- orbit_app: runtime role the API + worker bind as. RLS applies. DML grants
-- are issued by migrations (T6) — this role starts with only the implicit
-- public-schema USAGE.
CREATE ROLE orbit_app
  WITH LOGIN
       NOSUPERUSER
       NOCREATEDB
       NOCREATEROLE
       NOREPLICATION
       NOBYPASSRLS
       INHERIT
       CONNECTION LIMIT 50
       PASSWORD :'orbit_app_password';

-- orbit_support: read-only support role. SELECT grants are issued by
-- migrations (T6) on the specific audit/support-relevant tables. Never
-- INSERT/UPDATE/DELETE, never BYPASSRLS, never DDL.
CREATE ROLE orbit_support
  WITH LOGIN
       NOSUPERUSER
       NOCREATEDB
       NOCREATEROLE
       NOREPLICATION
       NOBYPASSRLS
       INHERIT
       CONNECTION LIMIT 4
       PASSWORD :'orbit_support_password';

-- Make orbit_migrate the owner of the `orbit` database so migrations can run
-- without superuser. The database itself was created by the image from
-- POSTGRES_DB=orbit; we just transfer ownership.
ALTER DATABASE orbit OWNER TO orbit_migrate;

-- Ensure the three roles can connect to the orbit database. Other databases
-- (template1, postgres) are not in scope.
REVOKE ALL ON DATABASE orbit FROM PUBLIC;
GRANT CONNECT, TEMPORARY ON DATABASE orbit TO orbit_migrate;
GRANT CONNECT ON DATABASE orbit TO orbit_app;
GRANT CONNECT ON DATABASE orbit TO orbit_support;

-- Default-schema access: orbit_migrate owns future objects in public; app +
-- support need USAGE on `public` to see tables migrations create. Per-table
-- DML/SELECT grants are T6's responsibility.
GRANT USAGE ON SCHEMA public TO orbit_app, orbit_support;

-- Sanity post-conditions. If any of these fail, init aborts and the
-- container does not come up.
DO $$
DECLARE
  r RECORD;
BEGIN
  FOR r IN
    SELECT rolname, rolsuper, rolbypassrls, rolcreatedb, rolcreaterole, rolreplication
    FROM pg_roles
    WHERE rolname IN ('orbit_migrate','orbit_app','orbit_support')
  LOOP
    IF r.rolsuper THEN
      RAISE EXCEPTION '% has SUPERUSER — aborting', r.rolname;
    END IF;
    IF r.rolbypassrls THEN
      RAISE EXCEPTION '% has BYPASSRLS — aborting', r.rolname;
    END IF;
    IF r.rolcreatedb OR r.rolcreaterole OR r.rolreplication THEN
      RAISE EXCEPTION '% has an extra cluster privilege — aborting', r.rolname;
    END IF;
  END LOOP;
END$$;
