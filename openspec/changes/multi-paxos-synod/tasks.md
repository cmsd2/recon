## 1. The source, pinned

- [x] 1.1 Pin the edition before writing a line. This transcribes **van Renesse, R. and Altinbuken,
      D. (2015) 'Paxos Made Moderately Complex', *ACM Computing Surveys*, 47(3), pp. 1–36** — §2, and
      Figures 4 (acceptor), 6 (commander and scout) and 7 (leader). The 2011 Cornell technical report
      of the same title is a different document with different figure numbering and a materially
      different §4.2; it is not what this quotes. Verify by opening the survey and checking each
      figure number against what the module quotes
- [x] 1.2 Extend `CLAUDE.md`'s reference material section: this is the first module here whose source
      is a paper rather than Cachin, so the convention that a module quotes its page has to name the
      paper *and the edition*, since both editions exist under one title. Verify the section carries
      the full reference and says which figures are used
- [x] 1.3 Verify README's roadmap item 5 names the same edition. It already names the survey, so this
      is a check rather than a correction
- [x] 1.4 Record the cross-check source in the module's documentation: **Liu, Y.A., Chand, S. and
      Stoller, S.D. (2019) 'Moderately Complex Paxos Made Simple'**, whose DistAlgo specification and
      TLA+ proofs are where the liveness fixes in group 6 come from. Verify each fix cites it where it
      departs from the survey's pseudocode

## 2. The wire and the ballot

- [x] 2.1 `Ballot` as `(round: u64, node: NodeId)`, lexicographic, with the source's `⊥` as the
      ordering's bottom. Verify with a unit test that any two ballots are comparable and that a
      ballot names its leader
- [x] 2.2 One message enum — `P1a`, `P1b`, `P2a`, `P2b` — carrying ballots and pvalues, the whole of
      Figures 4 and 6. Verify it survives encoding, as every wire type here does
- [x] 2.3 State the ballot generator's scope in the module: **this incarnation**, volatile, and why
      the source makes that sufficient. Verify the documentation says what a re-minted ballot would
      break and that a durable counter is what a fail-recovery variant buys

## 3. The acceptor

- [x] 3.1 `ballot_num` and `accepted`, with Figure 4 quoted above them. `p1a` takes up a strictly
      greater ballot and answers with everything accepted; `p2a` accepts under `b ≥ ballot_num` and
      adopts `b` in the same transition — a stated departure from the quoted figure, whose condition
      is `b = ballot_num`. The condition used is the 2011 report's, and is Liu et al.'s fix for the
      useless-replies issue; `design.md` says why the survey's line does not survive this link.
      Document it in the module's departures list with both editions named. Verify against a
      hand-driven test that a stale `p1a` is refused and that the refusal names the ballot that beat
      it
- [x] 3.2 Verify the promise is monotonic: an acceptor that has taken up a ballot never afterwards
      accepts under a lower one, asserted over a run that offers it lower ones
- [x] 3.3 Verify the departure does what it is for: an acceptor that never saw phase 1 for a ballot
      receives that ballot's `p2a`, adopts and accepts, and its `p2b` counts toward the commander's
      majority. Assert the non-vacuity half too — the acceptor really had not seen the `p1a`

## 4. The leader, and its bookkeeping

- [x] 4.1 `ballot_num`, `active`, `proposals`, with Figure 7 quoted. Verify a proposal arriving while
      passive is remembered and sent once the ballot is adopted
- [x] 4.2 The scout as a field rather than a thread — one at a time, for this process's own ballots
      only — collecting `p1b` to a majority and yielding the union of pvalues. Quote Figure 6(b) and
      state in the module where each of its `switch receive` arms went. Verify adoption needs a
      majority and not one fewer
- [x] 4.3 `pmax` over the collected pvalues, and the update that replaces this leader's proposal for
      a slot with the highest-ballot pvalue seen. This is the step the whole safety argument rests
      on. Verify directly: a proposal already accepted by a majority under a lower ballot is what the
      new leader proposes, not what it set out to propose
- [x] 4.4 Commanders as a map keyed by slot within the current ballot, collecting `p2b` to a
      majority. Quote Figure 6(a). Verify at most one commander exists per slot per ballot —
      Invariant C1 — and that a second proposal for a slot already commanded is not started
- [x] 4.5 `preempted` handling: move `ballot_num` past the ballot that beat it, go passive, and
      **do not** start a scout unless still trusted. Verify a preempted leader that Ω no longer
      trusts stops competing

## 5. Liveness through Ω, and the departure

- [x] 5.1 Compose `Child<EventualLeaderDetector>` and act on `Trust`: scout for the next ballot when
      trusted, passive otherwise. Verify a run where the detector settles chooses proposals for every
      slot proposed
- [x] 5.2 Document the departure in the module: §3 pings the preempting leader and backs off with an
      AIMD timeout, and says the concept is failure detection; this trusts Ω instead. State what
      differs — Ω names one leader where the paper lets any correct one win a race — and that the
      condition inherits Ω's own, which is stated at each link. Verify the module's departures list
      says all three
- [x] 5.3 Verify progress is claimed only conditionally: a run with two processes each believing
      themselves leader may choose nothing, and the suite says so rather than asserting termination
      unconditionally
- [x] 5.4 Drive a run where the detector is **wrong** for a while, so Ω's answer and the ballots
      disagree, and verify safety holds through it. The risk `design.md` names, tested rather than
      assumed

## 6. Retransmission and the link

- [x] 6.1 Take the link as a type parameter defaulting to the session link, classify `Boundary`, and
      propagate rather than absorb. Verify a session ending reaches the layer above, and that the
      module states it bridges nothing
- [x] 6.2 One periodic timer resending `p1a` and `p2a` to acceptors that have not answered, compared
      against its own `TimerId` before acting. Verify a dropped request is retried and the round
      still completes
- [x] 6.3 The three liveness fixes Liu et al. found in this specification, each reachable here
      because this link loses messages, and each tested by dropping the message that causes it:
      a lost `p1a` must not leave the leader waiting for ever (time out and restart phase one); a
      lost `p2b` must not leave a slot undecided (resend `p2a`); and a lost `preempt` must not leave
      the leader sending `p2a` for ever to acceptors that have moved to a higher ballot (**restart
      phase one**, which resending cannot substitute for). Verify each with the specific message
      dropped, not merely with lossy links switched on. Both restarts rerun phase one under the
      **same** ballot, as `design.md` decides: verify the lost-preempt case ends with the leader
      learning the higher ballot from a `p1b` answer rather than minting one blind
- [x] 6.4 Drive the route that motivated the acceptor departure end to end: one acceptor's `p1a`
      dies at a session ending, the scout completes with a majority that excludes it, and the
      retransmitted `p2a` reaches it cold. Verify its answer counts toward the decision and that no
      commander exits on a ballot lower than its own
- [x] 6.5 Record the fourth, which is the replica's and belongs to change 2 — no decision for a slot
      wedges every replica once `WINDOW` fills, fixed by re-proposing after a timeout — so change 2
      does not rediscover it. Verify it is written down where change 2 will find it
- [x] 6.6 Verify the send rate is flat once the work is done: the retry sweep is bounded by
      membership times the slots still undecided, and a decided slot retires its commander and
      leaves the sweep, so `tests/common::assert_send_rate_flat!` holds over the windows after the
      last decision without the module doing anything special

## 7. Safety, asserted over runs

- [x] 7.1 At most one proposal chosen per slot, collected from the trace across every process and
      every ballot — the property is that no *pair* of observers disagrees, so protocol state is not
      enough. Verify over a run with competing ballots
- [x] 7.2 A chosen proposal is one that was proposed, and a slot nobody proposed for stays empty
- [x] 7.3 Safety under crashes: a minority crashes and is **not** restarted, and no two processes
      learn different proposals. Verify the run really contained the crashes it claims
- [x] 7.4 Non-vacuity, and this suite needs both halves. "At most one chosen" is satisfied by a run
      that chooses nothing, and "no two disagree" by a run with one leader. Assert that something was
      chosen, that the run contained competing ballots, and that a preemption really happened —
      `tests/method.rs` is the precedent. Place each counter at the point in the schedule that
      depends on it, and cover both roles: preemptor and preempted, refuser and refused
- [x] 7.5 A stepwise checker fed from the trace: per-acceptor promise history and per-slot chosen
      sets, asserting after every event that promises only rise, accepts happen only at the
      promise, one command exists per ballot and slot, and no two observers disagree on a slot —
      so a violation names the first event that broke it and the seed replays it
- [x] 7.6 A seeded sweep: the property checks of 7.1–7.3 and the checker of 7.5 run across a batch
      of seeds with loss, duplication, reordering and a partition window on, and a failure reports
      its seed. Keep the batch small enough for `check.sh` and say in the test where to turn the
      count up
- [x] 7.7 The sabotage check, in `check-durability-tests.sh`'s shape and its own script: two
      feature-gated mutations — a leader that ignores `pmax`, an acceptor that accepts below its
      promise — under each of which every registered safety test must fail. A test still green
      under a mutation is reading something other than the property. Register the suite's safety
      tests by name, run it when touching the module, and give README's guard table its row in the
      same commit

## 8. The boundary this change does not cross

- [x] 8.1 Verify the suite crashes processes without restarting them, and that this is stated where
      it occurs rather than left to a reader to notice
- [x] 8.2 State the space bound in the module: unbounded, a transcription, and which section of the
      source bounds it — §4.1 and §4.2, which are change 3. Verify `docs/bounded-space.md` gains a
      row saying the same
- [x] 8.3 Verify the module documents that a process returning without its state is outside the
      model, in the source's own terms, and what a durable ballot counter would buy

## 9. What this dates

- [x] 9.1 `README.md`: the protocol table, the suite table and counts, and the specification tree.
      Verify the counts against `cargo test --workspace`
- [x] 9.2 `README.md`'s roadmap item 5 — the open question about which page was the blocker, and this
      answers it. Say what is built and what the remaining two changes are
- [x] 9.3 `./scripts/check.sh` passes in full
- [x] 9.4 `./scripts/check-durability-tests.sh` still passes. This module registers nothing — it keeps
      nothing durable — but the guard must not have been broken by the new suite
