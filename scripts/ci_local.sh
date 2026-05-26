#!/usr/bin/env bash
# Local CI mirror — runs the exact gates from .github/workflows/ci.yml.
#
# Use when GitHub Actions is unavailable (billing / outage) OR before pushing
# to catch failures faster than the CI feedback loop.
#
# Usage:  bash scripts/ci_local.sh
#
# Honors $SAGE_SKIP_NO_DEV_DEPS=1 to skip the cargo-hack check if not installed.
set -euo pipefail

cd "$(dirname "$0")/.."

say() { printf "\n\033[1;36m[ci_local]\033[0m %s\n" "$1"; }
ok()  { printf "\033[1;32m[ci_local] ✓ %s\033[0m\n" "$1"; }

say "1/6 cargo fmt --check"
cargo fmt --all -- --check
ok "fmt clean"

say "2/6 cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings
ok "clippy default clean"

say "3/6 cargo clippy --all-features"
cargo clippy --workspace --all-targets --all-features -- -D warnings
ok "clippy --all-features clean"

say "4/6 cargo test --workspace --all-features"
cargo test --workspace --all-features --quiet
ok "tests green"

say "5/6 CLAUDE.md presence + index freshness (CONSTITUTION §3.5)"
cargo run --quiet -p gen-claude-index -- --presence
cargo run --quiet -p gen-claude-index -- --check
ok "CLAUDE.md hierarchy clean"

if [[ "${SAGE_SKIP_NO_DEV_DEPS:-0}" == "1" ]]; then
    say "6/6 cargo hack --no-dev-deps — SKIPPED (SAGE_SKIP_NO_DEV_DEPS=1)"
elif command -v cargo-hack >/dev/null 2>&1; then
    say "6/6 cargo hack check --no-dev-deps (CONSTITUTION §2.4)"
    cargo hack check --workspace --no-dev-deps
    ok "no-dev-deps src compiles"
else
    printf "\033[1;33m[ci_local] ⚠ cargo-hack not installed; skipping §2.4 check.\n"
    printf "    Install with: cargo install cargo-hack --locked\033[0m\n"
fi

printf "\n\033[1;32m[ci_local] all local CI gates green ✓\033[0m\n"
