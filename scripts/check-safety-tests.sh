#!/usr/bin/env bash
#
# Safety-evidence guard, for the Synod protocol.
#
# The same instrument as `check-durability-tests.sh`, pointed at a different claim. A durability
# test that passes when storage is wiped is reading the network; an agreement test that passes when
# the leader stops honouring what the majority told it is reading nothing at all. Agreement admits
# the substitution particularly easily: "at most one proposal is chosen per slot" is satisfied by a
# run that chooses nothing, and "no two processes disagree" by a run with one settled leader —
# whatever the code underneath does. So the way to know a safety test is evidence is to break the
# thing it claims to protect and require the red.
#
# Two mutations, each removing exactly one clause the argument in §2.4 of *Paxos Made Moderately
# Complex* rests on, and each leaving everything else working so that a failure names one cause:
#
#   synod-ignore-pmax             the leader proposes what it set out to propose, rather than the
#                                 highest-ballot pvalue the majority reported. This is the step the
#                                 whole safety argument rests on, so a suite that survives it is
#                                 not testing agreement.
#   synod-accept-below-promise    an acceptor accepts under a ballot it has already superseded, and
#                                 answers as though it had not. Invariant A1 is untouched, so
#                                 exactly one invariant is removed. Without the promise, two
#                                 majorities need not agree.
#
# Registered per mutation rather than in one list, because the two are evidence for different
# clauses and a test that cannot detect one may be perfectly good evidence for the other. What the
# guard forbids is a registered test staying green: that means the property it names is not the
# property it checks.
#
# Adding a safety test: run this, see it named under "detects it but is not registered", and add
# it under the mutation it caught. Losing a name from the failures is the guard firing.
#
# Not part of ./scripts/check.sh: it rebuilds the crate under two features and runs the suite twice.
# Run it when touching `multi_paxos_synod.rs` or its suite.
#
# Usage: ./scripts/check-safety-tests.sh

set -euo pipefail
cd "$(dirname "$0")/.."

SUITE=multi_paxos_synod

# Every test that must go red when the leader ignores what the majority reported.
REGISTERED_ignore_pmax=$(cat <<'NAMES'
a_later_ballot_proposes_what_an_earlier_majority_accepted
a_majority_that_has_taken_up_a_ballot_cannot_afterwards_accept_a_lower_one
a_value_chosen_under_a_crashed_leader_is_what_its_successor_proposes
at_most_one_proposal_is_chosen_per_slot_under_competing_ballots
duelling_leaders_are_permitted_to_choose_nothing
safety_holds_across_a_sweep_of_seeds
NAMES
)

# Every test that must go red when an acceptor accepts below its own promise.
REGISTERED_accept_below_promise=$(cat <<'NAMES'
a_majority_that_has_taken_up_a_ballot_cannot_afterwards_accept_a_lower_one
NAMES
)

out=$(mktemp)
trap 'rm -f "$out"' EXIT
fail=0

audit() {
    local feature=$1 registered=$2

    echo
    echo "Running the Synod suite with --features $feature ..."
    # The suite is expected to fail; a non-zero status is the point. What would be an error is
    # failing to build, which is why the compiler's own complaints are still surfaced.
    cargo test -p recon-protocols --features "$feature" --test "$SUITE" --no-fail-fast \
        >"$out" 2>&1 || true

    if grep -qE '^error\[|^error: could not compile' "$out"; then
        echo "FAIL: the suite did not build under $feature"
        grep -E '^error\[|^error: could not compile' "$out" | head -20
        fail=1
        return
    fi
    if ! grep -q '^test result:' "$out"; then
        echo "FAIL: no suite ran under $feature, so nothing was measured"
        tail -20 "$out"
        fail=1
        return
    fi

    local detected
    detected=$(grep -E '^test .* FAILED$' "$out" | sed 's/^test //; s/ \.\.\. FAILED$//' | sort -u)
    registered=$(echo "$registered" | sort -u)

    local missing extra
    missing=$(comm -23 <(echo "$registered") <(echo "$detected"))
    if [ -n "$missing" ]; then
        echo
        echo "FAIL: registered as safety evidence for $feature, but passed with the clause removed."
        echo "      Each of these names a property the mutation genuinely breaks, and would not"
        echo "      have noticed. What it is really asserting is something weaker."
        echo "$missing" | sed 's/^/  - /'
        fail=1
    fi

    extra=$(comm -13 <(echo "$registered") <(echo "$detected"))
    if [ -n "$extra" ]; then
        echo
        echo "NOTE: detects $feature but is not registered. Add it if safety is the claim:"
        echo "$extra" | sed 's/^/  + /'
    fi

    if [ -z "$missing" ]; then
        echo "  all $(echo "$registered" | wc -l | tr -d ' ') registered tests noticed $feature"
    fi
}

audit synod-ignore-pmax "$REGISTERED_ignore_pmax"
audit synod-accept-below-promise "$REGISTERED_accept_below_promise"

echo
if [ "$fail" -ne 0 ]; then
    echo "FAIL: safety evidence"
    exit 1
fi
echo "PASS: every registered test goes red when the clause it depends on is removed"
