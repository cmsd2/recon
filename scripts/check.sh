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

# Documentation guard.
#
# A broken intra-doc link is a docstring naming something that is not there, and this project's
# method is reading code against its quoted contract — so a stale reference asserts a contract the
# code does not have. It is silent: nothing fails, the link just renders as text.
#
# It had already happened when this check was added. `Cmd::Start` was documented in two modules as
# the way to begin a broadcast after the command had been removed; `perfect_link` still explained
# its relationship to a `ScopedLink` trait deleted a change earlier; `Link::delivered` had been
# renamed `classify`. Eight complaints in all, four of them stale prose.
docs() { RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet; }

run "cargo fmt --check"  fmt
run "cargo clippy -D warnings" clippy
run "cargo build" build
run "cargo test" tests
run "cargo doc -D warnings" docs
run "ordered maps" ./scripts/check-ordered-maps.sh
run "error types"  ./scripts/check-error-types.sh
run "no transport" ./scripts/check-no-transport.sh

printf '\n========================================\n'
if [ "$fail" -ne 0 ]; then
    echo "FAILED — do not commit until these are clean."
    exit 1
fi
echo "All checks passed."
