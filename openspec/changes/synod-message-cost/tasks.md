## 1. Measure before changing anything

- [ ] 1.1 Put the measurement in the suite as a **test**, not a script: count each message kind over
      a settled run of a known number of entries at three and five processes, and record what the
      code does today. Verify the numbers in `proposal.md` reproduce — `p2a`/`p2b` at 3.6× the
      minimum with `retransmit` below the delivery bound, and exactly the minimum above it
- [ ] 1.2 Verify the leadership changes in that run are counted from the trace rather than assumed,
      since the per-entry figure is only meaningful once phase one is attributed separately

## 2. A self-addressed message becomes a call

- [ ] 2.1 `transmit` dispatches to the handler directly where `to == self.me`. Verify the module
      states the re-entrancy argument: which handler can call which, and why the chain terminates
- [ ] 2.2 Verify the chain terminates in fact and not only in argument — a test that drives a
      leader which is also an acceptor through a full phase one and phase two and asserts the run
      completes, with a bound on nesting depth if the argument needs one
- [ ] 2.3 Verify no message in a run is addressed by a process to itself, over a run that decides
      entries **and** changes leadership, so both phases are covered
- [ ] 2.4 Verify the leader's own acceptor still counts toward the majority: drive a run where the
      leader's own answer is the one that completes the quorum, and assert the decision happens.
      This is the regression a shortcut most easily causes
- [ ] 2.5 Verify the fault this removes is really gone: a self-session break, if the simulator can
      express one, must no longer be able to lose anything. State in the suite what was being
      modelled before and why it could not happen

## 3. Retransmission timed against the bound

- [ ] 3.1 The sweep resends `p1a` and `p2a` only where the attempt has been outstanding longer than
      a threshold. Verify the sweep interval and the threshold are separate things, named
      separately, and that the module says what each controls
- [ ] 3.2 Choose the threshold, and say in the module what it must exceed and what it must stay
      below: one round trip at the low end, `escalate_after` at the high end. Verify a test pins the
      ordering rather than leaving it to whoever configures `Timing`
- [ ] 3.3 Verify nothing is resent before an answer could have arrived: drive an attempt, sweep
      several times inside one round trip, and assert the request went out **once**
- [ ] 3.4 Verify the escalations are untouched — a lost `p1a` still restarts phase one and a stalled
      commander still goes back to it, at `escalate_after` as before. These are the cross-check's
      liveness fixes and this change is about cost
- [ ] 3.5 Verify the module states that `retransmit` below the delivery bound is a mistake rather
      than a tuning choice, in the terms `detect_after`'s own note uses

## 4. Resending on a session establishment

- [ ] 4.1 The scope handler resends outstanding requests to the peer an establishment names, and to
      that peer alone. Verify the module quotes `session_link.rs`'s own description of the event and
      says why this capability is the first to act on it
- [ ] 4.2 Verify the resend goes to one peer: break one session, re-establish it, and assert nothing
      was sent to any other process on account of the establishment
- [ ] 4.3 Verify it is the establishment and not the timeout that recovers: set the threshold well
      above the run, break a session with a request outstanding, re-establish, and assert the
      exchange completes. Assert the non-vacuity half — the request really was lost, and the
      threshold really had not elapsed
- [ ] 4.4 Verify nothing is resent on an establishment with a peer that owes nothing, so that a
      reconnecting cluster does not produce a burst proportional to membership squared

## 5. The identity, asserted

- [ ] 5.1 Assert the phase-two identity per entry: one request and one reply per acceptor, and one
      decision to every other process, over a run that loses nothing. Verify it holds **exactly**
      rather than as a bound
- [ ] 5.2 Assert the phase-one identity per leadership change, and that it does not grow with the
      entries decided. Verify the non-vacuity half: the run really decided several entries under one
      leader, or the amortisation is untested
- [ ] 5.3 Verify the identity is asserted at more than one membership size, so that the `n` in it is
      a variable rather than a constant that happens to fit
- [ ] 5.4 State in the suite what the identity does **not** cover: the detector's heartbeats, which
      are per tick rather than per entry, and which are why this capability cannot make the gossip
      pair's claim that an idle run sends nothing. Verify the suite says so rather than leaving a
      reader to wonder why the totals do not match

## 6. Safety, which is the standing risk

- [ ] 6.1 `./scripts/check-safety-tests.sh` passes with every registered test still red under both
      mutations. Changing when a message is resent changes which interleavings a seeded run reaches,
      which is exactly how the decision announcement disarmed four of these two changes ago.
      Investigate any test that goes green rather than deregistering it
- [ ] 6.2 Verify the hand-driven schedules that advance and tick expecting a resend still exercise
      one. Each such test is checked against what it claims to drive rather than merely made to pass
- [ ] 6.3 `cargo test --workspace` passes, and `multi_paxos_replica`'s suite in particular: its
      wedge test depends on the leader answering a re-proposal, and the re-proposal reaching the
      leader depends on this layer's retransmission

## 7. What this dates

- [ ] 7.1 The module's documentation on the retry sweep and on `Timing`. Verify the table of the
      three liveness fixes still reads correctly against the code, since the sweep now does two
      jobs on two schedules
- [ ] 7.2 `README.md`'s real-world set section: the first half of the resource-use obligation is met
      and the second is not. Verify the wording does not now imply Multi-Paxos is in the set, because
      the state is still unbounded and §4.2 is what changes that
- [ ] 7.3 `README.md`'s suite table and counts. Verify against `cargo test --workspace`
- [ ] 7.4 `docs/conditional-guarantees.md`: this is the first module to act on `SessionEstablished`
      rather than only propagate it, and that document is where what a scope event obliges is
      recorded. Verify it says so, and that it does **not** claim the other modules do
- [ ] 7.5 `./scripts/check.sh` passes in full
