-- Slice 1 T13a — session + email-verification lookup helpers.
--
-- Why: the session middleware reads the `sessions` row by `session_id_hash`
-- before the owning `user_id` is known, so it cannot prime `app.user_id`
-- via `Tx::for_user`. Under RLS, `orbit_app` reading `sessions` without a
-- primed GUC returns zero rows (the policy filters everything). The same
-- problem hits `email_verifications` during the verify-email flow, where
-- the raw token is the entry key.
--
-- Solution: two `SECURITY DEFINER` functions owned by `orbit_migrate`
-- (which owns the underlying tables and therefore bypasses the non-forced
-- RLS policy). The functions return only the fields the handler needs,
-- and they EXECUTE-privilege only to `orbit_app`. They do not expose any
-- additional surface beyond the unique-indexed reverse lookup that the
-- cookie already authorises; their existence is equivalent to having a
-- view on `(session_id_hash → user_id, …)` scoped to that one key.
--
-- This is the narrowest relaxation of SEC-022's "every read goes through
-- Tx::for_user" compatible with cookie auth. The subsequent writes (touch
-- last_used_at, INSERT session on signin, UPDATE sessions on signout,
-- UPDATE email_verifications on verify) continue to route through
-- `Tx::for_user(user_id)` once the id has been resolved.

-- ---------------------------------------------------------------------------
-- lookup_session_by_hash
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION lookup_session_by_hash(p_hash BYTEA)
RETURNS TABLE (
    id          UUID,
    user_id     UUID,
    created_at  TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ
) AS $$
  SELECT id, user_id, created_at, revoked_at
    FROM sessions
   WHERE session_id_hash = p_hash
   LIMIT 1
$$ LANGUAGE sql
   SECURITY DEFINER
   STABLE;

ALTER FUNCTION lookup_session_by_hash(BYTEA) OWNER TO orbit_migrate;
REVOKE ALL ON FUNCTION lookup_session_by_hash(BYTEA) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION lookup_session_by_hash(BYTEA) TO orbit_app;

-- ---------------------------------------------------------------------------
-- lookup_email_verification_by_hash
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION lookup_email_verification_by_hash(p_hash BYTEA)
RETURNS TABLE (
    id           UUID,
    user_id      UUID,
    expires_at   TIMESTAMPTZ,
    consumed_at  TIMESTAMPTZ
) AS $$
  SELECT id, user_id, expires_at, consumed_at
    FROM email_verifications
   WHERE token_hash = p_hash
   LIMIT 1
$$ LANGUAGE sql
   SECURITY DEFINER
   STABLE;

ALTER FUNCTION lookup_email_verification_by_hash(BYTEA) OWNER TO orbit_migrate;
REVOKE ALL ON FUNCTION lookup_email_verification_by_hash(BYTEA) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION lookup_email_verification_by_hash(BYTEA) TO orbit_app;
