#!/usr/bin/env bash
set -euo pipefail

# ── Resolve repo root ────────────────────────────────────────────────
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export REPO_ROOT
CLI_DIR="$REPO_ROOT/dev/cli"
BIN="$CLI_DIR/target/release/dev-cli"

# ── Colors ───────────────────────────────────────────────────────────
red()   { printf '\033[0;31m%s\033[0m\n' "$*"; }
green() { printf '\033[0;32m%s\033[0m\n' "$*"; }
dim()   { printf '\033[0;90m%s\033[0m\n' "$*"; }

# ── Health checks ────────────────────────────────────────────────────
fail=0
for cmd in docker git cargo; do
  if ! command -v "$cmd" &>/dev/null; then
    red "Missing: $cmd"
    fail=1
  fi
done
if [ "$fail" -ne 0 ]; then
  red "Install missing tools and retry."
  exit 1
fi

# ── Smart rebuild ────────────────────────────────────────────────────
needs_build() {
  [ ! -f "$BIN" ] && return 0

  # Rebuild if any source file is newer than the binary
  while IFS= read -r -d '' f; do
    if [ "$f" -nt "$BIN" ]; then
      return 0
    fi
  done < <(find "$CLI_DIR/src" -name '*.rs' -print0 2>/dev/null)

  # Rebuild if Cargo.toml changed
  [ "$CLI_DIR/Cargo.toml" -nt "$BIN" ] && return 0

  return 1
}

if needs_build; then
  dim "Building dev-cli..."
  cargo build --release --manifest-path "$CLI_DIR/Cargo.toml" --quiet
  green "dev-cli built."
fi

# ── Exec ─────────────────────────────────────────────────────────────
exec "$BIN" "$@"
