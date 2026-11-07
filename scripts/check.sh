#!/usr/bin/env bash
# Pre-commit gate for the workspace. Usage: scripts/check.sh [--full]
#   default: fmt, clippy (warnings denied), tests, CLI end-to-end scenario
#   --full:  also cargo-deny and a release build size report
set -euo pipefail
cd "$(dirname "$0")/.."

full=false
[[ "${1:-}" == "--full" ]] && full=true

step() { printf '\n== %s\n' "$1"; }

step "fmt"
cargo fmt --all -- --check

step "clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings

step "test"
if command -v cargo-nextest >/dev/null; then
    cargo nextest run --workspace --locked
    cargo test --workspace --doc --locked
else
    cargo test --workspace --locked
fi

step "cli end-to-end"
scripts/e2e-cli.sh

if $full; then
    step "deny"
    if command -v cargo-deny >/dev/null; then
        cargo deny check
    else
        # A skipped check must never look like a passed one.
        echo "SKIPPED: cargo-deny is not installed (cargo install cargo-deny --locked)" >&2
        exit 2
    fi

    step "release size"
    cargo build --release --locked -p cominorsa-desktop
    ls -l target/release/cominorsa-desktop | awk '{printf "%.1f MB\n", $5 / 1048576}'
fi

printf '\nAll checks passed.\n'
