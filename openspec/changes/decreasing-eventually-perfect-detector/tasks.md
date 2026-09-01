## 1. The detector port

- [ ] 1.1 Add `Detector` and `DetectorInd { Suspect, Restore }` — what a layer above a detector may
      depend on, and the whole of it. Say in the module why it exists now and not before: two
      detectors is the second consumer constraint 4 asks for
- [ ] 1.2 `perfect_failure_detector` satisfies it, classifying its `Crash` as a suspicion and never
      producing a withdrawal. Verify from a test that it never does

## 2. ◇P — Algorithm 2.7, then the two departures

- [ ] 2.1 Transcribe Algorithm 2.7 into the module docstring from the indexed book, and verify the
      guards against the page rather than a recollection — the OCR drops the negations in
      `if (p ∉ alive) ∧ (p ∉ suspected)`, and reading them as written inverts the algorithm
- [ ] 2.2 Implement it, with `Suspect` and `Restore`, and verify a crash is suspected everywhere and
      never retracted while it lasts
- [ ] 2.3 Verify a correct process suspected under withdrawn synchrony is **restored** when heard
      from again, and that a recovered process is restored
- [ ] 2.4 Verify nothing is reported when the suspected set does not change between rounds
- [ ] 2.5 Add the decrease: down one `Δ` after `quiet_rounds` clean rounds, never below the floor.
      Verify a false suspicion raises the timeout and sustained accuracy lowers it
- [ ] 2.6 Verify the timeout does not thrash — drive a latency that steps up and then down, and
      assert the delay follows, rising faster than it falls, and settles
- [ ] 2.7 Add the cap. Verify the timeout stops at `max_delay`, and that accuracy is then lost when
      the network settles above it — the stated condition failing, asserted rather than described
- [ ] 2.8 Sweep the cap against a latency distribution and assert the shape: false suspicions rise
      as the cap falls below the true delay. This is what makes the trade a measurement rather than
      a number someone picked, and its own non-vacuity check
- [ ] 2.9 Verify state is bounded by membership and the send rate is flat in time

## 3. Ω over the port

- [ ] 3.1 Make `EventualLeaderDetector` generic over the port, **still defaulting to `P`**: no
      behaviour change, every existing suite unchanged and passing
- [ ] 3.2 Handle `Restore`: `suspected` shrinks, and `maxrank` may climb. Verify trust returns to a
      restored process that outranks the incumbent, and that a restoration which does not change the
      leader raises nothing
- [ ] 3.3 Verify a recovered process can lead again — the thing `P` makes impossible, and the reason
      for this change
- [ ] 3.4 Flip the default to `◇P` and run everything above. Record what moves

## 4. What must not move

- [ ] 4.1 `uniform_reliable_broadcast` and `flooding_consensus` keep `P`. Add a line to each saying
      why: their agreement rests on strong accuracy by name, and a detector allowed to be wrong
      would break it — which is what the majority-ack broadcast beside them exists to avoid

## 5. The stack above, under a detector that retracts

- [ ] 5.1 Run the epoch-change, Paxos and logged-Paxos suites against the new default and fix or
      record every difference. `the_churn_after_a_leadership_change_is_finite` is the test most
      likely to speak
- [ ] 5.2 Verify the thing `P` made untestable: a majority lost to crashes and **restored by
      recovery** makes progress, with the recovered process eligible to lead. Replace the workaround
      in `leader_driven_consensus`'s no-majority test, whose comment says it exists because a healed
      partition never heals for the detector
- [ ] 5.3 Verify epoch churn under a flapping detector is bounded, or record that it is not and take
      the open question's fallback

## 6. What this dates

- [ ] 6.1 `stacks.rs`: name Ω over `P` for anything wanting the old behaviour
- [ ] 6.2 `README.md`: the protocol table, the suite table and counts, and the roadmap — `◇P` moves
      from "next" to built, and the accrual detector becomes the next thing
- [ ] 6.3 `docs/bounded-space.md`: Ω's deployment row stops saying "would be, over ◇P"
- [ ] 6.4 `docs/conditional-guarantees.md`: a detector's accuracy is already a scope there; add that
      the *timeout* is one too, and that capping it is choosing which liveness failure to have
- [ ] 6.5 `./scripts/check.sh` passes in full
