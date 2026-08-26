## Context

See `proposal.md` — Why. The relevant state is that both source modules exist and are tested:
`uniform_reliable_broadcast` over `best_effort_broadcast` plus a detector, and
`session_uniform_reliable_broadcast` over `session_best_effort_broadcast` plus a detector and a
resend clause. Each has a suite asserting the four guarantees and a test showing where the
detector's accuracy failing breaks them.

Two facts shape this change. `ack` is already keyed by a broadcast identifier and already counts
distinct relayers, so the majority predicate needs no new bookkeeping — only a different question
asked of the same map. And best-effort broadcast sends to the sender as well as the peers, so a
process's own relay returns to it over the link and counts towards the majority like any other;
the count is out of `N`, not `N − 1`.

## Goals / Non-Goals

**Goals:**

- Algorithm 3.5 transcribed in the book's own setting, with the pseudocode quoted and the single
  changed function marked as such.
- The same predicate over session links, where dropping the detector removes a whole liveness
  path rather than merely a dependency.
- The contrast with the all-ack versions demonstrated on a schedule that breaks them, and
  demonstrated non-vacuously.

**Non-Goals:**

- Collecting `pending`, `ack` or `delivered`. Unchanged from the book and from the modules these
  derive from.
- Touching the all-ack modules. The contrast needs both sides intact.
- Reconciling a minority that was partitioned away for longer than it takes to matter. Under a
  majority quorum a minority simply does not deliver; catching up when it returns is the existing
  resend clause's job and needs nothing new.

## Decisions

**Separate modules rather than a predicate parameter.** The obvious economy is to give the
existing types a delivery-rule knob. It is wrong here for a reason that shows in the type
signature: the majority versions have no `Cmd::Start`, no `Timer::Detector`, no `Wire::Detector`
arm and no `correct` field. That is not a different predicate, it is a smaller protocol with a
different interface, and a knob would have to keep the detector's whole surface present and inert.
Two modules also keep the contrast readable, which is the point of having both.

*Alternative considered:* a generic over the delivery condition. Rejected on constraint 4 — write
the protocols by hand, extract the shared shape later if it earns it. The session broadcast rungs
already established that what looks like duplication is usually the algorithm.

**The wire stops multiplexing.** With one child, the message type is the broadcast child's message
directly rather than an enum wrapping it. The convention this project already follows is that a
layer adding no per-hop state adds no wire field; the same reasoning says a layer with one child
adds no discriminant. This is the first place in the stack where a wire type gets *simpler* going
up, and it is worth noticing that removing an assumption is what did it.

**The predicate is `2 * ack.len() > N`, not `ack.len() > N / 2`.** The book writes
`#(ack[m]) > N/2` with real division. Integer division happens to give the same answer for every
`N`, but only by an argument the reader has to reconstruct; multiplying instead states the
intended meaning directly and cannot be misread. `N` is the full membership including this
process.

**Both suites use five processes, and the contrast tests require it.** This is the decision most
likely to be got wrong silently. The all-ack suite's schedule for breaking uniform agreement
partitions four processes two and two. Run against a majority quorum that schedule produces *no
split* — but only because neither side is a majority and neither side delivers anything, so the
assertion passes vacuously.

With five processes a three-two partition has a genuine majority side. The three deliver, the two
do not, and after healing the two catch up: the "does not split" assertion is then paired with a
positive delivery count on the majority side, which is what makes it mean something. Five also
gives `N > 2f` room for two crashes rather than one, so the crash tests have somewhere to go.

**Nothing is exposed for tests that is not already exposed.** `pending_count`, `delivered_count`
and `acknowledged_by` exist on the all-ack modules and carry over; `correct()` does not, because
there is nothing to report. A test asking "was anyone excluded?" asks it of the wire — no
heartbeat was ever sent — rather than of a field.

**The session version keeps the resend clause verbatim, including its cost.** Re-establishment
sends every pending message to that peer, because the acknowledgement record cannot tell this
process whether its own relay arrived. That reasoning is unchanged by the delivery predicate and
the clause is unchanged with it, unbounded growth included.

## Risks / Trade-offs

- **The contrast test passes vacuously** because a partition leaves no majority anywhere and
  nothing is delivered at all. → Five processes, a three-two split, and an explicit assertion that
  the majority side delivered before asserting that no two processes disagree.
- **A crash test exceeds the assumption without saying so.** With `N = 5`, three crashes leaves
  `N ≤ 2f` and the algorithm correctly blocks; a test that expected delivery would look like a
  liveness bug. → Every test that crashes processes states how many correct remain, and the one
  test that deliberately breaks the assumption asserts blocking rather than divergence.
- **The originator's self-relay is miscounted**, making the predicate off by one and a bare
  majority either too easy or impossible. → A test asserting the exact boundary: with a majority
  minus one relayer, nothing is delivered; with one more, it is.
- **"No detector" is asserted by inspecting the struct rather than the behaviour**, which a later
  refactor could quietly falsify. → Assert it from the trace: every message sent is a broadcast
  payload, and no run requires a start command.
- **The two modules drift from their all-ack originals** as one is edited and the other is not. →
  The suites share their schedules with the all-ack suites deliberately, so a divergence in
  behaviour shows up as a test failure on one side.
