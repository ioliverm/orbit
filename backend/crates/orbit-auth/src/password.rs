//! Argon2id password hashing pinned to OWASP 2024 parameters (SEC-001).
//!
//! This module intentionally exposes **no parameter knobs**. The params are
//! a `const` pulled straight from the ADR and asserted at hash-time with a
//! runtime `debug_assert!`-style check that returns `Err` on mismatch rather
//! than panicking (the caller should not be able to construct a `Params` that
//! differs from this crate's — we don't accept one as input — but a future
//! refactor that wires env-var overrides would trip this guard before the
//! first bad hash reaches the DB).
//!
//! The PHC string produced and consumed here is:
//!
//! ```text
//! $argon2id$v=19$m=19456,t=2,p=1$<base64-salt>$<base64-hash>
//! ```

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::{Algorithm, Argon2, Params, Version};

/// OWASP-2024 memory parameter (KiB). Pinned: 19 MiB = 19456 KiB.
pub const ARGON2_M_COST: u32 = 19_456;
/// OWASP-2024 time parameter (iterations). Pinned.
pub const ARGON2_T_COST: u32 = 2;
/// OWASP-2024 parallelism parameter. Pinned.
pub const ARGON2_P_COST: u32 = 1;
/// Output length in bytes (32 → 256 bit hash).
pub const ARGON2_OUT_LEN: usize = 32;

/// Errors this module can emit.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Argon2 parameter construction failed. Signals a programming error
    /// inside this crate (the pinned constants are invalid), not a caller
    /// fault; surfaced as `Err` rather than `panic!` to keep the hasher
    /// drop-safe in long-running services.
    #[error("invalid argon2 parameters: {0}")]
    Params(argon2::Error),
    /// Hashing failed. `argon2::password_hash::Error` covers both hashing
    /// and PHC-serialization errors; we collapse to one variant because the
    /// caller cannot meaningfully branch on the subtype.
    #[error("argon2 hash failed: {0}")]
    Hash(argon2::password_hash::Error),
    /// The stored PHC string was malformed or could not be verified.
    #[error("argon2 verify failed: {0}")]
    Verify(argon2::password_hash::Error),
    /// Post-hash self-check found params that don't match the pinned
    /// constants. Either `ARGON2_*` was changed without bumping the
    /// migration plan, or the `argon2` crate's defaults drifted.
    #[error("argon2 produced a hash with unexpected params (SEC-001 drift)")]
    ParamDrift,
}

fn hasher() -> Result<Argon2<'static>, Error> {
    let params = Params::new(
        ARGON2_M_COST,
        ARGON2_T_COST,
        ARGON2_P_COST,
        Some(ARGON2_OUT_LEN),
    )
    .map_err(Error::Params)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hash `password` and return the PHC-encoded string suitable for storage in
/// `users.password_hash`.
///
/// Post-hash, the returned string is re-parsed and its embedded params are
/// compared against [`ARGON2_M_COST`], [`ARGON2_T_COST`], [`ARGON2_P_COST`].
/// A mismatch is reported as [`Error::ParamDrift`] rather than a silent bad
/// write — SEC-001 requires the params to be verifiable from the stored hash.
pub fn hash(password: &str) -> Result<String, Error> {
    let argon2 = hasher()?;
    let salt = SaltString::generate(&mut OsRng);
    let phc = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(Error::Hash)?
        .to_string();

    // Self-check: parse what we just produced and assert the params. This
    // guards against future refactors that accidentally override the hasher
    // with weaker defaults.
    let parsed = PasswordHash::new(&phc).map_err(Error::Hash)?;
    let got = Params::try_from(&parsed).map_err(Error::Hash)?;
    if got.m_cost() != ARGON2_M_COST
        || got.t_cost() != ARGON2_T_COST
        || got.p_cost() != ARGON2_P_COST
    {
        return Err(Error::ParamDrift);
    }

    Ok(phc)
}

/// Verify `password` against the PHC string `phc`.
///
/// Returns `Ok(true)` on match, `Ok(false)` on mismatch. Malformed PHC strings
/// or unsupported algorithm markers return `Err(Error::Verify)` — the caller
/// is expected to translate that to the same generic `auth.invalid_credentials`
/// response as a plain mismatch (SEC-004), but we keep them distinguishable
/// here for audit-log triage.
pub fn verify(password: &str, phc: &str) -> Result<bool, Error> {
    let argon2 = hasher()?;
    let parsed = PasswordHash::new(phc).map_err(Error::Verify)?;
    match argon2.verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(other) => Err(Error::Verify(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_round_trips() {
        let phc = hash("correct horse battery staple").expect("hash");
        assert!(verify("correct horse battery staple", &phc).expect("verify"));
        assert!(!verify("wrong password", &phc).expect("verify"));
    }

    #[test]
    fn phc_embeds_owasp_2024_params() {
        // SEC-001: params must be verifiable from the stored hash. This is
        // the authoritative pinning test.
        let phc = hash("a-long-enough-test-passphrase").expect("hash");
        let parsed = PasswordHash::new(&phc).expect("parse phc");
        let params = Params::try_from(&parsed).expect("extract params");
        assert_eq!(params.m_cost(), 19_456, "m_cost (SEC-001)");
        assert_eq!(params.t_cost(), 2, "t_cost (SEC-001)");
        assert_eq!(params.p_cost(), 1, "p_cost (SEC-001)");
    }

    #[test]
    fn phc_uses_argon2id_v13() {
        let phc = hash("pin-the-alg").expect("hash");
        // The prefix is stable per the PHC spec; check it directly so a
        // future bump to argon2d / argon2i fails loud.
        assert!(phc.starts_with("$argon2id$v=19$"), "got: {phc}");
    }

    #[test]
    fn verify_rejects_malformed_phc() {
        let err = verify("pw", "not-a-phc-string").unwrap_err();
        assert!(matches!(err, Error::Verify(_)));
    }
}
