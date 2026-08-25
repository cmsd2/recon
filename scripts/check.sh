#!/usr/bin/env bash
#
# Everything that must be clean before a commit.
#
# Usage: ./scripts/check.sh

set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
run() {
    local name="$1"; shift
    printf '\n=== %s ===\n' "$name"
    if "$@"; then
        echo "PASS: $name"
    else
        echo "FAIL: $name"
        fail=1
    fi
}

fmt() { cargo fmt --all --check; }
clippy() { cargo clippy --workspace --all-targets -- -D warnings; }
build() { cargo build --workspace --all-targets; }
tests() { cargo test --workspace; }

run "cargo fmt --check"  fmt
run "cargo clippy -D warnings" clippy
run "cargo build" build
run "cargo test" tests
run "ordered maps" ./scripts/check-ordered-maps.sh
run "error types"  ./scripts/check-error-types.sh
run "no transport" ./scripts/check-no-transport.sh

printf '\n========================================\n'
if [ "$fail" -ne 0 ]; then
    echo "FAILED — do not commit until these are clean."
    exit 1
fi
echo "All checks passed."
