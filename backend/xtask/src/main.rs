//! Workspace housekeeping tasks. Slice 0a ships the `check` subcommand that
//! enforces two repo-wide invariants that are too structural for Clippy:
//!
//! * Calculation crates (`orbit-tax-core`, `orbit-tax-rules`, `orbit-tax-spain`)
//!   must not use `std::collections::HashMap` — determinism (SEC-085).
//! * Raw `.acquire()` calls may only appear in `orbit-db/src/tx.rs`, the
//!   authorized home of `Tx::for_user` (SEC-022).
//!
//! Design note: we grep with pure `std`. Pulling in `walkdir` + `regex` would
//! be convenient but would add two workspace deps for a ~80-line scan. Revisit
//! if this file grows.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => run_check(),
        Some(other) => {
            eprintln!("xtask: unknown subcommand `{other}`");
            eprintln!("usage: cargo xtask <check>");
            ExitCode::from(2)
        }
        None => {
            eprintln!("xtask: missing subcommand");
            eprintln!("usage: cargo xtask <check>");
            ExitCode::from(2)
        }
    }
}

fn run_check() -> ExitCode {
    let backend = repo_root().join("backend");
    let mut violations: Vec<String> = Vec::new();

    // SEC-085: no HashMap in calc crates.
    let calc_crates = [
        "crates/orbit-tax-core",
        "crates/orbit-tax-rules",
        "crates/orbit-tax-spain",
    ];
    for rel in calc_crates {
        let dir = backend.join(rel);
        scan_rust_files(&dir, &mut |path, source| {
            for (lineno, line) in source.lines().enumerate() {
                // Match both the fully-qualified path and the typical
                // `use std::collections::HashMap;` form. Avoid flagging the
                // crate's own doc-comment that names the type inside
                // backticks by excluding comment lines.
                if is_comment_only(line) {
                    continue;
                }
                if line.contains("std::collections::HashMap") {
                    violations.push(format!(
                        "{}:{}: forbidden `std::collections::HashMap` in calc crate (SEC-085)",
                        display_path(path),
                        lineno + 1
                    ));
                }
            }
        });
    }

    // SEC-022: no raw `.acquire()` outside the authorized helper.
    let tx_allowed = backend.join("crates/orbit-db/src/tx.rs");
    let xtask_dir = backend.join("xtask");
    scan_rust_files(&backend, &mut |path, source| {
        if same_file(path, &tx_allowed) {
            return;
        }
        // xtask is the checker itself; it contains the literal string
        // ".acquire()" as the needle. Skip its source tree.
        if path.starts_with(&xtask_dir) {
            return;
        }
        for (lineno, line) in source.lines().enumerate() {
            if is_comment_only(line) {
                continue;
            }
            if line.contains(".acquire()") {
                violations.push(format!(
                    "{}:{}: forbidden raw `.acquire()` — only `Tx::for_user` in `orbit-db/src/tx.rs` may acquire a handle (SEC-022)",
                    display_path(path),
                    lineno + 1
                ));
            }
        }
    });

    if violations.is_empty() {
        println!("xtask check: OK ({} calc-crate + acquire checks passed)", calc_crates.len() + 1);
        ExitCode::SUCCESS
    } else {
        eprintln!("xtask check: {} violation(s):", violations.len());
        for v in &violations {
            eprintln!("  {v}");
        }
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    // `cargo xtask` sets CARGO_MANIFEST_DIR to `backend/xtask`; walk up two
    // levels to reach the repo root. Fall back to current dir if unset.
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        return Path::new(&manifest)
            .ancestors()
            .nth(2)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn scan_rust_files<F: FnMut(&Path, &str)>(root: &Path, visit: &mut F) {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            // Skip `target` and hidden dirs.
            if file_type.is_dir() {
                if let Some(name) = path.file_name().and_then(OsStr::to_str) {
                    if name == "target" || name.starts_with('.') {
                        continue;
                    }
                }
                stack.push(path);
                continue;
            }
            if file_type.is_file() && path.extension().and_then(OsStr::to_str) == Some("rs") {
                if let Ok(source) = fs::read_to_string(&path) {
                    visit(&path, &source);
                }
            }
        }
    }
}

fn is_comment_only(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("//!") || trimmed.starts_with("///")
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn display_path(path: &Path) -> String {
    // Prefer a repo-relative display if possible.
    if let Ok(rel) = path.strip_prefix(repo_root()) {
        return rel.display().to_string();
    }
    path.display().to_string()
}
