## 1. The module

- [x] 1.1 Transcribe Algorithm 5.1 over `best_effort_broadcast` and `perfect_failure_detector`,
      with the pseudocode quoted above the implementation and the status stated — transcription,
      academic, fail-stop, space bounded by membership and rounds; verify it compiles and that a
      run with no faults decides
- [x] 1.2 Render the decision guard as a standing condition called from both the broadcast path
      and the detector path; verify a round completes on a `Crash` indication alone, with no
      message in flight at the moment it completes
- [x] 1.3 Seed `receivedfrom[0]` to the full membership; verify a first-round decision requires
      having heard from every process, and does not happen when one is missing
- [x] 1.4 Carry the previous round's proposal set into the next round's broadcast, as the
      pseudocode does; verify with a crash cascade that the decision still reflects a proposal made
      by a process that crashed after broadcasting it
- [x] 1.5 Register the module in `crates/recon-protocols/src/lib.rs`; verify `./scripts/check.sh`
      passes

## 2. The four properties

- [x] 2.1 Verify termination with no faults, and with crashes, in synchronous mode
- [x] 2.2 Verify termination is bounded by membership: crash processes in consecutive rounds and
      assert a decision is still reached in no more rounds than there are processes
- [x] 2.3 Verify validity — every decided value was proposed, and a unanimous proposal is the
      decision
- [x] 2.4 Verify integrity: each process reports at most one decision, including when a decision
      from another process arrives after it has already decided
- [x] 2.5 Verify agreement holds under crashes while the detector is perfect, including when the
      deciding process crashes immediately after deciding — the case the `DECIDED` broadcast exists
      for
- [x] 2.6 Verify the decision rule is deterministic: two processes holding the same proposal set
      decide the same value

## 3. What strong accuracy is worth

- [x] 3.1 Find a schedule where a false suspicion splits the decision — a partition inside
      synchronous mode, so that each side accuses the other of crashing — and verify two correct
      processes decide differently
- [x] 3.2 Verify the split is an agreement failure and not a termination failure in disguise:
      assert both sides decided, that the decisions differ, and that no process crashed during the
      run
- [x] 3.3 Verify the split survives the system stabilising: heal the partition, run on well past
      the point where every process is reachable by every other, and assert both decisions stand.
      The decision was irrevocable and was taken during the unstable interval, so eventual
      stability arrives too late — assert this rather than the weaker fact that this detector never
      withdraws an accusation, which would not survive replacing it with an eventually perfect one
- [x] 3.4 Verify the same schedule under a detector that stays perfect does **not** split, so the
      difference is attributable to the accuracy failure and not to the partition's effect on
      delivery

- [x] 3.5 Verify the correct set does not decay: assert that no process crashed in the splitting
      run, that every process is reachable by every other before it ends, and that the two wrongly
      held views of `correct` were non-empty and disjoint — the violation is between correct
      processes, not a consequence of losing them
## 4. Bounds and non-vacuity

- [x] 4.1 Verify state does not grow with messages handled: run many messages through repeated
      rounds and assert the state stays bounded by membership and the rounds actually entered
- [x] 4.2 Verify a second proposal after a decision produces no second decision from the instance
- [x] 4.3 Verify every absence-of-violation assertion is paired with a minimum decision count, so
      that a protocol deciding nothing would fail the suite

## 5. Recording it

- [x] 5.1 Add the rung to the README's protocol table with its status and space bound, and move the
      "Next" line on; verify the relative links still resolve
- [x] 5.2 Record what the exercise showed as notes in the change: whether the split was as easy to
      provoke as expected, whether the two failure modes — accuracy costing safety, completeness
      costing liveness — separated cleanly in testing, and what that implies for the leader-driven
      rungs that come next
