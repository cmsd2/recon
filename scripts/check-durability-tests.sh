#!/usr/bin/env bash
#
# Durability-evidence guard.
#
# A test that says something survived a restart should fail when nothing did. One that passes
# anyway is reading the network, not the disk — and in this project the network is never empty,
# because the stubborn children retransmit everything they have ever sent on every tick, so the
# backlog in flight at any instant holds a full replayable copy of the run. A process that crashes
# and comes back into that backlog is repopulated by its peers within one settle, and a durability
# assertion made afterwards cannot tell that apart from a record read back off the disk.
#
# So this breaks the thing and requires the red. `--features lose-storage-on-restart` makes
# `Sim::restart` discard what was written, and every test named below must then fail. Reading
# cannot substitute for it: the audit that produced this list found two tests with identical
# structure and intent where only one actually leaked, three tests whose stated purpose was
# durability and which could not have detected its absence — including the one guarding the
# composed durable record, whose own comment says a half-written record would go unnoticed until
# recovery — and a leak in a test written the day before specifically to close this hole.
#
# Adding a durability test: run this, see it named under "detects it but is not registered", and
# add it. Losing a name from the failures is the guard firing.
#
# Not part of ./scripts/check.sh: it rebuilds the crate under a feature and runs the suites again,
# which is a minute rather than the seconds every other guard costs. Run it when touching recovery,
# storage, or any test that crashes and restarts a process.
#
# Usage: ./scripts/check-durability-tests.sh

set -euo pipefail
cd "$(dirname "$0")/.."

# Every test that must notice when a restart finds nothing written. One name per line.
REGISTERED=$(cat <<'NAMES'
a_process_that_decided_still_holds_that_decision_after_a_crash_and_recovery
a_process_that_dies_inside_a_write_recovers_consistently
a_process_that_log_delivers_crashes_and_recovers_does_not_deliver_twice
a_recovered_process_answers_a_read_with_what_it_accepted
a_recovered_process_appends_something_new
a_recovered_process_broadcasting_something_new_does_not_reuse_an_identifier
a_restarted_process_does_not_enter_a_timestamp_it_has_entered_before
a_restarted_sender_does_not_reuse_an_identifier_the_recipient_has_logged
a_retransmission_arriving_straight_after_recovery_is_recognised
acknowledgements_are_never_written_down
agreement_holds_across_crashes_and_recoveries
dying_inside_the_decision_write_never_leaves_a_decision_announced_without_a_record
dying_inside_the_epoch_write_never_leaves_an_epoch_announced_without_a_record
dying_inside_the_write_never_leaves_a_promise_without_a_record
dying_inside_the_write_never_leaves_an_acceptance_announced_without_a_record
dying_inside_the_write_never_log_delivers_without_a_record
no_duplication_holds_across_a_restart
recovery_re_announces_the_log_with_no_message_having_arrived
recovery_re_announces_what_was_already_log_delivered
recovery_reads_re_announces_and_re_broadcasts_with_nothing_in_between
that_run_really_contained_all_three
the_log_is_durable_before_the_announcement_even_across_a_crash
the_ordered_sequence_survives_a_restart
the_parent_and_both_children_keep_their_own_part_of_one_record
the_recovered_process_reads_its_epoch_back_rather_than_starting_again
NAMES
)

out=$(mktemp)
trap 'rm -f "$out"' EXIT

echo "Running the suites with storage lost on every restart..."
# The suites are expected to fail; a non-zero status here is the point, not an error. What would be
# an error is failing to build, which is why the compiler's own complaints are still surfaced.
cargo test -p recon-protocols --features lose-storage-on-restart --no-fail-fast >"$out" 2>&1 || true

# `error: test failed` is the expected outcome and says nothing is wrong. A compiler diagnostic, or
# no suite having run at all, means the audit measured nothing and must not report success.
if grep -qE '^error\[|^error: could not compile' "$out"; then
    echo "FAIL: the suites did not build under the feature"
    grep -E '^error\[|^error: could not compile' "$out" | head -20
    exit 1
fi
if ! grep -q '^test result:' "$out"; then
    echo "FAIL: no suite ran, so nothing was measured"
    tail -20 "$out"
    exit 1
fi

detected=$(grep -E '^test .* FAILED$' "$out" | sed 's/^test //; s/ \.\.\. FAILED$//' | sort -u)
registered=$(echo "$REGISTERED" | sort -u)

fail=0

missing=$(comm -23 <(echo "$registered") <(echo "$detected"))
if [ -n "$missing" ]; then
    echo
    echo "FAIL: registered as durability evidence, but passed with storage lost."
    echo "      Each of these asserts something survived a restart, and would not have noticed"
    echo "      that nothing did. What it is really reading is the network."
    echo "$missing" | sed 's/^/  - /'
    fail=1
fi

extra=$(comm -13 <(echo "$registered") <(echo "$detected"))
if [ -n "$extra" ]; then
    echo
    echo "NOTE: detects it but is not registered. Add to this script if durability is the claim:"
    echo "$extra" | sed 's/^/  + /'
fi

echo
if [ "$fail" -ne 0 ]; then
    echo "FAIL: durability evidence"
    exit 1
fi
echo "PASS: all $(echo "$registered" | wc -l | tr -d ' ') registered tests notice when storage is lost"
