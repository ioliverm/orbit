//! HIBP k-anonymity breached-password check (SEC-002).
//!
//! Sends only the first 5 chars of the SHA-1 hex digest to
//! `api.pwnedpasswords.com/range/<prefix>`; the response is a text list of
//! suffixes. No password (raw or hashed) crosses the wire.
//!
//! SEC-003 fail-closed posture: on **any** network / parse / timeout error
//! we treat the password as breached and reject it, rather than accept it
//! on the benefit of the doubt.

use sha1::{Digest, Sha1};

/// Outcome of a HIBP check. The caller maps this to a generic validation
/// error — the SPA only ever sees "password is not acceptable".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HibpCheck {
    /// The password's SHA-1 suffix was not present in the response.
    NotBreached,
    /// The suffix was present; reject.
    Breached,
    /// Network / parse / timeout error. Treated as breached by SEC-003
    /// fail-closed — caller rejects.
    Unavailable,
}

/// Check `password` against HIBP via k-anonymity. Returns within ~500 ms
/// thanks to the client's request timeout; longer runs are treated as
/// [`HibpCheck::Unavailable`].
pub async fn check(client: &reqwest::Client, password: &str) -> HibpCheck {
    let mut hasher = Sha1::new();
    hasher.update(password.as_bytes());
    let digest = hasher.finalize();
    let hex = encode_hex_upper(&digest);
    let (prefix, suffix) = hex.split_at(5);

    let url = format!("https://api.pwnedpasswords.com/range/{prefix}");
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return HibpCheck::Unavailable,
    };
    if !resp.status().is_success() {
        return HibpCheck::Unavailable;
    }
    let body = match resp.text().await {
        Ok(b) => b,
        Err(_) => return HibpCheck::Unavailable,
    };
    for line in body.lines() {
        // Lines look like `SUFFIX:count`. Only the 35-char suffix matters.
        let line_suffix = line.split(':').next().unwrap_or("");
        if line_suffix.eq_ignore_ascii_case(suffix) {
            return HibpCheck::Breached;
        }
    }
    HibpCheck::NotBreached
}

fn encode_hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_prefix_hex_matches_expected() {
        // SHA-1("password") starts with `5BAA6`.
        let mut hasher = Sha1::new();
        hasher.update(b"password");
        let digest = hasher.finalize();
        let hex = encode_hex_upper(&digest);
        assert_eq!(&hex[..5], "5BAA6");
    }
}
