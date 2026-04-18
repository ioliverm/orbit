#!/usr/bin/env bash
# Orbit — install local Git hooks.
#
# Installs scripts/dev/pre-commit.sh as .git/hooks/pre-commit via a SYMLINK so
# the hook tracks the in-tree script and is updated when the team edits it.
# Run from anywhere inside the repo.
#
# Safe to re-run; existing hook is replaced only if it already points to our
# script (or does not exist). If the hook is something else, we back it up.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

HOOK_SRC="scripts/dev/pre-commit.sh"
HOOK_DST=".git/hooks/pre-commit"

if [ ! -x "$HOOK_SRC" ]; then
  chmod +x "$HOOK_SRC"
fi

mkdir -p "$(dirname "$HOOK_DST")"

# Compute the path the symlink should store. A relative path keeps things
# portable if .git is moved (worktrees, rare).
# .git/hooks/pre-commit -> ../../scripts/dev/pre-commit.sh
LINK_TARGET="../../$HOOK_SRC"

install_link() {
  ln -s "$LINK_TARGET" "$HOOK_DST"
  echo "installed: $HOOK_DST -> $LINK_TARGET"
}

if [ -L "$HOOK_DST" ]; then
  current="$(readlink "$HOOK_DST")"
  if [ "$current" = "$LINK_TARGET" ]; then
    echo "up to date: $HOOK_DST -> $LINK_TARGET"
    exit 0
  fi
  echo "replacing existing symlink ($current)"
  rm "$HOOK_DST"
  install_link
elif [ -e "$HOOK_DST" ]; then
  backup="${HOOK_DST}.orbit-backup-$(date +%s)"
  echo "existing hook found at $HOOK_DST; backing up to $backup"
  mv "$HOOK_DST" "$backup"
  install_link
else
  install_link
fi

echo
echo "Done. Verify with: git hook list 2>/dev/null || ls -l .git/hooks/pre-commit"
echo "Tools required on PATH: gitleaks, cargo, rustfmt, cargo-clippy."
