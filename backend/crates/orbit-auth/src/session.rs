//! Session + CSRF token primitives (S0-22).
//!
//! The flow these primitives fit into (ADR-011):
//!
//! 1. On signin, mint a `SessionToken` and a `CsrfToken`, each 32 bytes of
//!    CSPRNG. The raw bytes go out to the client (in the cookie); their
//!    SHA-256 digest is what we persist in `sessions.session_id_hash`
//!    (session) — so a DB leak does not equal a live-session leak.
//! 2. The session cookie is `HttpOnly; Secure; SameSite=Lax; Path=/`.
//! 3. The CSRF cookie is readable by the SPA (not `HttpOnly`), and the SPA
//!    echoes it back on every state-changing request via `X-CSRF-Token`.
//!    The server compares header vs cookie in constant time
//!    ([`verify_csrf_double_submit`]).
//!
//! This module does not talk to Postgres or axum. It's the shape-only layer;
//! the axum wiring lives in `orbit-api` in a later slice.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use cookie::{Cookie, SameSite};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Cookie name for the opaque access-session ID (ADR-011 §"Session and token
/// shapes").
pub const SESSION_COOKIE_NAME: &str = "orbit_sess";
/// Session cookie lifetime: 30 minutes. ADR-011 §"Session and token shapes".
pub const SESSION_MAX_AGE: Duration = Duration::from_secs(1800);
/// Length in bytes of a raw token before base64url encoding. 32 bytes = 256
/// bits of entropy, matching SEC-005 / ADR-011.
pub const TOKEN_LEN: usize = 32;

/// Raw session token (base64url-encoded 32-byte CSPRNG value). This is the
/// opaque cookie value sent to the client. Never store this directly; store
/// the SHA-256 digest ([`SessionTokenHash`]) instead.
#[derive(Debug, Clone)]
pub struct SessionToken(String);

impl SessionToken {
    /// Expose the cookie-ready base64url string. Kept on an inherent method
    /// rather than a `Display` impl to avoid accidental logging — the token
    /// is a Regulatory-Critical secret (SEC-050).
    pub fn as_cookie_value(&self) -> &str {
        &self.0
    }
}

/// SHA-256 digest of a [`SessionToken`]'s raw bytes. Stored in
/// `sessions.session_id_hash`. 32 bytes; matches the BYTEA column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTokenHash(pub [u8; 32]);

/// Raw CSRF token (base64url-encoded 32-byte CSPRNG value). Sent in the
/// `orbit_csrf` cookie (not `HttpOnly`, so the SPA can read it) and echoed
/// back in the `X-CSRF-Token` header. Not stored server-side — the
/// double-submit pattern only compares the two wire values.
#[derive(Debug, Clone)]
pub struct CsrfToken(String);

impl CsrfToken {
    /// Expose the cookie-ready base64url string.
    pub fn as_cookie_value(&self) -> &str {
        &self.0
    }
}

/// SHA-256 digest of a [`CsrfToken`]. The double-submit pattern does not
/// require storing this server-side, but we return it alongside the raw
/// token anyway so that future slices that want at-rest hashing
/// (e.g. for a "revoke all" list) do not have to re-introduce a second
/// mint API. Currently unused by the auth flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsrfTokenHash(pub [u8; 32]);

fn mint_token() -> (String, [u8; 32]) {
    let mut raw = [0u8; TOKEN_LEN];
    OsRng.fill_bytes(&mut raw);
    let encoded = URL_SAFE_NO_PAD.encode(raw);
    let hash: [u8; 32] = Sha256::digest(raw).into();
    (encoded, hash)
}

/// Mint a new opaque session token. Returns the cookie-ready value **and**
/// the SHA-256 hash that must be written to `sessions.session_id_hash`.
pub fn new_session_token() -> (SessionToken, SessionTokenHash) {
    let (encoded, hash) = mint_token();
    (SessionToken(encoded), SessionTokenHash(hash))
}

/// Mint a new CSRF double-submit token. Returns the cookie-ready value and
/// its SHA-256 hash (see [`CsrfTokenHash`] for storage notes).
pub fn new_csrf_token() -> (CsrfToken, CsrfTokenHash) {
    let (encoded, hash) = mint_token();
    (CsrfToken(encoded), CsrfTokenHash(hash))
}

/// Build the session-cookie header value.
///
/// Flags (ADR-011 §"Cookie specifics", SEC-006 / SEC-189):
///
/// * `HttpOnly` — yes. JS cannot read this cookie; mitigates XSS → session
///   theft.
/// * `Secure` — **always on**, in every code path, no dev-mode escape hatch.
///   The factory never emits a plain-HTTP cookie. If a local developer needs
///   to hit the API from an HTTP origin they must terminate HTTPS at a local
///   proxy (Caddy, Vite dev-server self-signed) — a separate helper would
///   be introduced for that, keeping this one unconditionally safe
///   (defense against regressing SEC-189).
/// * `SameSite=Lax` — safe default for an SPA under the same origin; allows
///   top-level GET navigations to carry the cookie (needed for the email
///   verification and password-reset link flows).
/// * `Path=/` — the whole SPA uses it.
/// * `Max-Age=1800` — 30 min per ADR-011. Refresh rotation is handled
///   separately in Slice 1.
///
/// If `domain` is `Some`, it is set on the cookie; if `None` the cookie is
/// host-scoped, which is the correct default in dev and for single-host
/// production deployments.
pub fn session_cookie(token: &SessionToken, domain: Option<&str>) -> Cookie<'static> {
    // `cookie::Cookie::build` takes borrowed or owned values; use owned so
    // the returned `Cookie<'static>` doesn't carry a lifetime tied to the
    // argument.
    let mut builder = Cookie::build((SESSION_COOKIE_NAME, token.0.clone()))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .max_age(
            cookie::time::Duration::try_from(SESSION_MAX_AGE)
                .expect("1800s fits in a cookie Duration"),
        );
    if let Some(d) = domain {
        builder = builder.domain(d.to_owned());
    }
    builder.build()
}

/// Constant-time comparison of a double-submit CSRF pair: the header value
/// from `X-CSRF-Token` and the cookie value from `orbit_csrf`.
///
/// Returns `true` iff both values are present, equal in length, and equal
/// byte-for-byte in constant time ([`subtle::ConstantTimeEq`]). An empty
/// string on either side is a mismatch.
///
/// The caller is responsible for first fetching the header and cookie from
/// the request; this function is deliberately framework-agnostic.
pub fn verify_csrf_double_submit(header: &str, cookie: &str) -> bool {
    // `ConstantTimeEq` short-circuits on unequal lengths but still returns a
    // `Choice`; we reject zero-length up front so a missing header doesn't
    // trivially match a missing cookie (both empty) and authorize the
    // request.
    if header.is_empty() || cookie.is_empty() {
        return false;
    }
    header.as_bytes().ct_eq(cookie.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_token_is_32_bytes_base64url() {
        let (tok, hash) = new_session_token();
        // 32 bytes base64url-encoded without padding = ceil(32 * 8 / 6) = 43.
        assert_eq!(tok.as_cookie_value().len(), 43);
        // URL_SAFE alphabet: [A-Za-z0-9_-]; no '+' or '/' or '=' allowed.
        for c in tok.as_cookie_value().chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '_' || c == '-',
                "non-base64url char: {c}"
            );
        }
        // Hash is SHA-256 of the *raw* bytes (the bytes base64-encoded in the
        // cookie value), not of the base64url string. We can't recompute
        // without the raw bytes, so assert length + non-zero.
        assert_eq!(hash.0.len(), 32);
        assert_ne!(hash.0, [0u8; 32]);
    }

    #[test]
    fn session_tokens_are_unique() {
        let (a, _) = new_session_token();
        let (b, _) = new_session_token();
        assert_ne!(a.as_cookie_value(), b.as_cookie_value());
    }

    #[test]
    fn csrf_token_shape_matches_session_token() {
        let (tok, hash) = new_csrf_token();
        assert_eq!(tok.as_cookie_value().len(), 43);
        assert_eq!(hash.0.len(), 32);
    }

    #[test]
    fn session_cookie_has_required_flags() {
        let (tok, _) = new_session_token();
        let c = session_cookie(&tok, None);
        assert_eq!(c.name(), SESSION_COOKIE_NAME);
        assert_eq!(c.value(), tok.as_cookie_value());
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(
            c.secure(),
            Some(true),
            "Secure must always be set (SEC-189)"
        );
        assert_eq!(c.same_site(), Some(SameSite::Lax));
        assert_eq!(c.path(), Some("/"));
        assert_eq!(
            c.max_age().map(|d| d.whole_seconds()),
            Some(1800),
            "session Max-Age must be 1800s (ADR-011)"
        );
    }

    #[test]
    fn session_cookie_honours_domain_when_provided() {
        let (tok, _) = new_session_token();
        assert_eq!(session_cookie(&tok, None).domain(), None);
        assert_eq!(
            session_cookie(&tok, Some("app.orbit.example")).domain(),
            Some("app.orbit.example")
        );
    }

    #[test]
    fn csrf_double_submit_accepts_matching_pair() {
        let (tok, _) = new_csrf_token();
        let v = tok.as_cookie_value();
        assert!(verify_csrf_double_submit(v, v));
    }

    #[test]
    fn csrf_double_submit_rejects_mismatch() {
        let (a, _) = new_csrf_token();
        let (b, _) = new_csrf_token();
        assert!(!verify_csrf_double_submit(
            a.as_cookie_value(),
            b.as_cookie_value()
        ));
    }

    #[test]
    fn csrf_double_submit_rejects_empty() {
        assert!(!verify_csrf_double_submit("", ""));
        assert!(!verify_csrf_double_submit("abc", ""));
        assert!(!verify_csrf_double_submit("", "abc"));
    }

    #[test]
    fn csrf_double_submit_rejects_length_mismatch() {
        assert!(!verify_csrf_double_submit("abc", "abcd"));
    }
}
