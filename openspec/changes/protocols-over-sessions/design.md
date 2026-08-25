## Context

See `proposal.md` — Why. The documents this change acts on are `docs/conditional-guarantees.md`,
which framed bridging and propagating, and `docs/bounded-space.md`, which forbids the state that
would make reliable broadcast live.

Seven rungs exist. Six run over the perfect link, which retransmits for ever; the seventh is the
session link, which does not and says so. This change puts three of the six over the seventh,
beside their originals rather than in place of them.

## Goals / Non-Goals

**Goals:**

- Each converted rung's guarantee stated as what actually holds over a link that can lose a suffix.
- The difference between reliable and uniform reliable broadcast made observable, since it is the
  whole argument for the failure detector.
- No new message, no acknowledgement protocol, no state the book's algorithms do not keep.

**Non-Goals:**

- Converting the existing rungs. They stay on the perfect link, unchanged and tested.
- Making reliable broadcast live. That needs retained payloads and retry, which is state growing
  with messages and is forbidden without a window.
- Windowing anything. The bounded-space work is a separate change with its own guarantees.

## Decisions

### 1. An ending and an establishment are two events, and both are real

The simulator reports only the ending, naming a predicted next epoch. That conflates two things a
real endpoint learns separately.

**An ending is synchronous and knowable.** The operating system closes the handle; the next read or
write errors. A protocol learns at the moment of failure that its last writes may be gone. What it
cannot do is anything about it — the peer is unreachable, and anything sent is discarded.

**An establishment is asynchronous and not under the layers' control.** It happens when the link
manages to reconnect. A deployed link keeps trying on its own, with or without backoff, and reports
its epoch and connected status upward — so establishment is the link's business, not something the
layers above provoke by happening to transmit. See decision 7.

So the ending names the epoch that *ended* — the next is not yet a fact and may never become one —
and the establishment names the epoch now in force. A layer that must resend can only do so on the
second.

*Why not just report the establishment:* the ending is genuinely informative even though it is not
actionable. A layer above may want to stop waiting on something, surface a warning, or record that
a guarantee lapsed. Discarding a signal a real transport provides would make the model less
faithful, not simpler.

### 2. Separate modules, not one layer generic over its link

These are not the same algorithms over different children. Uniform reliable broadcast over a
session link has an `upon` clause the book's version does not. Reliable broadcast over one has a
weaker guarantee. Parameterising a single layer would need a shared link port and would give every
layer above the perfect link an indication arm it can never reach — unifying, at some cost,
algorithms that genuinely differ.

The book does the same: Lazy and Eager reliable broadcast appear side by side rather than as one
algorithm configured two ways.

*Cost, stated plainly:* three modules that resemble their originals. The resemblance is real and a
reader will notice it; what differs is small and is exactly where the interest lies, so each module
says at the top what it changed and why.

### 3. Uniform reliable broadcast gains one `upon` clause and nothing else

```text
upon event ⟨ SessionChanged | q ⟩ do
    forall (s, m) ∈ pending such that q ∉ ack[m] do
        trigger ⟨ beb, Broadcast | [DATA, s, m] ⟩;
```

`pending` already holds payloads and `ack` already records who has been seen to acknowledge what,
both from Algorithm 3.4. The action is the broadcast the algorithm already performs. So this adds
no state, no message type and no round trip — only a new trigger for an existing action.

*Why re-broadcast rather than send to `q` alone:* the book's vocabulary at this layer is
`beb, Broadcast`. A targeted send would be a new communication pattern; re-broadcasting is not.
It costs traffic, which is recorded rather than optimised.

### 4. Reliable broadcast propagates and its agreement is scoped

It could be made live by retaining payloads and resending. That is per-message state without a
window, which `docs/bounded-space.md` forbids, and it is the tracking this change was told not to
add.

So it reports the change upward and its specification says agreement holds within the sessions
carrying the relay. This is a true statement about Algorithm 3.3 over a real link, and it is more
useful than a version that quietly fails.

### 5. Best-effort broadcast propagates because it cannot do otherwise

It holds nothing but the process set. Absorbing the report would deny the layers above their only
signal, and it has nothing with which to repair the loss itself.

### 6. Liveness for uniform reliable broadcast rests on there being no third outcome

Either a session is established again — in which case decision 3 resends what was missed — or the
peer stays silent and the detector accuses it, in which case it leaves `correct` and
`correct ⊆ ack[m]` no longer waits for it.

The two halves come from different places, and it is worth being precise about which, because an
earlier draft of this design got it wrong. **The link** produces the establishment, by retrying on
its own (decision 7). **The detector** produces the accusation, by timing out.

The earlier draft had the detector's heartbeats provoking establishment, on the grounds that a
heartbeat is a send and a send brings a session up. That works, but it makes liveness incidental to
a protocol that exists for another purpose — and it would fail for a stack with no detector, or one
whose detector was configured with a long period. Putting reconnection in the link removes the
coupling: a peer that is reachable will be reconnected to whether or not anything above is
interested.

This is worth stating as the design's central claim because it is what makes the uniform rung
different from the one below, and it is testable directly: a partition shorter than the detection
timeout must resolve by resend, and one longer than it by accusation.

### 7. The link is stubborn about connections, not about messages

A deployed link keeps trying to reconnect until it succeeds, reporting its epoch and connected
status upward. The simulator models the same, establishing a session as soon as one is possible
rather than waiting to be prompted.

*Why this is not the stubborn link returning:* the idea was right and the object was wrong.
Retransmitting every **message** for ever is unbounded in both state and traffic, which is why
`docs/bounded-space.md` calls that rung academic. Retrying the **connection** is bounded — one
pending attempt per peer, state proportional to membership — and is what every real transport
wrapper does. `docs/postmortem.md` marked exactly this as an idea worth keeping from the previous
attempt's `conn.rs`: retry with backoff, a session id that increments on reconnect, and the new
epoch announced to the layer above.

*Consequence for the model:* a healed partition or a restarted process reconnects with nothing sent
from above, which is what the specification now requires. Backoff is permitted but not required;
the simulator may delay before establishing, and no protocol may depend on how long it takes.

## Risks / Trade-offs

**The two liveness paths overlap and a test may pass by the wrong one** → a partition that heals
just as the detector fires could be resolved by accusation while appearing to test resending.
Mitigation: test the two separately with partitions well inside and well outside the detection
timeout, and assert which mechanism fired by inspecting `correct` as well as the deliveries.

**Re-broadcasting on every re-establishment could amplify** → a flapping session produces a burst
per flap, proportional to what is pending. Mitigation: none in this change; it is bounded by
`pending`, which is bounded by nothing, so it is recorded as a consequence of the existing
unbounded state rather than a new problem.

**The new modules read as copies** → and a reader may not see what differs. Mitigation: each states
its difference at the top, and the reliable and uniform versions are specified against each other
so the divergence is the documented content rather than an accident.

**Changing when the report is made could break the session link's existing tests** → they were
written against reporting at the ending. Mitigation: they are part of this change's delta and are
expected to move; the ones asserting a lost suffix is never reported as delivered must survive
untouched, since that is a safety property and unaffected.

## Open Questions

- **Whether the perfect-link stack should eventually be retired from above.** Once the session
  stack is trusted, keeping six rungs over a link nothing would deploy is a maintenance cost with a
  teaching benefit. Not a question this change needs to answer.
- **Whether `pending` should be pruned once a message is delivered everywhere.** Uniform reliable
  broadcast computes stability already; doing it would bound the resend burst as well as the state.
  It belongs with the bounded-space work, not here.
