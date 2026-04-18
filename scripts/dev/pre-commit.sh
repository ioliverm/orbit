#!/usr/bin/env bash
# Orbit — local pre-commit hook.
#
# Covers security-checklist item S0-01 (gitleaks as a pre-commit hook) and the
# fast Rust lints. Full policy gates (tests, audit, deny, sbom) live in CI —
# this hook trades off strictness for speed so commits stay sub-second when
# nothing Rust-y changed.
#
# Install via: scripts/dev/install-hooks.sh
# Manual run:  scripts/dev/pre-commit.sh
#
# Exit codes:
#   0  all checks passed
#   1  a check failed (message printed, commit aborted)
#
# Required tools on PATH: gitleaks, cargo, rustfmt, cargo-clippy.
# Missing tools are reported as errors — we do NOT silently skip security
# checks (S0-01). Install via: brew install gitleaks / rustup component add.

set -euo pipefail

# Resolve repo root regardless of where the hook was invoked from.
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# Colors if stdout is a TTY.
if [ -t 1 ]; then
  BOLD=$'\033[1m'; RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RESET=$'\033[0m'
else
  BOLD=""; RED=""; GREEN=""; YELLOW=""; RESET=""
fi

step() { echo "${BOLD}==>${RESET} $*"; }
fail() { echo "${RED}pre-commit: $*${RESET}" >&2; exit 1; }
warn() { echo "${YELLOW}pre-commit: $*${RESET}" >&2; }

require() {
  command -v "$1" >/dev/null 2>&1 \
    || fail "missing required tool '$1'. See scripts/dev/pre-commit.sh header for install hints."
}

# ----------------------------------------------------------------------------
# 1. gitleaks on staged content (S0-01).
# ----------------------------------------------------------------------------
step "gitleaks protect --staged"
require gitleaks
gitleaks protect --staged --redact --no-banner \
  || fail "gitleaks found a secret in staged changes. Unstage/rewrite, then retry."

# ----------------------------------------------------------------------------
# 2. Fast Rust lints — only if any staged file touches Rust sources or Cargo.
# ----------------------------------------------------------------------------
# Detect staged Rust-relevant files. `git diff --cached --name-only` yields
# paths relative to the repo root, including deletions — filter with -z to
# handle unusual paths safely.
rust_touched=false
while IFS= read -r -d '' path; do
  case "$path" in
    *.rs | Cargo.toml | Cargo.lock | rust-toolchain.toml | deny.toml | */Cargo.toml)
      rust_touched=true
      break
      ;;
  esac
done < <(git diff --cached --name-only -z --diff-filter=ACMR)

if [ "$rust_touched" = true ]; then
  step "cargo fmt --check"
  require cargo
  cargo fmt --all -- --check \
    || fail "cargo fmt found unformatted code. Run: cargo fmt --all"

  step "cargo clippy -- -D warnings"
  cargo clippy --workspace --all-targets -- -D warnings \
    || fail "cargo clippy reported warnings. Fix them, then re-stage and retry."
else
  warn "no Rust files staged — skipping cargo fmt / clippy."
fi

echo "${GREEN}pre-commit: all checks passed.${RESET}"
