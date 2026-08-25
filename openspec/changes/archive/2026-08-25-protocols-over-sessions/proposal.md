## Why

The session link models what a deployed stack sits on, but nothing sits on it. The six existing
rungs still run over the perfect link, which retransmits for ever and never ends — so they have
never been exercised against session loss, which is the failure a real transport cannot hide.

Running them there is not merely a substitution. It changes what two of them can promise, and the
difference between the two is the point:

- **Reliable broadcast is not live over a session link.** Eager relay is fire-and-forget, it keeps
  identifiers rather than payloads, and Algorithm 3.3 is fail-silent — so when a relay is lost to a
  session ending, nothing retries and nothing declares the peer gone. Its agreement is scoped.
- **Uniform reliable broadcast is live**, because between the link and the failure detector there
  is no third outcome. Either a session is established again — the link reconnects on its own, as a
  deployed one would — in which case it resends what that peer has not acknowledged; or the peer
  stays silent and is accused, in which case `correct` shrinks and `correct ⊆ ack[m]` is satisfied
  without it.

That is the sharpest available argument for why uniform reliable broadcast needs a failure detector
at all, and it is only visible on a link that can lose a suffix.

## What Changes

- **A session ending and a session establishment become two events, not one.** The simulator
  currently reports only the ending, and names an epoch it can only predict. Both events are real
  and neither substitutes for the other. An ending is known synchronously at the moment of failure
  — an operating system closes the handle and the next read or write errors — and it tells a
  protocol that its last writes may be gone. But it cannot be acted on, the peer being unreachable.
  An establishment is what can be acted on, and it happens at a moment neither end fully controls:
  it may be provoked by a heartbeat, by an application send, or by the peer connecting inward.
- **A link that reconnects on its own.** The simulator establishes a session as soon as one is
  possible, rather than waiting for something above to transmit — modelling the link a deployment
  would have, which keeps retrying and reports its epoch and connected status. This is the stubborn
  idea applied to the right object: retrying a *connection* is bounded by membership, where
  retransmitting every *message* is bounded by nothing.
- **The ending names the epoch that ended**, rather than predicting the next. At the moment of
  failure the next epoch is not a fact, and may never become one.
- **Session-carrying variants of the three broadcast rungs**, as separate modules beside the
  existing ones rather than replacing them.
- **One new `upon` clause in uniform reliable broadcast**, and nothing else: on a session being
  *established*, re-broadcast the pending messages that peer has not acknowledged. Not on the
  ending, where there would be nothing to send over. It uses only state Algorithm
  3.4 already keeps and an action it already performs — no new message type, no acknowledgement
  protocol, no tracking that the book does not have.
- **Scoped guarantees, stated.** Best-effort and reliable broadcast say what they no longer promise
  across a session ending. Uniform reliable broadcast keeps both of its guarantees, and says what
  they now rest on.

## Capabilities

### New Capabilities

- `broadcast/session-best-effort-broadcast`: fan-out over a session link, whose validity holds
  within a session and not across one.
- `broadcast/session-reliable-broadcast`: eager reliable broadcast over a session link, whose
  agreement is scoped for want of any means to retry or to detect.
- `broadcast/session-uniform-reliable-broadcast`: uniform reliable broadcast over a session link
  and a failure detector, which keeps both guarantees because those two mechanisms between them
  leave no third outcome.

### Modified Capabilities

- `simulation`: session establishment becomes a reported event alongside the ending.
- `links/session-link`: the link reports both, and the ending names the epoch that ended rather
  than predicting the next.

## Impact

- **Why separate modules and not one generic layer.** These are not the same algorithms over
  different links. Uniform reliable broadcast over a session link has an `upon` clause the book's
  version does not, and reliable broadcast over one has a weaker guarantee. Parameterising a single
  layer would require a shared link port and would give every layer above the perfect link an
  indication arm it can never reach, to unify algorithms that genuinely differ. The book itself
  presents Lazy and Eager reliable broadcast side by side rather than as one algorithm configured
  two ways.
- **Nothing existing is converted.** The six rungs over the perfect link stay exactly as they are,
  with their tests untouched, so the academic ladder remains exercised end to end.
- **The bounded-space rule bites here.** Reliable broadcast could be made live by retaining
  payloads and resending, but that is state growing with messages, which is forbidden without a
  window. Recording that it is *not* live is the honest alternative and costs nothing.
