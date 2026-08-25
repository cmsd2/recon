# What the first change to the simulator cost

Task 4.1. Two questions were asked in advance: what a protocol needs when its output is
indications about *processes* rather than delivered payloads, and what the first change to
`recon-sim` since it was built would require.

## The `Protocol` trait needed nothing

The detector's `Ind` is `Crash { node }` and its `Msg` is a unit struct. No payload is delivered
upward, nothing is composed beneath it, and it is the first protocol whose whole purpose is to
report on peers. It fits the existing trait unchanged.

That is a genuine result rather than an absence of one. The trait was designed against three
protocols that all carried payloads through a stack, and the worry was that it had been shaped by
that. It had not been. `Cmd`/`Ind`/`Msg`/`Timer` turned out to be the right four ports for a
protocol that delivers no data at all.

The detector is also the first to use `cx.now()` for anything load-bearing — it records when each
peer was last heard from and compares against a timeout. Time being supplied rather than read is
what makes its tests exact instead of flaky.

## The simulator needed two things, and only one was planned

**Planned: a synchronous mode.** As the proposal argued, perfect detection is impossible in an
asynchronous model, so the simulator gained a bound. About 40 lines across `config.rs` and
`sim.rs`, plus the enforcement detail that the promise is applied at delivery time rather than by
zeroing the fault knobs — so `.synchronous(d).loss(0.9)` cannot quietly reintroduce loss. There is
a test for exactly that.

**Unplanned: a defect in suspension.** A timer coming due while a process was suspended was
*consumed* rather than deferred: `dispatch` saw a stopped process and returned, and the timer was
gone. The process resumed alive but permanently inert.

This contradicted the simulation spec already accepted in the previous change — *"a suspended
process resumes with its state intact… its pending timers still fire"* — and the existing test for
that scenario passed only because its timer came due *after* the resume. A pending timer is
process state; a suspension preserves state; therefore a suspension must hold timers and re-arm
them on resume, while a crash still destroys them.

It was found by `a_brief_suspension_is_not_an_accusation` failing. Nothing else in four rungs had
both a periodic timer and a reason to be paused, so nothing else could have found it.

## The heartbeat period had to be separated from the detection timeout

Algorithm 2.5 uses one delay Δ for both. Implemented literally, a single missed round is fatal:
a process stalled for an instant spanning its own send is accused, though it is alive and the
network kept its promise. That is not a violation of the book — its Δ is meant to cover a full
request/reply round — but it makes the abstraction brittle in exactly the case that matters, and
the specification written for this change already required tolerating a brief stall.

Beating every `period` and accusing after `timeout` of silence gives a tolerance of
`timeout − period − Δ`. Accuracy needs `timeout > period + Δ`, which is asserted in the
constructor and documented at the type.

This was also found by a failing test rather than by reading, which is now the third time in this
project that a confident reading of the book turned out to need the simulator to settle it.

## For uniform reliable broadcast, which follows

- The detector is a peer of best-effort broadcast, not a layer beneath it. URB will own both, and
  will be the first protocol with two children — the case the reliable-broadcast notes named as
  the one that would reopen the macro question. Two `with_*` helpers with near-identical bodies is
  the evidence to look for.
- URB inherits the timing assumption through P. Its own specification says nothing about timing,
  and should say something: Algorithm 3.4's guarantees are conditional on the detector's, which are
  conditional on the network's. This is the clearest case yet for the `Scope` associated type of
  `docs/scope-annotated-modules.md`, and the second consumer that decision was waiting for.
