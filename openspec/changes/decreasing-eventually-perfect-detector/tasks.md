## 1. The detector port

- [x] 1.1 Add `Detector` and `DetectorInd { Suspect, Restore }` — what a layer above a detector may
      depend on, and the whole of it. Say in the module why it exists now and not before: two
      detectors is the second consumer constraint 4 asks for
- [x] 1.2 `perfect_failure_detector` satisfies it, classifying its `Crash` as a suspicion and never
      producing a withdrawal. Verify from a test that it never does

## 2. ◇P — Algorithm 2.7, then the two departures

- [x] 2.1 Transcribe Algorithm 2.7 into the module docstring from the indexed book, and verify the
      guards against the page rather than a recollection — the OCR drops the negations in
      `if (p ∉ alive) ∧ (p ∉ suspected)`, and reading them as written inverts the algorithm
- [x] 2.2 Implement it, with `Suspect` and `Restore`, and verify a crash is suspected everywhere and
      never retracted while it lasts
- [x] 2.3 Verify a correct process suspected under withdrawn synchrony is **restored** when heard
      from again, and that a recovered process is restored
- [x] 2.4 Verify nothing is reported when the suspected set does not change between rounds
- [x] 2.5 Add the decrease: down one `Δ` after `quiet_rounds` clean rounds, never below the floor.
      Verify a false suspicion raises the timeout and sustained accuracy lowers it.
      **"Clean round" had to be redefined, and the first definition was wrong.** Easing off after
      rounds with no suspicion *withdrawn* decreases the delay precisely when the detector is being
      consistently wrong — a network bad enough that suspicions are never taken back produces no
      withdrawals at all. Measured: against a network twelve times the initial delay the delay
      drifted back to the floor instead of holding near the cap. The condition is now **nothing
      suspected**: no outstanding claim that could be wrong, and the network evidently keeping up.
      A crashed peer therefore freezes the delay where it reached, which is stated and is better
      than either the ratchet or easing off blindly
- [x] 2.6 Verify the timeout does not thrash — drive a latency that steps up and then down, and
      assert the delay follows, rising faster than it falls, and settles
- [x] 2.7 Add the cap. Verify the timeout stops at `max_delay`, and that accuracy is then lost when
      the network settles above it — the stated condition failing, asserted rather than described.
      The cap is a **ceiling, not a resting place**: the delay reaches it and the decrease then pulls
      it back once suspicions clear, so the test samples the trajectory rather than reading a final
      value
- [x] 2.8 Sweep the cap against a latency distribution and assert the shape: false suspicions rise
      as the cap falls below the true delay. This is what makes the trade a measurement rather than
      a number someone picked, and its own non-vacuity check
- [x] 2.9 Verify state is bounded by membership and the send rate is flat in time

## 3. Ω over the port

- [x] 3.1 Make `EventualLeaderDetector` generic over the port, **still defaulting to `P`**: no
      behaviour change, every existing suite unchanged and passing
- [x] 3.2 Handle `Restore`: `suspected` shrinks, and `maxrank` may climb. Verify trust returns to a
      restored process that outranks the incumbent, and that a restoration which does not change the
      leader raises nothing
- [x] 3.3 Verify a recovered process can lead again — the thing `P` makes impossible, and the reason
      for this change
- [x] 3.4 Flip the default to `◇P` and run everything above. Record what moves. **Nothing moved
      that a test could see** — 521 of 521 passed unchanged, no edit to any suite. What the suites
      could not see is in 5.2: they exercise crash-and-restart, where a restarted Ω trusts afresh
      and the edge exists, and never a healed partition, where it does not

## 4. What must not move

- [x] 4.1 `uniform_reliable_broadcast` and `flooding_consensus` keep `P`. Add a line to each saying
      why: their agreement rests on strong accuracy by name, and a detector allowed to be wrong
      would break it — which is what the majority-ack broadcast beside them exists to avoid

## 5. The stack above, under a detector that retracts

- [x] 5.1 Run the epoch-change, Paxos and logged-Paxos suites against the new default and fix or
      record every difference. **Every existing test passed unchanged** — 521 of 521, with no edit
      to any suite above Ω. The churn test did not speak. What did is below
- [x] 5.2 **Was blocked; resolved as option B, with a delta spec for `consensus/epoch-change`.** Verify the thing `P` made untestable: a majority
      lost and restored makes progress, with the restored process eligible to lead.

      Ω does its part: after a heal all five processes trust the highest-ranked one, confirmed by
      driving Ω alone under the same partition. Epoch-change does not. Measured, epoch-change alone,
      5 processes partitioned `[A,B] [C] [D,E]` then healed: `trusted = [E,E,E,E,E]` and
      `lastts = [32, 27, 43, 10, 10]`. Everyone trusts E; E's last epoch is 10; **E never announces
      another**, so nobody can enter one and the stack is stuck.

      The cause is that `⟨Ω, Trust⟩` is edge-triggered on the leader *value*, and Algorithm 5.5
      announces only on that edge. E was maxrank of its own partition and of the whole membership,
      so its trust never changed and it was never told to announce — while the other group ran its
      epochs up to 43. This is a liveness gap in Algorithm 5.5 composed with Algorithm 2.8, not in
      either alone, and it was **unreachable under `P`** because a partition never healed for the
      detector. Crash-and-restart is unaffected and works: a restarted process's Ω trusts afresh
      from `⊥`, so the edge exists.

      Fixed by option B in `design.md`: a process that trusts a leader other than itself, while in
      an epoch that leader did not start, tells it the timestamp it has reached; the leader chooses
      its next candidate above that rather than one step past its own. That also fixes the climb —
      stepping by `N` per refusal costs a round trip per step, and the gap here is 33
- [x] 5.3 Verify epoch churn under a flapping detector is bounded, or record that it is not and take
      the open question's fallback. **Bounded, and the fallback was not needed**: the existing
      `the_churn_after_a_leadership_change_is_finite` passes unchanged, and a settled stack sends no
      report and starts no epoch. The open question is closed

## 5b. Telling a leader where the others have reached

- [x] 5b.1 `epoch_change`: on a trust change to a leader other than itself, a process whose current
      epoch was started by someone else reports its `lastts` to that leader. State the departure
- [x] 5b.2 The leader's refusal handler chooses its next candidate above the timestamp it was told,
      in its own residue class, rather than stepping by `N`. Verify a report no greater than the
      candidate already chosen moves nothing, so repeated reports cannot drive it without bound
- [x] 5b.3 Restore `progress_resumes_when_a_healed_partition_restores_the_majority` in
      `leader_driven_consensus`, and add the same at the epoch-change level
- [x] 5b.4 Verify a settled stack still sends no report and starts no epoch — the quiescence the
      existing `a_steady_leader_starts_one_epoch_and_no_more` asserts, now that there is a second
      thing that could break it

## 6. What this dates

- [x] 6.1 `stacks.rs`: name Ω over `P` for anything wanting the old behaviour
- [x] 6.2 `README.md`: the protocol table, the suite table and counts, and the roadmap — `◇P` moves
      from "next" to built, and the accrual detector becomes the next thing
- [x] 6.3 `docs/bounded-space.md`: Ω's deployment row stops saying "would be, over ◇P"
- [x] 6.4 `docs/conditional-guarantees.md`: a detector's accuracy is already a scope there; add that
      the *timeout* is one too, and that capping it is choosing which liveness failure to have
- [x] 6.5 `./scripts/check.sh` passes in full
