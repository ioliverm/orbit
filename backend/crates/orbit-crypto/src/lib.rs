//! Low-level cryptographic primitives for Orbit.
//!
//! Slice 0a surface:
//!
//!   * [`generate_ip_hash_salt`] — 32-byte CSPRNG salt for the IP-hash helper.
//!   * [`hmac_ip`] — HMAC-SHA256 of an [`IpAddr`] under a given salt (SEC-054).
//!
//! Traces to:
//!   - ADR-011 (auth stack pick: RustCrypto `hmac` + `sha2`, `rand::OsRng`).
//!   - SEC-054 (IP addresses logged only as HMAC-SHA256 under a 32-byte salt;
//!     salt stored in the secret file, rotated annually, old salt retained
//!     90 days for operator pivot).
//!   - Slice-0 security checklist S0-25 (this crate implements the helper;
//!     the audit-log write path consumes it in a later task).

use std::net::IpAddr;

use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Length of the IP-hash salt in bytes.
///
/// Pinned at 32 bytes per SEC-054. The size matches the output block of the
/// underlying HMAC-SHA256, which keys HMAC at its natural width without any
/// internal re-hashing of the key material.
pub const IP_HASH_SALT_LEN: usize = 32;

/// Generate a fresh 32-byte CSPRNG salt for the IP-hash pipeline.
///
/// **Operational contract (SEC-054):** call this **once** at environment
/// provisioning, persist the bytes in the 0600-permissioned secret file, and
/// load the same salt on every process start. **Never rotate the salt without
/// re-hashing the entire `audit_log.ip_hash` column** — rotating in place
/// would silently decorrelate existing rows from future ones.
///
/// Rotation procedure (SEC-032 / SEC-054): generate a new salt, keep the old
/// one for 90 days (so operator pivot on recent rows still works), and during
/// that window hash new writes under the new salt while old rows are rewritten
/// in batches. That rewrite path is owned by a later slice and is **not**
/// part of this helper.
///
/// # Panics
///
/// Panics if the OS CSPRNG fails. `OsRng` is documented as infallible on all
/// supported targets (Linux `getrandom(2)`, macOS `getentropy(2)`); if it ever
/// returns an error the process cannot safely continue issuing session IDs or
/// password salts anyway, so the panic is the right failure mode.
pub fn generate_ip_hash_salt() -> [u8; IP_HASH_SALT_LEN] {
    let mut salt = [0u8; IP_HASH_SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// HMAC-SHA256 of an IP address under `salt`.
///
/// The IP is serialized to its **canonical `Display` form** before hashing so
/// that v4 and v6 addresses produce hashes with the same domain shape and so
/// that an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) does **not** collide
/// with the bare v4 string. This is deliberate: the audit-log consumer treats
/// the two as distinct evidence trails, and canonical `Display` is what the
/// standard library guarantees to be round-trippable.
///
/// # Stability
///
/// The hash domain is fixed: `HMAC-SHA256(salt, utf8_bytes_of(ip.to_string()))`.
/// Any change here is a migration event for `audit_log.ip_hash`.
pub fn hmac_ip(salt: &[u8; IP_HASH_SALT_LEN], ip: IpAddr) -> [u8; 32] {
    // `new_from_slice` cannot fail for HMAC-SHA256 with any key length, but we
    // use `expect` to be explicit about the contract.
    let mut mac =
        HmacSha256::new_from_slice(salt).expect("HMAC-SHA256 accepts arbitrary-length keys");
    mac.update(ip.to_string().as_bytes());
    let out = mac.finalize().into_bytes();
    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn salt_is_exactly_32_bytes_and_non_zero() {
        // (a) Pin the salt length. The array type already enforces this at
        //     compile-time, but the test guards against a future refactor that
        //     silently changes the constant.
        let salt = generate_ip_hash_salt();
        assert_eq!(salt.len(), IP_HASH_SALT_LEN);
        assert_eq!(salt.len(), 32);
        // A fresh OS CSPRNG salt being all-zero is a ~2^-256 event; if we see
        // it we have bigger problems than a flaky test.
        assert_ne!(salt, [0u8; IP_HASH_SALT_LEN], "OsRng returned all zeros");
    }

    #[test]
    fn same_salt_and_ip_produces_same_hash() {
        // (b) Determinism: same (salt, ip) → same hash.
        let salt = generate_ip_hash_salt();
        let ip: IpAddr = Ipv4Addr::new(203, 0, 113, 7).into();
        let a = hmac_ip(&salt, ip);
        let b = hmac_ip(&salt, ip);
        assert_eq!(a, b);
    }

    #[test]
    fn different_salts_produce_different_hashes_for_same_ip() {
        // (c) Domain separation: the salt actually matters.
        let salt_a = generate_ip_hash_salt();
        let salt_b = generate_ip_hash_salt();
        assert_ne!(salt_a, salt_b, "two OS CSPRNG draws collided");
        let ip: IpAddr = Ipv4Addr::new(203, 0, 113, 7).into();
        assert_ne!(hmac_ip(&salt_a, ip), hmac_ip(&salt_b, ip));
    }

    #[test]
    fn v4_and_v6_hash_distinctly() {
        // Canonical `Display` means 192.0.2.1 and ::ffff:192.0.2.1 are
        // distinct inputs, which is the documented contract.
        let salt = [7u8; 32];
        let v4: IpAddr = Ipv4Addr::new(192, 0, 2, 1).into();
        let v6: IpAddr = Ipv6Addr::from([0, 0, 0, 0, 0, 0xffff, 0xc000, 0x0201]).into();
        assert_ne!(hmac_ip(&salt, v4), hmac_ip(&salt, v6));
    }
}
