# ADR-011: Authentication, session, and MFA architecture

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)
- **Traces to:** ADR-001 (Rust + axum), ADR-005 (`users`, `sessions`, `audit_log` tables), ADR-010 (cookie auth, double-submit CSRF), spec §7.2 / §7.9, SEC-001..SEC-011, SEC-030..SEC-035, SEC-054, SEC-100, Slice-1 ACs G-8..G-10, AC-7.2.

## Context

SEC-001 through SEC-011 enumerate the **what** of authentication (argon2id params, rate limits, generic errors, session rotation, MFA seed handling, recovery-code hashing, new-device notice, mandatory-MFA toggle point). This ADR specifies the **how** — concrete crates, token shapes, table columns that extend ADR-005, and the end-to-end flows for signup, signin, password reset, and MFA enrolment.

Slice 1 needs signup, email verification, signin, signout, and password reset. MFA is **scaffolded but not reachable in Slice 1** (OQ-01 defers mandatory-MFA to v1.1 per SEC-011); the architecture must accept MFA addition in a future slice without rewriting the flow. Session lifecycle and refresh rotation are Slice-1 concerns because the `sessions` table and revocation plumbing ship in Slice 0 (S0-24).

The design must not contradict the UX's "one-page signup wizard that flows disclaimer → residency → first grant" (UX §4.1, Slice-1 AC 4.1–4.3). That flow sits **after** email+password creation; a multi-step signup wizard in the auth layer is therefore a misread of the UX. The auth layer does: password creation + email verification. The "wizard" is a series of onboarding screens that live in the authenticated app.

## Decision

### Crate choices

| Concern | Crate | Version floor | Rationale |
|---|---|---|---|
| Password hashing | `argon2` (the `password-hash` ecosystem crate) | 0.5.x | RustCrypto, audited, argon2id supported with OWASP 2024 params. |
| CSPRNG | `rand` with `OsRng` for secrets; `rand_chacha` if deterministic seeds ever needed in tests | 0.8.x | Standard. |
| Constant-time compare | `subtle` | 2.5.x | For token/hash comparisons. |
| TOTP | `totp-rs` | 5.x | RFC 6238 compliant, tested, no surprise deps. Used for MFA enrolment/verify from the slice that ships MFA (Slice 7 target). |
| JWT / PASETO | **Neither in v1.** Session IDs are opaque 32-byte CSPRNG values stored in `sessions`. | — | SEC-006 + SEC-010 want server-side revocation; opaque is simpler and cheaper than PASETO when the DB round-trip is already there. |
| Symmetric encryption for TOTP seed | `chacha20poly1305` (RustCrypto) via a thin `orbit-crypto` wrapper | 0.10.x | Column-level encrypt/decrypt per SEC-042. |
| HIBP breach-check | `reqwest` + hand-rolled k-anonymity client (`https://api.pwnedpasswords.com/range/{prefix}`) | — | ~30 LOC; no dep needed beyond `reqwest` + `sha1`. |
| axum cookie helpers | `axum-extra` with the `cookie` feature, plus `cookie` crate directly | axum-extra 0.9.x | Standard axum idiom. |
| Email send | `lettre` (SMTP client) for Postmark / Scaleway TX Email | 0.11.x | Pure-Rust SMTP; works with both providers. |
| Rate limiting | Hand-rolled **leaky-bucket** against a Postgres `rate_limit_buckets` table per SEC-160 | — | No Redis dep (ADR-001). |

**SHA-1 is used only inside the HIBP k-anonymity client per the HIBP API contract.** It is not used for password storage (argon2id) or for any Orbit-internal integrity surface.

### Session and token shapes

Concrete value shapes, all 32 random bytes unless noted:

- **`session_id`**: 32-byte CSPRNG, base64url-encoded → 43-char string. Stored **as SHA-256 hash** in `sessions.session_id_hash`. The cookie value is the raw base64url; the server hashes on lookup. Rationale: even a leaked DB dump does not hand out live sessions.
- **`refresh_id`**: 32-byte CSPRNG, base64url. Stored as `sessions.refresh_token_hash` (SHA-256). Same reasoning.
- **`csrf_token`**: 32-byte CSPRNG, base64url. Mirrored client-side; not stored server-side (double-submit pattern does not need a server-side store).
- **Email-verification token**: 32-byte CSPRNG base64url. Stored hashed in `email_verifications.token_hash`. Single-use, 24-hour expiry.
- **Password-reset token**: 32-byte CSPRNG base64url. Stored hashed in `password_reset_tokens.token_hash`. Single-use, 60-minute expiry (SEC-005).

Cookie specifics (all `Secure; HttpOnly; SameSite=Lax; Path=/` unless noted):

| Cookie | Max-Age | Path | HttpOnly |
|---|---|---|---|
| `orbit_sess` | 1800 (30 min) | `/` | yes |
| `orbit_refresh` | 604800 (7 days) | `/api/v1/auth` | yes |
| `orbit_csrf` | session lifetime | `/` | **no** (SPA reads it) |
| `orbit_locale` | 365 days | `/` | no (SPA reads) |

`orbit_locale` is an "essential cookie" for AEPD purposes (strictly necessary for the service the user requested — locale display) so no cookie banner is required in Slice 0 (SEC-127).

### Argon2id parameters (SEC-001)

```
m = 19456 KiB
t = 2
p = 1
salt = 16 bytes CSPRNG
output = 32 bytes
format = PHC string: $argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>
```

Parameters pinned in `orbit_auth::argon2::PARAMS` as a `const`. A unit test extracts and asserts these from a freshly-generated hash.

### Schema additions (extend ADR-005)

`users` gains:
- `email_verified_at TIMESTAMPTZ NULL`
- `password_changed_at TIMESTAMPTZ NOT NULL`
- `mfa_enrolled_at TIMESTAMPTZ NULL`
- `mfa_totp_secret_ciphertext BYTEA NULL` (encrypted per SEC-042)
- `mfa_recovery_codes_hashes TEXT[] NOT NULL DEFAULT '{}'` (argon2id hashes, one-shot)
- `mfa_disable_pending_at TIMESTAMPTZ NULL` (cool-down marker per SEC-009)

`sessions` (concrete for Slice 1):
- `id UUID PK`
- `user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE`
- `session_id_hash BYTEA NOT NULL UNIQUE` (SHA-256 of access-session secret)
- `refresh_token_hash BYTEA NOT NULL UNIQUE` (SHA-256 of refresh)
- `family_id UUID NOT NULL` (groups rotations; revoke-family on refresh reuse)
- `ip_hash BYTEA NOT NULL` (HMAC-SHA256 per SEC-054)
- `user_agent TEXT NOT NULL` (capped at 512 chars)
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `last_used_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `revoked_at TIMESTAMPTZ NULL`
- `revoke_reason TEXT NULL` (enum-like: `user_signout | refresh_reuse | password_change | mfa_change | admin`)

New tables in Slice 1:

`email_verifications`:
- `id UUID PK`
- `user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE`
- `token_hash BYTEA NOT NULL UNIQUE`
- `expires_at TIMESTAMPTZ NOT NULL`
- `consumed_at TIMESTAMPTZ NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- RLS scoped by `user_id`.

`password_reset_tokens`:
- `id UUID PK`
- `user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE`
- `token_hash BYTEA NOT NULL UNIQUE`
- `expires_at TIMESTAMPTZ NOT NULL` (created_at + 60 min)
- `consumed_at TIMESTAMPTZ NULL`
- `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`
- `ip_hash BYTEA NOT NULL`
- RLS scoped by `user_id`; cleanup worker purges expired rows weekly.

### Flows

Mermaid sequence diagrams for signin and password-reset, per the brief. Signup and MFA-enrolment described in prose below.

#### Signup (prose)

1. SPA `POST /api/v1/auth/signup` with `{ email, password, localeHint }`.
2. Server validates email format, runs HIBP k-anonymity check on password prefix, rejects if pwned (SEC-002).
3. Argon2id hash password. Insert `users` row with `email_verified_at = NULL`.
4. Insert `email_verifications` row, send email via `lettre` → Postmark EU with a link `https://app.orbit.<tld>/signup/verify-email?token=<raw>`.
5. Response: 201, no cookies yet. Audit-log `signup.success` (SEC-100).
6. User clicks link → SPA `POST /api/v1/auth/verify-email { token }`. Server hashes + looks up + marks `users.email_verified_at = now()`.
7. Server issues `orbit_sess`, `orbit_refresh`, `orbit_csrf` cookies (creates the first `sessions` row). Audit-log `login.success` with `reason = 'post_verification'`.
8. SPA redirects to `/app/signup/disclaimer` (the onboarding wizard, not an auth step).

Rate limits per SEC-160. Generic error per SEC-004.

#### Signin sequence

```mermaid
sequenceDiagram
    autonumber
    participant User
    participant SPA as React SPA
    participant API as axum API
    participant PG as Postgres
    participant Mail as Postmark

    User->>SPA: email + password
    SPA->>API: POST /api/v1/auth/signin (x-csrf-token on new-session path: skipped on first-load; generated in response)
    API->>API: Rate limit: per-IP 10/10m, per-account 5/10m (SEC-003)
    API->>API: HIBP check on password if rotated recently (optional; skip hot path)
    API->>PG: SELECT users WHERE lower(email) = lower($1)
    alt user not found / wrong password / locked
        API-->>SPA: 401 { code: "auth.invalid_credentials" } (generic; SEC-004)
        API->>PG: INSERT audit_log(action='login.failure', user_id=nullable)
    else password OK + email verified
        alt mfa_enrolled_at IS NULL OR mfa not required
            API->>API: mint session_id, refresh_id, csrf_token, family_id (CSPRNG)
            API->>PG: INSERT sessions (...hashes..., ip_hash, ua)
            API->>PG: INSERT audit_log(action='login.success')
            API-->>SPA: 200 + Set-Cookie orbit_sess/refresh/csrf
            opt new device (ip_hash or ua never seen for user)
                API->>Mail: "New sign-in from <city-guess>" email (SEC-010)
            end
            SPA->>User: redirect to /app/dashboard
        else mfa required (v1.1+)
            API->>API: mint short-lived mfa_challenge_token (5 min), set mfa_challenge cookie
            API-->>SPA: 200 { mfaRequired: true }
            SPA->>User: show TOTP entry
            User->>SPA: 6-digit code
            SPA->>API: POST /api/v1/auth/mfa/challenge { code }
            API->>PG: SELECT users.mfa_totp_secret_ciphertext
            API->>API: decrypt TOTP seed, verify window ±1
            alt verified
                API->>PG: INSERT sessions, audit_log(action='login.success', factors=['password','totp'])
                API-->>SPA: 200 + cookies
            else failed
                API->>PG: INSERT audit_log(action='login.mfa_failure')
                API-->>SPA: 401 { code: "auth.mfa_invalid" }
            end
        end
    else email not verified
        API-->>SPA: 403 { code: "auth.email_unverified" }
        SPA->>User: "Check your inbox" state with "resend" button (rate limited)
    end
```

#### Password reset sequence

```mermaid
sequenceDiagram
    autonumber
    participant User
    participant SPA
    participant API
    participant PG
    participant Mail

    User->>SPA: enter email on /signin/forgot
    SPA->>API: POST /api/v1/auth/forgot { email }
    API->>API: Rate limit: 3/email/hour, 5/IP/hour (SEC-160)
    API->>PG: SELECT users WHERE lower(email) = lower($1)
    alt user exists + email verified
        API->>API: mint reset_token (32B CSPRNG)
        API->>PG: INSERT password_reset_tokens(user_id, token_hash, expires_at=now()+60m, ip_hash)
        API->>Mail: "Reset link: https://app.orbit.<tld>/signin/reset/<raw>"
    else user absent or unverified
        API->>API: no-op (generic response per SEC-004)
    end
    API-->>SPA: 200 { message: "Si la dirección existe..." }
    Note over User: User clicks email link
    User->>SPA: GET /signin/reset/:token (client route)
    SPA->>User: show new-password form
    User->>SPA: new password (twice)
    SPA->>API: POST /api/v1/auth/reset { token, newPassword }
    API->>API: HIBP check on newPassword (SEC-002)
    API->>PG: SELECT password_reset_tokens WHERE token_hash=$1 AND consumed_at IS NULL AND expires_at > now()
    alt found
        API->>API: argon2id hash newPassword
        API->>PG: BEGIN; UPDATE users SET password_hash, password_changed_at=now(); \
                    UPDATE password_reset_tokens SET consumed_at=now(); \
                    UPDATE sessions SET revoked_at=now(), revoke_reason='password_change' WHERE user_id=$1 AND revoked_at IS NULL; \
                    DELETE FROM password_reset_tokens WHERE user_id=$1 AND id != $2; \
                    INSERT audit_log(action='password.reset.success'); \
                  COMMIT;
        API->>Mail: "Tu contraseña se ha cambiado" email to all known addresses (SEC-009 pattern)
        API-->>SPA: 200 { redirectTo: '/signin' }
        Note over SPA: user re-signs-in; MFA challenge NOT bypassed (SEC-009)
    else not found / expired / consumed
        API-->>SPA: 400 { code: "auth.reset_invalid_or_expired" }
    end
```

#### Signout

- `POST /api/v1/auth/signout` under `[A]` + `[V]`.
- Server looks up current session by `session_id_hash`, sets `revoked_at = now()`, `revoke_reason = 'user_signout'`, clears cookies in the response.
- Audit-log `logout`.

#### Refresh rotation

- SPA calls `POST /api/v1/auth/refresh` automatically ~5 min before `orbit_sess` expires.
- Server looks up `sessions` by `refresh_token_hash`.
  - If `revoked_at IS NOT NULL`: revoke the entire `family_id` (refresh-reuse detection) and return 401 forcing signin. Audit-log `session.refresh_reuse_detected`.
  - Otherwise: mint new `session_id` and `refresh_id`, insert **a new `sessions` row** with the same `family_id`, set the old row's `revoked_at = now()` and `revoke_reason = 'refresh_rotation'`. Return Set-Cookie with the new values.

#### MFA enrolment (architected, not reachable in Slice 1)

1. `POST /api/v1/auth/mfa/enroll/start` under `[A]` → server mints 20-byte TOTP secret, returns the OTP-auth URI for QR display + a one-shot seed string. Server stores the secret encrypted in `users.mfa_totp_secret_ciphertext` with a tentative `mfa_enrolled_at = NULL` marker (enrolment in progress, rejected on challenge).
2. User enters a 6-digit code from their authenticator.
3. `POST /api/v1/auth/mfa/enroll/confirm { code }` → server verifies ±1 window; on success, generates 10 recovery codes (16-char base32, argon2id-hashed one-shot), returns them once, sets `users.mfa_enrolled_at = now()`.
4. Audit-log `mfa.enroll.success`.

**MFA disable** (SEC-009): request creates a `mfa_disable_pending_at = now()` marker; email sent to all known addresses; 24 h later, a background job actually clears the seed; user can abort via a signed link from the email.

### Rate limit store

Postgres-backed leaky bucket:

```sql
CREATE TABLE rate_limit_buckets (
  key TEXT PRIMARY KEY,             -- e.g. "signin:ip:sha256-hex" or "signin:account:<user_uuid>"
  tokens DOUBLE PRECISION NOT NULL,
  last_refilled_at TIMESTAMPTZ NOT NULL
);
```

- `tokens` refilled at `limit / period` rate on read.
- SELECT-FOR-UPDATE on `key` inside the same tx that handles the request.
- At Slice-1 traffic this is trivially cheap. Revisit if lock contention appears at Slice 5.

### Email deliverability posture

- SPF, DKIM, DMARC records set on `orbit.<tld>` before Slice 0 completes.
- Outbound provider Postmark EU (SEC-121) or Scaleway TX Email; decision per ADR-002 follow-up. Either way: SCCs + TIA documented if US-HQ, sub-processor register lists role and data categories.

### What Slice 1 actually ships

- Signup, email verification, signin (no MFA challenge), signout, refresh rotation, forgot-password, reset-password, new-device email.
- `sessions`, `email_verifications`, `password_reset_tokens`, `rate_limit_buckets` tables.
- `/api/v1/auth/mfa/*` endpoints **declared in OpenAPI but return 501** in Slice 1. The contract shape lets the SPA carry the challenge screen placeholder without auth-layer churn when MFA is turned on.
- The "Active sessions" UI (SEC-010) is deferred to Slice 2/3 with the Account panel (C-7 in open-questions-resolved). The **backend** (revoke endpoint, session-list endpoint) ships in Slice 1 for use in tests.

## Alternatives considered

- **PASETO v4.local tokens in cookies.** Tempting for tamper-evidence without a DB hit. Rejected: we do a DB hit anyway for RLS `Tx::for_user` and for `sessions.revoked_at` — there is no cold path that benefits from PASETO's statelessness. Opaque IDs are simpler.
- **JWT in `Authorization` header.** See ADR-010 §5; rejected there.
- **WebAuthn / passkeys as the default second factor.** Stronger than TOTP but recovery complexity is real; one-developer support budget says TOTP first, WebAuthn as a v2 addition.
- **Magic link signin (passwordless).** Rejected for v1 on two counts: (a) raises the email-provider SPoF for every signin, (b) persona-B is technical and uses a password manager; magic links would be friction.
- **Bcrypt / scrypt.** Rejected explicitly by SEC-001.
- **Redis rate-limit store.** Rejected to keep dependency count low; Postgres leaky-bucket is sufficient at Slice-1..5 scale.
- **CSRF via SameSite=Strict only (drop double-submit).** `SameSite=Strict` breaks common UX (clicking an emailed magic link does not carry the cookie); Slice-1 does not use magic links but the Lax+double-submit pattern is more robust across edge cases like Slice-5's market-quote webhook flow. Keep double-submit.
- **Store refresh tokens unhashed but encrypted.** More complex, no better security posture than SHA-256 hash at rest.

## Consequences

**Positive:**
- Opaque session IDs + hashed storage make a DB leak non-catastrophic for live sessions.
- Refresh-reuse detection is a concrete flow, not a TODO.
- MFA scaffolding lets v1.1's mandatory-toggle flip one config and ship.
- Generic error copy + rate limits + HIBP check cover the credential-stuffing threat model (S1).
- Every state-changing auth action is audit-logged per SEC-100; the `audit_log.payload_summary` never carries password/token values (SEC-101).

**Negative / risks:**
- Session rotation on every refresh creates many `sessions` rows over time. Retention worker purges `revoked_at < now() - 90 days` rows weekly; acceptable.
- Email is a soft SPoF for signup + reset. Mitigation: the provider is monitored; a 2-provider failover is v1.1. Signin itself works without email — only reset needs the provider live.
- Postgres rate-limit bucket assumes a single API instance. ADR-002 is explicit that v1 has one. Revisit at scale.
- TOTP-only MFA means a lost phone is a support ticket; backup codes mitigate. WebAuthn is the v2 story.
- Cool-down disable (SEC-009) is asynchronous (a job runs 24 h later). The `mfa_disable_pending_at` column is the state anchor; tests cover the "user aborts via email" path.

**Tension with prior ADRs:**
- None. ADR-005's `sessions` outline is extended, not changed.

**Follow-ups:**
- Implementation engineer: `orbit_auth` crate exports `Argon2id::hash`, `Argon2id::verify`, `SessionService::{create, rotate, revoke}`, `EmailVerificationService`, `PasswordResetService`. One-helper-per-concern; each has integration tests against a real Postgres.
- Implementation engineer: write cross-tenant probes (SEC-023) against `/api/v1/auth/me` and session-list endpoints.
- Implementation engineer: integration test asserts that an old `orbit_sess` value after a signin-rotation does not authenticate (SEC-007).
- Slice-7 follow-up: flip `MFA_MANDATORY_FOR_PAID = true` config, add the enrolment nag to the first-paid-upgrade screen.
- Security-engineer: confirm HIBP k-anonymity query is acceptable in the outbound-allowlist (SEC-149); the only other egress is to `api.pwnedpasswords.com` (Cloudflare-fronted, globally cached).
