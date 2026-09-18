## 1. Measure before changing anything

- [x] 1.1 Put the measurement in the suite as a **test**, not a script: count each message kind over
      a settled run of a known number of entries at three and five processes, and record what the
      code does today. Verify the numbers in `proposal.md` reproduce — `p2a`/`p2b` at 3.6× the
      minimum with `retransmit` below the delivery bound, and exactly the minimum above it
- [x] 1.2 Verify the leadership changes in that run are counted from the trace rather than assumed,
      since the per-entry figure is only meaningful once phase one is attributed separately

## 2. A self-addressed message stops crossing the network

**Revised during implementation.** Planned as a shortcut in `MultiPaxosSynod::transmit`; that made
the hand-off invisible to the trace and emptied a non-vacuity floor two registered safety tests
depend on. It belongs to the simulator, which is what turns an effect into a packet. See `design.md`.

- [x] 2.1 `Sim::transmit` hands a message addressed to its own sender over at the current instant,
      with no latency and no fault applied, instead of putting it on the network. Verify the
      protocol is **not** changed: it goes on emitting the effect
- [x] 2.2 Answer `CLAUDE.md`'s question of any new simulator capability — can it lose something
      without raising the event that says so? Verify the answer is recorded in `design.md` and that
      it is *no*: the hand-off narrows what can be lost and there is no scope between a process and
      itself to end
- [x] 2.3 `TraceEvent::HandedToSelf`, distinct from `Sent`, with `ProtoTrace::exchanges` offering
      both together and `sends` continuing to mean the network alone. Verify the two accessors say
      in their own documentation which question each answers
- [x] 2.4 Verify no message a process addresses to itself reaches the network, over a run that
      decides entries **and** changes leadership, so both phases are covered
- [x] 2.5 Verify the leader's own acceptor still counts toward the majority: drive a run where the
      leader's own answer is the one that completes the quorum, and assert the decision happens.
      This is the regression the shortcut most easily causes
- [x] 2.6 Verify the hand-off takes **no time**, which is the half that is not bookkeeping: a leader
      no longer waits a delivery bound for its own acceptor. Assert it from the trace rather than
      from a duration
- [x] 2.7 Verify the hand-off is still observable: a refusal a leader hears from its own acceptor
      must be visible to a reader of the trace. **This is the floor that the protocol-side draft
      emptied**, so assert it directly rather than trusting that `exchanges` covers it
- [x] 2.8 Fix every suite the split dates. Verify each is pointed at the accessor matching *its own*
      claim — `exchanges` where the question is whether a process acted, `sends` where it is what the
      network carried — rather than at whichever one makes it pass
- [x] 2.9 `logged_epoch_consensus`'s partial-write test sequenced by event rather than by a duration:
      its 45 ms window was a function of the latency configuration and stopped landing once a
      leader's message to itself stopped taking a delivery bound. Verify it now searches for the
      partial state by stepping, and that its non-vacuity floor still binds

## 3. Retransmission timed against the bound

- [x] 3.1 The sweep resends `p1a` and `p2a` only where the attempt has been outstanding longer than
      a threshold. Verify the sweep interval and the threshold are separate things, named
      separately, and that the module says what each controls
- [x] 3.2 Choose the threshold, and say in the module what it must exceed and what it must stay
      below: one round trip at the low end, `escalate_after` at the high end. Verify a test pins the
      ordering rather than leaving it to whoever configures `Timing`
- [x] 3.3 Verify nothing is resent before an answer could have arrived: drive an attempt, sweep
      several times inside one round trip, and assert the request went out **once**
- [x] 3.4 Verify the escalations are untouched — a lost `p1a` still restarts phase one and a stalled
      commander still goes back to it, at `escalate_after` as before. These are the cross-check's
      liveness fixes and this change is about cost
- [x] 3.5 Verify the module states that `retransmit` below the delivery bound is a mistake rather
      than a tuning choice, in the terms `detect_after`'s own note uses

## 4. Resending on a session establishment

- [x] 4.1 The scope handler resends outstanding requests to the peer an establishment names, and to
      that peer alone. Verify the module quotes `session_link.rs`'s own description of the event and
      says why this capability is the first to act on it
- [x] 4.2 Verify the resend goes to one peer: break one session, re-establish it, and assert nothing
      was sent to any other process on account of the establishment
- [x] 4.3 Verify it is the establishment and not the timeout that recovers: set the threshold well
      above the run, break a session with a request outstanding, re-establish, and assert the
      exchange completes. Assert the non-vacuity half — the request really was lost, and the
      threshold really had not elapsed
- [x] 4.4 Verify nothing is resent on an establishment with a peer that owes nothing, so that a
      reconnecting cluster does not produce a burst proportional to membership squared

## 5. The identity, asserted

- [x] 5.1 Assert the phase-two identity per entry: one request and one reply per acceptor, and one
      decision to every other process, over a run that loses nothing. Verify it holds **exactly**
      rather than as a bound
- [x] 5.2 Assert the phase-one identity per leadership change, and that it does not grow with the
      entries decided. Verify the non-vacuity half: the run really decided several entries under one
      leader, or the amortisation is untested
- [x] 5.3 Verify the identity is asserted at more than one membership size, so that the `n` in it is
      a variable rather than a constant that happens to fit
- [x] 5.4 State in the suite what the identity does **not** cover: the detector's heartbeats, which
      are per tick rather than per entry, and which are why this capability cannot make the gossip
      pair's claim that an idle run sends nothing. Verify the suite says so rather than leaving a
      reader to wonder why the totals do not match

## 6. Safety, which is the standing risk

- [x] 6.1 `./scripts/check-safety-tests.sh` passes with every registered test still red under both
      mutations. Changing when a message is resent changes which interleavings a seeded run reaches,
      which is exactly how the decision announcement disarmed four of these two changes ago.
      Investigate any test that goes green rather than deregistering it
- [x] 6.2 Verify the hand-driven schedules that advance and tick expecting a resend still exercise
      one. Each such test is checked against what it claims to drive rather than merely made to pass
- [x] 6.3 `cargo test --workspace` passes, and `multi_paxos_replica`'s suite in particular: its
      wedge test depends on the leader answering a re-proposal, and the re-proposal reaching the
      leader depends on this layer's retransmission

## 7. What this dates

- [x] 7.1 The module's documentation on the retry sweep and on `Timing`. Verify the table of the
      three liveness fixes still reads correctly against the code, since the sweep now does two
      jobs on two schedules
- [x] 7.2 `README.md`'s real-world set section: the first half of the resource-use obligation is met
      and the second is not. Verify the wording does not now imply Multi-Paxos is in the set, because
      the state is still unbounded and §4.2 is what changes that
- [x] 7.3 `README.md`'s suite table and counts. Verify against `cargo test --workspace`
- [x] 7.4 `docs/conditional-guarantees.md`: this is the first module to act on `SessionEstablished`
      rather than only propagate it, and that document is where what a scope event obliges is
      recorded. Verify it says so, and that it does **not** claim the other modules do
- [x] 7.5 `./scripts/check.sh` passes in full
