## 1. Reading the pages before writing the modules

- [x] 1.1a Transcribe **Algorithm 2.8** into the Ω module's docstring from the indexed Cachin,
      Guerraoui & Rodrigues, and verify it against the book rather than a recollection. Done, and it
      corrected this change's own proposal: the page says `maxrank`, the **highest**-ranked process
      not suspected, where the proposal had written lowest
- [ ] 1.1b Transcribe Modules 5.5 and 5.6 and the consensus module into their module docstrings,
      once those modules exist
- [x] 1.2 Transcribe Algorithms 5.5, 5.6 and 5.7 into their module docstrings. **The `Uses:` lines
      are already verified against the book** — the line that was wrong last time and decided which
      link the whole change ran over:
      - 5.5 Leader-Based Epoch-Change: `PerfectPointToPointLinks`, `BestEffortBroadcast`,
        `EventualLeaderDetector` — **three** children, not the two this change assumed
      - 5.6 Read/Write Epoch Consensus: `PerfectPointToPointLinks`, `BestEffortBroadcast`
      - 5.5 also settles the timestamp question: `ts := rank(self)` advanced by `ts := ts + N`, so
        each process draws from its own residue class and no two can mint the same timestamp
- [ ] 1.3 Transcribe Algorithms 5.8, 5.9, 5.10 and 5.11, and verify what 5.9 stores and at which
      point, since the durable-before-visible ordering is stated by the placement of `store` in the
      handler rather than by prose

## 2. The eventual leader detector

- [x] 2.1 Add the module deriving Ω from the perfect failure detector — `maxrank(Π \ suspected)`,
      the highest-ranked process not suspected — and verify it builds with nothing composed over it
- [x] 2.2 Verify the trusted process is a deterministic function of the suspected set: two processes
      with the same suspicions trust the same leader, and an unchanged set raises no new indication
- [x] 2.3 Verify a single leader emerges in a run where nothing is suspected, and that the trusted
      process changes when the leader crashes
- [x] 2.4 Verify the detector **can** disagree, by withdrawing the synchrony assumption so correct
      processes are suspected and two processes trust different leaders at once. Without this the
      whole change is untestable, so it is checked here rather than assumed later
- [x] 2.5 State the departure in the module docstring: Algorithm 2.8 derives Ω from an *eventually*
      perfect detector and this derives it from a perfect one, which is stronger — and record that
      an Ω which is never wrong makes every test above it vacuous

## 3. Epoch-change

- [x] 3.1 Add the module over Ω per Algorithm 5.5, and verify a run with a steady leader starts one
      epoch and no more
- [x] 3.2 Verify timestamps strictly increase at each process, and that two processes starting an
      epoch with the same timestamp name the same leader
- [x] 3.3 Verify a leadership change starts a new epoch, and that nothing else does — no timer, no
      ordinary message traffic. **It starts more than one**, and that is the algorithm: while `Trust`
      propagates, a process still trusting the old leader NACKs the new one's announcement, so the
      new leader climbs. A test asserting exactly two failed. The churn is pinned as finite by
      `the_churn_after_a_leadership_change_is_finite`
- [x] 3.4 Verify epochs settle: once the leader detector stops changing its mind, every correct
      process reaches the same final epoch and starts no further one
- [x] 3.5 Verify processes may legitimately be in different epochs meanwhile, so that the settling
      assertion above is not read as a claim that they never differ

## 4. Read/write epoch consensus, tested before anything composes over it

- [x] 4.1 Add the module per Algorithm 5.6 — read from a majority, write to a majority, decide — and
      verify a single epoch with a correct leader and no faults decides the proposed value
- [x] 4.2 Verify no decision is reached when fewer than a majority have accepted, by withholding
      acceptances from all but a minority
- [x] 4.3 Verify a value decided in one epoch is among what a later epoch reads from a majority,
      which is the intersection argument the whole algorithm rests on
- [x] 4.4 Implement the abort handshake and verify an abandoned instance reports the value and
      timestamp it accepted
- [x] 4.5 Verify an abandoned instance is **silent**: deliver a message to an instance that has been
      abandoned and assert it sends nothing and decides nothing. Getting this wrong is a safety bug
      rather than a liveness one
- [x] 4.6 Verify only the epoch's leader initiates a read or a write, and that a follower asked to
      propose initiates nothing
- [x] 4.7 Verify nothing is decided that was not proposed, and that a process decides at most once
      per epoch

## 5. Leader-driven consensus — Paxos

- [x] 5.1 Add the module over epoch-change and epoch consensus per Algorithm 5.7, holding the epoch
      consensus as a concrete field replaced on each epoch change, and verify a run with no faults
      decides everywhere. **It did not, and the cause was the perfect link's duplicate set**: each
      epoch builds a new link whose sequence numbers restart at one, while the receiver's set is
      cleared at a different moment, so a foreign-epoch message recorded `(src, 1)` and the real
      one was discarded as a duplicate. Three of five processes stalled permanently. `ep, on_msg`
      now drops mis-tagged traffic at the door, before the link beneath can record it
- [x] 5.2 Verify the next instance is constructed from the state the previous one returned, and not
      before it has answered — collapsing that window loses the state and with it the safety property
- [x] 5.3 Verify a decision is final: a new epoch beginning after a process has decided does not make
      it decide again or differently
- [x] 5.4 Verify agreement holds under crashes, including a leader crashing partway through a write
- [x] 5.5 **The headline obligation.** Verify agreement holds under an *inaccurate* leader detector:
      withdraw the synchrony assumption so correct processes are suspected, produce two processes
      each acting as leader in overlapping epochs, and assert no two processes decide differently
- [x] 5.6 Verify that assertion is not vacuous, by confirming from the trace that more than one
      process really did act as a leader in overlapping epochs — an agreement assertion over a run
      with one unchallenged leader proves nothing
- [x] 5.7 Verify the contrast that justifies the abstraction: the same schedule that splits
      `flooding_consensus` does not split this one
- [x] 5.8 Verify termination is conditional and honest — every correct process decides once a
      majority is correct and the detector settles, and nothing is decided while no majority exists.
      The no-majority half uses crashes, not a partition: Ω here is derived from a *perfect* failure
      detector, whose accusations never retract, so a healed partition never heals for the detector
      and a "restore the majority" run would be testing that departure rather than the algorithm.
      Recovery is where that question belongs, and it is asked in group 8

      **Found while writing this group, and fixed in `epoch_change`:** Algorithm 5.5's bare `[NACK]`
      does not terminate over a link that retransmits. Every process that does not yet trust the
      announcer NACKs, each NACK bumps `ts` and re-announces, and the stubborn link beneath keeps
      everything alive — a five-process run with one crash reached epoch **647,309** and 2.3 million
      sends inside a second of virtual time, and no epoch lasted long enough for a write to finish.
      Algorithm 5.8 sends `[NACK, nts]` and guards with `such that nts = ts`; taking 5.8's form here
      brings the same run to epoch 14 and 3,511 sends. The departure is recorded in the module

## 6. The fail-recovery half — logged epoch-change

- [x] 6.1 Add the module per Algorithm 5.8, and verify the epoch timestamp is durable before any
      message or indication reveals it
- [x] 6.2 Verify a restarted process does not reuse a timestamp it or an earlier incarnation used.
      **It does reuse one, and that is the algorithm.** `ts` is volatile and 5.8 does not store it,
      so a recovered leader climbs from `rank(self)` again and re-announces candidates it has used.
      What `startts` being durable buys is that no process ever *enters* a timestamp twice, and that
      is what the test asserts — with the reuse confirmed rather than assumed away
- [x] 6.3 Verify a process that crashes and recovers while leadership is settled rejoins the same
      final epoch rather than starting a fresh sequence

      **Departure found while writing this group.** Algorithm 5.8 answers every NEWEPOCH it does not
      act on with a NACK, and over the stubborn broadcast its own `Uses:` line names, that does not
      terminate: the broadcast redelivers the announcement every process has just accepted, each
      redelivery fails `newts > startts`, each refusal makes the leader climb, for ever. Measured at
      epoch 380 and climbing with leadership settled and nothing faulty. `epoch_change` does not
      have this because a best-effort broadcast over perfect links delivers once. A repeat of the
      epoch already entered is now silence rather than a refusal; a stale or untrusted announcement
      is still refused, and a test pins that the refusal still does its work

## 7. Logged read/write epoch consensus

- [x] 7.1 Add the module per Algorithm 5.9, over the stubborn link and stubborn broadcast its header
      names, and verify the accepted value and timestamp are written before the acceptance is sent —
      in the handler's own text, not by relying on the driver to buffer effects. The same for the
      decision, one step later
- [x] 7.2 Verify recovery restores the accepted state, and that a read of a recovered process returns
      the value and timestamp it accepted rather than an empty state — asserted on the wire, not
      only in the field, since the leader's READ is still being retransmitted when it comes back
- [x] 7.3 Verify a process that accepted nothing recovers nothing and is not treated as having
      accepted. **Not "recovers nothing":** 5.9 stores `(valts, val)` in `Init`, so every process
      has a record from its first event. What the test asserts is that the record says ⊥
- [x] 7.4 Verify dying inside the write never leaves an acceptance announced without a record, by
      spending `crash_on_next_write`. Both outcomes occur across forty seeds, and either way the
      leader's retransmission brings the acceptance back with a record behind it
- [x] 7.5 Verify safety across crashes and recoveries, including a leader crashing after some but not
      all processes have accepted, with a non-vacuity half confirming the write really was left
      partly applied

      **The stubborn children forced three changes to the book's counters**, all recorded in the
      module: `accepted := accepted + 1` counts messages and one process's ACCEPT arrives for ever,
      so it counts processes instead; `upon #(states) > N/2` and `upon accepted > N/2` are re-armed
      by clearing what they count, which is not enough when the messages come back, so `written` and
      `announced` make each fire once; and `store` is applied only when it changes something, so the
      write count stays one per acceptance and a test can check it rather than trust it

## 8. Logged leader-driven consensus

- [ ] 8.1 Add the module per Algorithms 5.10 and 5.11, and verify a run with crashes and recoveries
      decides everywhere once a majority is back
- [ ] 8.2 Verify a process that has decided still holds that decision after a crash and recovery
- [ ] 8.3 Verify agreement under all three faults at once — crashes, recoveries, and a detector
      suspecting correct processes
- [ ] 8.4 Verify that run really contained all three, by confirming from the trace at least one
      crash, at least one recovery, and more than one acting leader
- [ ] 8.5 Verify progress resumes when a lost majority is restored by recovery, and that no decision
      is reached while no majority exists

## 9. What this dates

- [ ] 9.1 Add a protocol table row and a suite row per module to `README.md`, and verify the counts
      and claimed statuses against what `cargo test --workspace` prints and what the modules say
- [ ] 9.2 Update `docs/bounded-space.md`'s audit table with each module's state and what bounds it,
      and verify the claim matches what the modules state
- [ ] 9.3 Update `docs/conditional-guarantees.md` where it discusses what a failure detector's
      accuracy costs, now that there is an abstraction here which survives losing it
- [ ] 9.4 Check whether `CLAUDE.md`'s note that everything rests on a perfect failure detector is
      still true, and correct it if not
- [ ] 9.5 Run `./scripts/check.sh` and verify it passes in full
