## Context

See `proposal.md` for the measurement. What follows is how the three parts fit together and where
each could go wrong.

The sweep as it stands does four jobs on one timer at `Timing::retransmit`:

1. resend `p1a` to acceptors a running scout has not heard from;
2. escalate a scout that has waited longer than `escalate_after` — restart phase one;
3. escalate a commander that has waited longer than `escalate_after` — back to phase one, because a
   lost `preempt` means no `p2a` can ever land;
4. resend `p2a` to acceptors a commander has not heard from.

Only 1 and 4 are retransmission. 2 and 3 are the liveness fixes from the cross-check, they are
already gated on `escalate_after`, and they are not what this change touches.

## Goals / Non-Goals

**Goals:**

- An asserted closed form for what a run costs, per leadership change and per entry.
- Retransmission whose rate is set by the delivery bound and by scope events, not by the sweep tick.
- A self-addressed message that is a call.

**Non-Goals:**

- Bounding state. §4.2, and it is the change after this one.
- Generalising the `SessionEstablished` resend to other modules. One consumer first.
- Changing `escalate_after` or either escalation. They are liveness, not cost.
- Batching, pipelining depth, or any optimisation not in the source.

## Decisions

### The identity is stated per leadership change and per entry, not per entry alone

An average over a run folds phase one into the per-entry figure and hides the thing worth showing.
So the test asserts two numbers against a run whose leadership changes it counts:

- per leadership change that completes: `n` `p1a` and `n` `p1b`;
- per entry decided: `n` `p2a`, `n` `p2b`, and `n − 1` `decision` — becoming `n − 1`, `n − 1` and
  `n − 1` once self-addressed messages are calls.

Measured against the current code with `retransmit` well above the bound, both hold exactly, which
is what makes them assertable rather than approximate. A test that asserted `≤ 4n` per entry would
pass the 3.6× run this change exists to fix.

*Alternative considered:* a ratio against `consensus_based_total_order_broadcast` over the same
workload, which is the comparison the reader actually cares about. Rejected as a test: it couples
two modules' costs so that tuning either breaks it, and the port suite is where the two are compared
on behaviour rather than on price. The comparison belongs in the README.

### Retransmission is timed against the delivery bound, and the sweep becomes a backstop

`Timing` gains nothing. The commander and the scout already record `started`; the sweep resends only
where `now - started` exceeds a threshold derived from the timing already configured, and the
obvious derivation is the one `detect_after` already uses — a small multiple of the bound the
detector's own note names. The sweep interval then controls *granularity*, and the threshold
controls *rate*, which is the separation conflating them lost.

The threshold has to exceed one round trip and be well below `escalate_after`, or escalation fires
before a retransmission has had a chance and phase two restarts for a message that was merely in
flight. Both ends are checkable, so the module states them and a test pins the ordering rather than
leaving it to whoever configures `Timing`.

*Alternative considered:* per-destination backoff, as §3's pinging scheme has. Rejected: it is
state per peer per attempt for a case the session link already handles, and this module deliberately
took Ω over §3's scheme once already.

### `SessionEstablished` resends immediately, and this is the honest half

A session ending is the only way this stack loses a message, and the establishment that follows is
the only moment a resend can succeed. Acting on it means a scout or commander whose request died at
an ending recovers in one delivery rather than in a timeout, and it means the timeout above can be
generous without costing recovery time — the two decisions support each other.

What it must not do is resend to a peer whose session is *not* established, which is the failure a
naive implementation has: a broadcast on every establishment, to everybody. The resend is to the
peer the event names, and only for attempts that peer has not answered.

*Alternative considered:* treating an ending as an immediate answer-is-lost signal and resending at
the ending rather than at the establishment. Rejected because there is nothing to send into: the
session is gone, and the link would drop it. The establishment is the moment, which is what
`session_link.rs` says.

### A self-addressed message is the driver's business, not the protocol's

Written first in `MultiPaxosSynod::transmit`: dispatch to the handler where `to == self.me`. It
worked, and it was wrong for a reason the first draft did not see.

**It made the exchange invisible to the trace.** The hand-driven harness files every send into its
wire and the sim-driven helpers read `trace().sends()`, so a hand-off that never becomes an effect
is one neither can observe. Measured, `preemptions()` — the non-vacuity floor under
`the_safety_suite_is_not_vacuous` and `at_most_one_proposal_is_chosen_per_slot_under_competing_
ballots`, both **registered safety evidence** — returned exactly `0` in runs that still ran three
and five distinct ballots and still chose proposals. Every preemption those runs contained was a
leader learning of a higher ballot from its own acceptor, and no rewrite of the schedules brings it
back, because the event genuinely stopped existing.

A protocol that hides an exchange from the trace weakens the suite's evidence in a way nothing
would catch later. So the shortcut belongs where the distinction actually lives: **a driver is what
turns an `Effect::Send` into a packet**, and it is what should notice the packet is addressed to
the process it came from. The protocol goes on emitting the effect, the simulator hands it over at
the current instant without a latency, and the trace records it as its own kind of event — so it
remains observable while ceasing to be a network message. Every protocol gets it, not only this
one.

*Alternative considered:* keeping the protocol-side dispatch and rewriting `preemptions` to read
something else. Rejected: there is nothing else to read. The refusal only ever existed as a
message.

### What the hand-off actually removes, and what it does not

The proposal first claimed it removed a fault — a session ending dropping a process's message to
itself. **It does not, because that fault does not exist.** `Sim::connected(a, a)` is
unconditionally true and `Sim::ensure_session(a, a)` returns without creating a session, so a
self-addressed message has always been unloseable. The claim was wrong and is corrected in the
proposal rather than quietly dropped.

What it removes is real and is two things. The message is **counted** as a network send when it is
not one, which is the identity's `n` against `n − 1`. And it is **delayed by a full network
latency**, which is not bookkeeping: a leader waits a delivery bound for its own acceptor's answer,
lengthening both phases, and a longer phase is more sweeps — so this is part of why the 3.6× figure
is what it is. Correcting it makes the retransmission threshold's job smaller.

### The simulator invariant, answered

`CLAUDE.md` requires a new simulator capability to be checked against
`docs/conditional-guarantees.md` with one question: **can it lose something without raising the
event that says so?**

No, and it strictly narrows what can be lost. A hand-off to oneself is delivered at the instant it
is made, so it cannot be delayed, dropped, duplicated, reordered, or caught by a session ending; it
bypasses every fault the network applies rather than being subject to one silently. There is no
scope to end, because there is no session between a process and itself and never was. And it is
recorded, so nothing that happens goes unrecorded.

One thing follows that is worth stating rather than discovering: it is delivered **at** the current
instant and not *within* the current handler. That is the ordinary effect model — every effect a
handler emits is applied after it returns — and it means the simulator does not run a handler
inside another handler's effect loop. The protocol-side draft would have had to argue that the
nesting terminates; this one has no nesting to argue about.

## Risks / Trade-offs

- **Changing when a message is resent changes which interleavings a seeded run reaches** → this is
  how the decision announcement silently disarmed four safety tests two changes ago. The safety
  guard runs as part of this change, not after it, and any registered test that goes green is
  investigated rather than deregistered.

- **Hand-driven schedules assume the sweep resends on every tick** → several tests advance by
  `detect_after` and tick, expecting a resend. They will need to advance past the new threshold
  instead. That is a mechanical change, but a test that quietly stops exercising a resend is the
  hazard, so each one touched gets checked against what it claims to drive.

- **Re-entrancy from self-dispatch** → argued and tested, per the decision above. If the argument
  does not hold, the fallback is a one-element deferral queue, which is more machinery than the
  saving justifies; in that case the self-send stays on the wire and the identity is stated as
  `3n − 1`.

- **The identity is exact, so it is brittle** → deliberately. An exact number that breaks when the
  algorithm changes is the point; the gossip pair's identity has the same property and has caught
  real regressions. What would make it brittle in the bad sense is asserting it over a lossy run,
  so it is asserted over a run whose faults are stated and counted.

## Open Questions

- **The retransmission threshold as a multiple of the bound.** It must exceed one round trip and sit
  well below `escalate_after`. `Timing` names `heartbeat` and `detect_after` already; whether the
  threshold is derived from those or named separately is decidable once the first schedule is
  written, and it changes no spec either way.
