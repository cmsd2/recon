## 1. The source, pinned

- [ ] 1.1 Pin the edition before writing a line. This transcribes van Renesse, *Paxos Made
      Moderately Complex*, Cornell University technical report, 25 March 2011 — Figures 2, 3 and 4,
      §2. The 2015 ACM Computing Surveys version by van Renesse & Altinbüken is a different document
      with different figure numbering, and README currently names *that* one. Verify by opening the
      PDF and checking the figure numbers against what the module quotes
- [ ] 1.2 Extend `CLAUDE.md`'s reference material section: this is the first module here whose source
      is a paper rather than Cachin, so the convention that a module quotes its page has to say which
      paper and which edition. Verify the section names the report, its date, and the figures used
- [ ] 1.3 Correct README's roadmap item 5, which names the 2015 survey. Verify the reference matches
      what 1.1 pinned

## 2. The wire and the ballot

- [ ] 2.1 `Ballot` as `(round: u64, node: NodeId)`, lexicographic, with the source's `⊥` as the
      ordering's bottom. Verify with a unit test that any two ballots are comparable and that a
      ballot names its leader
- [ ] 2.2 One message enum — `P1a`, `P1b`, `P2a`, `P2b` — carrying ballots and pvalues, the whole of
      Figures 2 and 3. Verify it survives encoding, as every wire type here does
- [ ] 2.3 State the ballot generator's scope in the module: **this incarnation**, volatile, and why
      the source makes that sufficient. Verify the documentation says what a re-minted ballot would
      break and that a durable counter is what a fail-recovery variant buys

## 3. The acceptor

- [ ] 3.1 `ballot_num` and `accepted`, with Figure 2 quoted above them. `p1a` takes up a strictly
      greater ballot and answers with everything accepted; `p2a` accepts under `b ≥ ballot_num`.
      Verify against a hand-driven test that a stale `p1a` is refused and that the refusal names the
      ballot that beat it
- [ ] 3.2 Verify the promise is monotonic: an acceptor that has taken up a ballot never afterwards
      accepts under a lower one, asserted over a run that offers it lower ones

## 4. The leader, and its bookkeeping

- [ ] 4.1 `ballot_num`, `active`, `proposals`, with Figure 4 quoted. Verify a proposal arriving while
      passive is remembered and sent once the ballot is adopted
- [ ] 4.2 The scout as a field rather than a thread — one at a time, for this process's own ballots
      only — collecting `p1b` to a majority and yielding the union of pvalues. Quote Figure 3(b) and
      state in the module where each of its `switch receive` arms went. Verify adoption needs a
      majority and not one fewer
- [ ] 4.3 `pmax` over the collected pvalues, and the update that replaces this leader's proposal for
      a slot with the highest-ballot pvalue seen. This is the step the whole safety argument rests
      on. Verify directly: a proposal already accepted by a majority under a lower ballot is what the
      new leader proposes, not what it set out to propose
- [ ] 4.4 Commanders as a map keyed by slot within the current ballot, collecting `p2b` to a
      majority. Quote Figure 3(a). Verify at most one commander exists per slot per ballot —
      Invariant C1 — and that a second proposal for a slot already commanded is not started
- [ ] 4.5 `preempted` handling: move `ballot_num` past the ballot that beat it, go passive, and
      **do not** start a scout unless still trusted. Verify a preempted leader that Ω no longer
      trusts stops competing

## 5. Liveness through Ω, and the departure

- [ ] 5.1 Compose `Child<EventualLeaderDetector>` and act on `Trust`: scout for the next ballot when
      trusted, passive otherwise. Verify a run where the detector settles chooses proposals for every
      slot proposed
- [ ] 5.2 Document the departure in the module: §3 pings the preempting leader and backs off with an
      AIMD timeout, and says the concept is failure detection; this trusts Ω instead. State what
      differs — Ω names one leader where the paper lets any correct one win a race — and that the
      condition inherits Ω's own, which is stated at each link. Verify the module's departures list
      says all three
- [ ] 5.3 Verify progress is claimed only conditionally: a run with two processes each believing
      themselves leader may choose nothing, and the suite says so rather than asserting termination
      unconditionally
- [ ] 5.4 Drive a run where the detector is **wrong** for a while, so Ω's answer and the ballots
      disagree, and verify safety holds through it. The risk `design.md` names, tested rather than
      assumed

## 6. Retransmission and the link

- [ ] 6.1 Take the link as a type parameter defaulting to the session link, classify `Boundary`, and
      propagate rather than absorb. Verify a session ending reaches the layer above, and that the
      module states it bridges nothing
- [ ] 6.2 One periodic timer resending `p1a` and `p2a` to acceptors that have not answered, compared
      against its own `TimerId` before acting. Verify a dropped request is retried and the round
      still completes
- [ ] 6.3 Verify the send rate is flat: the retry set is bounded by the outstanding `waitfor` sets
      and therefore by membership, so `tests/common::assert_send_rate_flat!` should hold without the
      module doing anything special. This is the first thing here whose rate is flat by construction

## 7. Safety, asserted over runs

- [ ] 7.1 At most one proposal chosen per slot, collected from the trace across every process and
      every ballot — the property is that no *pair* of observers disagrees, so protocol state is not
      enough. Verify over a run with competing ballots
- [ ] 7.2 A chosen proposal is one that was proposed, and a slot nobody proposed for stays empty
- [ ] 7.3 Safety under crashes: a minority crashes and is **not** restarted, and no two processes
      learn different proposals. Verify the run really contained the crashes it claims
- [ ] 7.4 Non-vacuity, and this suite needs both halves. "At most one chosen" is satisfied by a run
      that chooses nothing, and "no two disagree" by a run with one leader. Assert that something was
      chosen, that the run contained competing ballots, and that a preemption really happened —
      `tests/method.rs` is the precedent

## 8. The boundary this change does not cross

- [ ] 8.1 Verify the suite crashes processes without restarting them, and that this is stated where
      it occurs rather than left to a reader to notice
- [ ] 8.2 State the space bound in the module: unbounded, a transcription, and which section of the
      source bounds it — §4.1 and §4.2, which are change 3. Verify `docs/bounded-space.md` gains a
      row saying the same
- [ ] 8.3 Verify the module documents that a process returning without its state is outside the
      model, in the source's own terms, and what a durable ballot counter would buy

## 9. What this dates

- [ ] 9.1 `README.md`: the protocol table, the suite table and counts, and the specification tree.
      Verify the counts against `cargo test --workspace`
- [ ] 9.2 `README.md`'s roadmap item 5 — the open question about which page was the blocker, and this
      answers it. Say what is built and what the remaining two changes are
- [ ] 9.3 `./scripts/check.sh` passes in full
- [ ] 9.4 `./scripts/check-durability-tests.sh` still passes. This module registers nothing — it keeps
      nothing durable — but the guard must not have been broken by the new suite
