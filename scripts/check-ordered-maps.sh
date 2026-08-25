#!/usr/bin/env bash
#
# Determinism guard.
#
# Rust's default hasher is randomly seeded per process, so iterating a HashMap or HashSet
# yields a different order on every run. A protocol that picks gossip targets or fans out a
# broadcast by iterating one is unreproducible even with a seeded RNG — and it fails silently,
# which defeats the entire premise of the simulator.
#
# Ordered maps only, in the crates whose behaviour must replay identically.
#
# Usage: ./scripts/check-ordered-maps.sh

set -euo pipefail
cd "$(dirname "$0")/.."

CRATES=(crates/recon-core crates/recon-sim crates/recon-protocols)
PATTERN='\b(HashMap|HashSet)\b'

fail=0
for crate in "${CRATES[@]}"; do
    [ -d "$crate" ] || continue
    while IFS= read -r hit; do
        # An explicitly seeded hasher is fine; the ban is on the randomly-seeded default.
        case "$hit" in
            *allow-hashmap*) continue ;;
        esac
        echo "FAIL: $hit"
        fail=1
    done < <(grep -rnE "$PATTERN" "$crate" --include='*.rs' || true)
done

if [ "$fail" -ne 0 ]; then
    echo ""
    echo "Use BTreeMap / BTreeSet instead. Iteration order of the std hash containers varies"
    echo "between processes and breaks seed reproducibility."
    echo "If a hash container is genuinely required, justify it and mark the line 'allow-hashmap'."
    exit 1
fi

echo "PASS: no HashMap/HashSet in ${CRATES[*]}"
