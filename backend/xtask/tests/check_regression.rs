//! Regression tests for `cargo xtask check` (S0-08, SEC-022, SEC-085).
//!
//! These tests verify that the two structural lints in `xtask check` still
//! fire against the violations they are designed to catch:
//!
//!   * **HashMap in a calc crate** — a forbidden `use std::collections::HashMap;`
//!     dropped into `orbit-tax-core/src/lib.rs` must cause `xtask check` to
//!     exit non-zero and name the file on stderr (SEC-085).
//!   * **Raw `.acquire()` outside `orbit-db/src/tx.rs`** — a `.acquire()`
//!     string injected into `orbit-db/src/lib.rs` must cause `xtask check` to
//!     exit non-zero and name the file on stderr (SEC-022).
//!
//! # Why a Rust test and not a shell script?
//!
//! The project is primarily Rust; this test is `cargo test -p xtask` and runs
//! on every platform Orbit builds on. A bash script at `scripts/ci/` would
//! have worked equally well — flagged in the T9 report as the tradeoff — but
//! would have a second language + shell-quoting surface to review. A Rust
//! integration test keeps the whole regression story inside the same test
//! binary matrix that CI already exercises.
//!
//! # Safety: file mutation with a guaranteed revert
//!
//! Each sub-test edits a real source file, runs the checker, and reverts.
//! The revert is handled by an RAII guard (`FileGuard`) whose `Drop` impl
//! rewrites the original bytes even if the test panics part-way through.
//! If both this drop AND the panic-handler fail (rare), the only residue is
//! a single well-known line at the end of one file; `git diff` would show it
//! instantly.
//!
//! # Serial execution
//!
//! The two sub-tests mutate shared files, so they must run one at a time.
//! Rather than pulling in `serial_test`, this file ships a single
//! `#[test]`-annotated function that drives both cases sequentially. Running
//! `cargo test -p xtask` executes them in that single-threaded order.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Repo-root resolution
// ---------------------------------------------------------------------------

/// Locate the repo root from `CARGO_MANIFEST_DIR` (`backend/xtask`), two
/// levels up. Mirrors `xtask::repo_root` so tests and subject-under-test agree
/// on the same notion of "root".
fn repo_root() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by cargo when running tests");
    Path::new(&manifest)
        .ancestors()
        .nth(2)
        .expect("repo root must exist two levels above backend/xtask")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// File guard — restores byte-identical contents on Drop
// ---------------------------------------------------------------------------

/// RAII guard that snapshots a file's bytes on construction and restores them
/// on Drop. The Drop impl is panic-safe; even a panic mid-test cannot leave
/// the repo with the injected violation still on disk.
struct FileGuard {
    path: PathBuf,
    original: Vec<u8>,
}

impl FileGuard {
    fn snapshot(path: &Path) -> Self {
        let original =
            fs::read(path).unwrap_or_else(|e| panic!("snapshot {}: {e}", path.display()));
        FileGuard {
            path: path.to_path_buf(),
            original,
        }
    }

    /// Append text to the end of the file. Returns the guard unchanged so the
    /// caller can keep holding it for the duration of the test.
    fn append(&self, text: &str) {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&self.path)
            .unwrap_or_else(|e| panic!("open append {}: {e}", self.path.display()));
        f.write_all(text.as_bytes())
            .unwrap_or_else(|e| panic!("append to {}: {e}", self.path.display()));
    }
}

impl Drop for FileGuard {
    fn drop(&mut self) {
        // Best-effort restore. We deliberately swallow errors: a panic in
        // Drop would mask the test's own panic and hide the real failure.
        // If the write fails (disk full, permissions), the test output will
        // show the primary failure and the developer will see a clean
        // `git diff` delta on the affected file.
        if let Err(e) = fs::write(&self.path, &self.original) {
            eprintln!(
                "FileGuard::drop: failed to restore {}: {e} — run `git checkout {}` to recover",
                self.path.display(),
                self.path.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Checker invocation
// ---------------------------------------------------------------------------

struct CheckResult {
    status: i32,
    stderr: String,
    #[allow(dead_code)]
    stdout: String,
}

fn run_xtask_check() -> CheckResult {
    let root = repo_root();
    // `cargo run -p xtask -- check` is the invocation CI uses. Using cargo
    // (rather than pre-building and calling the binary by path) guarantees
    // that this test exercises exactly the same code path a developer would
    // run locally. The `--quiet` flag suppresses cargo's own chatter so
    // `stderr` contains only the xtask's output.
    let out = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "-p", "xtask", "--", "check"])
        .current_dir(&root)
        .output()
        .expect("spawn cargo run -p xtask");

    CheckResult {
        status: out.status.code().unwrap_or(-1),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

// ---------------------------------------------------------------------------
// Sub-tests
// ---------------------------------------------------------------------------

/// Baseline: on a clean tree, `xtask check` must succeed. This guards the
/// two injection sub-tests against a false-positive where the checker is
/// already failing for an unrelated reason.
fn assert_baseline_green() {
    let res = run_xtask_check();
    assert_eq!(
        res.status, 0,
        "baseline `xtask check` is failing BEFORE any injection — the regression harness cannot \
         distinguish checker-working-correctly from checker-already-broken. stderr:\n{}",
        res.stderr
    );
}

/// SEC-085: inject `use std::collections::HashMap;` at the bottom of
/// `orbit-tax-core/src/lib.rs` and confirm xtask flags it.
fn assert_hashmap_in_calc_crate_is_flagged() {
    let target = repo_root().join("backend/crates/orbit-tax-core/src/lib.rs");
    let guard = FileGuard::snapshot(&target);

    // Deliberate violation. The newline prefix guarantees we are a fresh
    // top-level line even if the file did not end with a newline.
    guard.append("\nuse std::collections::HashMap;\n");

    let res = run_xtask_check();

    // Restore before asserting so a panic does not leave the file dirty.
    drop(guard);

    assert_ne!(
        res.status, 0,
        "REGRESSION: xtask check exited 0 after a forbidden `std::collections::HashMap` was \
         injected into orbit-tax-core/src/lib.rs. SEC-085 lint is not firing. stderr:\n{}",
        res.stderr
    );
    assert!(
        res.stderr.contains("orbit-tax-core")
            && res.stderr.contains("HashMap"),
        "REGRESSION: xtask check exited non-zero but did not name the HashMap violation in stderr. \
         Expected stderr to mention `orbit-tax-core` and `HashMap`. Got:\n{}",
        res.stderr
    );
}

/// SEC-022: inject a raw `.acquire()` call into `orbit-db/src/lib.rs` (which
/// is NOT the allow-listed `tx.rs`) and confirm xtask flags it.
fn assert_acquire_outside_tx_is_flagged() {
    let target = repo_root().join("backend/crates/orbit-db/src/lib.rs");
    let guard = FileGuard::snapshot(&target);

    // We append a string literal containing `.acquire()` at module scope.
    // The xtask scanner is a substring match on source lines (excluding
    // comment-only lines), so a string literal is sufficient to trip it —
    // which is exactly the fidelity we want to lock in: the checker is a
    // cheap text scanner, not a semantic analysis, and the regression test
    // must exercise the tool as-built. The `#[allow(dead_code)]` keeps the
    // surrounding `cargo check` happy even though the test reverts the file
    // long before the compiler would care.
    guard.append("\n#[allow(dead_code)]\nconst _RLS_REGRESSION_NEEDLE: &str = \"_.acquire()\";\n");

    let res = run_xtask_check();

    drop(guard);

    assert_ne!(
        res.status, 0,
        "REGRESSION: xtask check exited 0 after a raw `.acquire()` was injected into \
         orbit-db/src/lib.rs. SEC-022 lint is not firing. stderr:\n{}",
        res.stderr
    );
    assert!(
        res.stderr.contains("orbit-db") && res.stderr.contains(".acquire()"),
        "REGRESSION: xtask check exited non-zero but did not name the `.acquire()` violation in \
         stderr. Expected stderr to mention `orbit-db` and `.acquire()`. Got:\n{}",
        res.stderr
    );
}

// ---------------------------------------------------------------------------
// Test entry point
// ---------------------------------------------------------------------------

/// Single entry point so the three assertions execute in order on one thread.
/// `cargo test` parallelizes across `#[test]` functions by default; bundling
/// the sequence into one function makes the ordering explicit.
#[test]
fn xtask_check_catches_hashmap_and_raw_acquire() {
    assert_baseline_green();
    assert_hashmap_in_calc_crate_is_flagged();
    assert_acquire_outside_tx_is_flagged();
    // Final sanity: after both reverts the tree must be clean again.
    assert_baseline_green();
}
