//! Structured, allowlist-gated logging for Orbit. (SEC-050)
//!
//! The [`event!`] macro is the **only** sanctioned path to emit a log line in
//! application code. It accepts a level, a static message, and zero or more
//! `key = value` fields. Every value expression is required to implement the
//! sealed [`SafeToLog`] trait at compile time. Sensitive product types
//! (`Money`, `Grant`, `Scenario`, `Calculation`, `SellNowInput`, `Export`)
//! deliberately do **not** implement `SafeToLog`; attempting to log one is a
//! compile error.
//!
//! # Design notes
//!
//! * **No `tracing` in Slice 0a.** A full `tracing` + `tracing-subscriber::fmt::json()`
//!   stack is the correct Slice 0b wiring (per ADR-013 §Observability). For
//!   Slice 0a we only need the compile-time allowlist; the emission backend
//!   is a thin hand-rolled JSON writer so that this crate has **zero runtime
//!   deps**. Swapping the body of [`emit`] for a `tracing::event!` call is a
//!   mechanical change that will not affect call sites.
//! * **Sealed trait, not proc-macro.** `SafeToLog` lives in a private module
//!   so downstream crates cannot broaden the allowlist by accident. A custom
//!   derive can be added later in a sub-crate if we grow structured fields.
//! * **`&'static str` is `SafeToLog`; `&str` is not.** A non-`'static` string
//!   reference is very likely to hold user-provided or PII data (NIF/NIE,
//!   notes, email). Callers that genuinely need to log a dynamic string must
//!   wrap it with [`SafeString`] and accept responsibility at the call site.
//! * **`SafeString` is a newtype, not a blanket `impl`.** It is a deliberate
//!   seam for code review: every `SafeString::new(...)` is a place where
//!   SEC-050 was weighed.
//!
//! # Example
//!
//! ```
//! use orbit_log::{event, Level, SafeString};
//!
//! event!(Level::Info, "request_completed",
//!     request_id = 0u128,
//!     route = "/healthz",
//!     status = 200u64,
//!     cache_hit = true
//! );
//!
//! // Dynamic strings must go through SafeString:
//! let safe = SafeString::new("fixed-kind-tag".to_string());
//! event!(Level::Info, "component_ready", kind = safe);
//! ```

use std::fmt::Write as _;
use std::io::Write as _;

// ---------------------------------------------------------------------------
// Sealed trait
// ---------------------------------------------------------------------------

mod sealed {
    /// Seal: only types listed in this crate may implement `SafeToLog`.
    pub trait Sealed {}
}

/// Marker + encoder for values that are safe to log per SEC-050.
///
/// Implementors are curated in this crate. The trait is sealed, so downstream
/// crates cannot add new log-safe types without editing `orbit-log`. If you
/// need to log a dynamic string, wrap it with [`SafeString`].
pub trait SafeToLog: sealed::Sealed {
    /// Write the value as a JSON fragment into `out`.
    fn log_encode(&self, out: &mut String);
}

// ---------------------------------------------------------------------------
// Permitted primitive implementations
// ---------------------------------------------------------------------------

macro_rules! impl_safe_integer {
    ($($t:ty),+ $(,)?) => {
        $(
            impl sealed::Sealed for $t {}
            impl SafeToLog for $t {
                fn log_encode(&self, out: &mut String) {
                    let _ = write!(out, "{}", self);
                }
            }
        )+
    };
}

impl_safe_integer!(u8, u16, u32, u64, i8, i16, i32, i64, u128, i128, usize, isize);

impl sealed::Sealed for bool {}
impl SafeToLog for bool {
    fn log_encode(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

// Only `&'static str` is SafeToLog. Non-static `&str` is rejected at compile
// time because this impl requires the `'static` lifetime bound.
impl sealed::Sealed for &'static str {}
impl SafeToLog for &'static str {
    fn log_encode(&self, out: &mut String) {
        encode_json_string(self, out);
    }
}

// ---------------------------------------------------------------------------
// SafeString: explicit opt-in wrapper for dynamic strings
// ---------------------------------------------------------------------------

/// A string the caller has affirmed is safe to log under SEC-050.
///
/// Every construction is a review seam. Do not use `SafeString::new` to
/// launder grant identifiers, NIF/NIE, user notes, tickers, or anything else
/// that could leak Financial-Personal data.
#[derive(Debug, Clone)]
pub struct SafeString(String);

impl SafeString {
    /// Wrap a string the caller asserts is safe to log.
    pub fn new(value: String) -> Self {
        SafeString(value)
    }

    /// The underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl sealed::Sealed for SafeString {}
impl SafeToLog for SafeString {
    fn log_encode(&self, out: &mut String) {
        encode_json_string(&self.0, out);
    }
}

// Slice 0b will add `impl SafeToLog for Uuid` and `impl SafeToLog for
// DateTime<Utc>` once we wire the `uuid` + `chrono` crates; today the
// integer and bool impls + `SafeString` are enough for every existing call
// site (`request_id` is rendered as `u128`, timestamps via `i64` epoch ms).

// ---------------------------------------------------------------------------
// Level
// ---------------------------------------------------------------------------

/// Log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

// ---------------------------------------------------------------------------
// Compile-time gate
// ---------------------------------------------------------------------------

/// Identity function that forces `T: SafeToLog`. Used by the `event!` macro
/// to anchor the compile-time allowlist at the field site.
#[inline(always)]
#[doc(hidden)]
pub fn assert_safe_to_log<T: ?Sized + SafeToLog>(v: &T) -> &T {
    v
}

// ---------------------------------------------------------------------------
// Emission
// ---------------------------------------------------------------------------

/// Emit a structured JSON log line to stderr.
///
/// Public so the `event!` macro can call it. Not intended for direct use.
#[doc(hidden)]
pub fn emit(level: Level, message: &'static str, fields: &[(&'static str, &dyn SafeToLog)]) {
    let mut out = String::with_capacity(128);
    out.push('{');
    out.push_str("\"level\":\"");
    out.push_str(level.as_str());
    out.push_str("\",\"message\":");
    encode_json_string(message, &mut out);
    for (k, v) in fields {
        out.push(',');
        encode_json_string(k, &mut out);
        out.push(':');
        v.log_encode(&mut out);
    }
    out.push('}');
    out.push('\n');
    let _ = std::io::stderr().write_all(out.as_bytes());
}

fn encode_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------------------
// event! macro
// ---------------------------------------------------------------------------

/// Emit a structured log line with allowlist-gated fields.
///
/// Every `value` expression must implement [`SafeToLog`]; this is checked at
/// compile time. The macro expands to a single [`emit`] call.
///
/// ```
/// use orbit_log::{event, Level};
/// event!(Level::Info, "request_completed", status = 200u64, ok = true);
/// ```
#[macro_export]
macro_rules! event {
    // No fields.
    ($level:expr, $message:expr $(,)?) => {{
        // Force `$message` to be a `&'static str`.
        const _MSG: &'static str = $message;
        $crate::emit($level, _MSG, &[]);
    }};
    // One or more `key = value` fields.
    ($level:expr, $message:expr, $( $key:ident = $value:expr ),+ $(,)?) => {{
        const _MSG: &'static str = $message;
        $crate::emit(
            $level,
            _MSG,
            &[
                $(
                    (
                        stringify!($key),
                        $crate::assert_safe_to_log(&$value) as &dyn $crate::SafeToLog,
                    ),
                )+
            ],
        );
    }};
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_valid_json_with_no_fields() {
        // We can't capture stderr easily here; exercise the encoder directly.
        let mut out = String::new();
        encode_json_string("hello", &mut out);
        assert_eq!(out, "\"hello\"");
    }

    #[test]
    fn integers_encode_as_numbers() {
        let mut out = String::new();
        42u64.log_encode(&mut out);
        assert_eq!(out, "42");
    }

    #[test]
    fn bool_encodes_as_json_bool() {
        let mut out = String::new();
        true.log_encode(&mut out);
        assert_eq!(out, "true");
    }

    #[test]
    fn safe_string_escapes_special_chars() {
        let mut out = String::new();
        SafeString::new("a\"b\nc".to_string()).log_encode(&mut out);
        assert_eq!(out, "\"a\\\"b\\nc\"");
    }

    #[test]
    fn static_str_accepted() {
        // Compiles because `&'static str: SafeToLog`.
        event!(Level::Info, "hello", tag = "ok");
    }

    #[test]
    fn trybuild_compile_fail() {
        // Run trybuild compile-fail fixtures. Each one is a tiny program that
        // must NOT compile because it tries to log a sensitive or
        // non-'static value.
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/compile_fail/*.rs");
    }
}
