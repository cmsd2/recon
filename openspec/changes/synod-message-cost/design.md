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

### A self-addressed message becomes a call

`transmit(to, msg)` with `to == self.me` dispatches to the handler directly instead of going through
the link. The handler is the same function the delivery would have reached, so nothing about the
protocol's logic changes; what changes is that it is synchronous, cannot be delayed, and cannot be
lost.

That last point is the reason this is correctness and not only cost. A process cannot fail to
deliver a message to itself, and the simulator currently can break a self-session and make it do so.
Any run that depended on that was exercising an impossible fault.

The risk is re-entrancy: `transmit` is called from inside handlers, and dispatching to a handler
from inside a handler is how a stack overflows. The chain is bounded in fact — a `p2a` to self
produces a `p2b` to self, which either completes the commander or does not, and a `decision` is
raised rather than sent to self — but "bounded in fact" is what the module has to argue rather than
assume, and the tasks require the argument and a test that the depth is what the argument says.

*Alternative considered:* keeping self-sends on the wire and excluding them from the count in the
test. Rejected: it makes the test the place where the deployment's shape is stated, which is exactly
backwards, and it leaves the impossible fault in place.

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
