#!/usr/bin/env bash
#
# Error-typing guard.
#
# The previous attempt flattened seven distinct domain failures into
#   io::Error::new(ErrorKind::Other, "json decoding error")
# discarding the real cause and making failures in a running cluster indistinguishable.
# Each layer defines its own thiserror type and preserves the originating cause instead.
#
# Usage: ./scripts/check-error-types.sh

set -euo pipefail
cd "$(dirname "$0")/.."

CRATES=(crates/recon-core crates/recon-sim crates/recon-protocols)
fail=0

check() {
    local pattern="$1" what="$2"
    for crate in "${CRATES[@]}"; do
        [ -d "$crate" ] || continue
        while IFS= read -r hit; do
            echo "FAIL ($what): $hit"
            fail=1
        done < <(grep -rnE "$pattern" "$crate" --include='*.rs' || true)
    done
}

check 'io::Error::new'          'io::Error used for a domain failure'
check 'ErrorKind::Other'        'ErrorKind::Other discards the real cause'
check 'json decoding error'     'the string this project exists not to reproduce'

if [ "$fail" -ne 0 ]; then
    echo ""
    echo "Define a thiserror type for the layer and carry the cause with #[source]."
    exit 1
fi

echo "PASS: no io::Error domain failures in ${CRATES[*]}"
