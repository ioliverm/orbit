-- Slice 1 T17 / S1 (sec-review): pin `search_path` on the two T13a
-- SECURITY DEFINER lookup functions (CWE-426 defense-in-depth).
--
-- Both `lookup_session_by_hash` and `lookup_email_verification_by_hash`
-- were introduced in 20260502120000_t13a_session_lookup.sql as
-- SECURITY DEFINER so the unauthenticated cookie/email-verify paths can
-- resolve a user_id before `Tx::for_user` primes RLS. Without a pinned
-- search_path, a definer-rights function resolves unqualified names
-- against the caller's search_path, which an attacker with CREATE on
-- any schema on the search_path could abuse to shadow `sessions` or
-- `email_verifications` with a malicious view.
--
-- In the current role layout `orbit_app` has no CREATE on any schema
-- (see 20260418120000_init.sql §4), so the risk is theoretical — but
-- search_path-pinning is a well-known Postgres hardening requirement
-- for definer-rights code and costs us nothing.
--
-- `pg_catalog` is listed first so built-in names (`uuid`, `timestamptz`,
-- `=`, `now()`) resolve to the trusted schema regardless of what else
-- the caller has on its path. `public` follows so the two tables the
-- functions reference continue to resolve (both were created there by
-- the init migration).

ALTER FUNCTION lookup_session_by_hash(BYTEA)
  SET search_path = pg_catalog, public;

ALTER FUNCTION lookup_email_verification_by_hash(BYTEA)
  SET search_path = pg_catalog, public;
