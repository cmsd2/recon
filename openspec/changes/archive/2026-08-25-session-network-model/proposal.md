## Why

The simulator models a fair-loss datagram service. That is what you have if you build on UDP and
construct reliability yourself — which is what the stubborn and perfect links do, and why they
exist. But `docs/bounded-space.md` records that neither would ship: TCP and QUIC already
retransmit, and the deployable link is a *session link* that takes its guarantees from the
transport and reports when a session ends.

So every rung above the link layer is currently tested against a network that does not resemble
the one it would run on. It has been exercised against packet loss, which the transport hides, and
never against session loss, which the transport cannot hide and which is the failure that actually
reaches it.

That failure is also, at present, untestable at any layer. `docs/conditional-guarantees.md` was
written about reconnection with an unknown lost suffix, and nothing in the repository can produce
one.

## What Changes

- **A session network model in the simulator**, alongside the existing fair-loss behaviour rather
  than replacing it. Between each pair of processes there is a session: within it, delivery is
  reliable, ordered and free of duplicates, as TCP gives; a partition, a crash, or an explicit
  break ends it, losing an unknown suffix of what was in flight, and a new session begins at a
  higher epoch.
- **FIFO delivery within a session.** The delivery queue currently draws latency per message
  independently, so messages overtake one another. A protocol tested that way has been exercised
  against reordering TCP never produces and never against the ordering it does, so the queue gains
  per-pair ordering when a run is session-based.
- **A way for a session change to reach a protocol.** The model is pointless without it: the whole
  interest is the reconnection case, and a protocol that is not told about it cannot react, be
  tested, or be held to anything. `Protocol` has handlers for commands, messages and timers, and a
  session ending is none of them.
- **One session-aware link** that uses both, as the first consumer.

The six existing rungs stay on fair-loss and are not converted. That is a separate judgement, to be
made on evidence this change produces rather than in the same change that introduces the thing they
would move onto.

## Capabilities

### New Capabilities

- `links/session-link`: A link whose guarantees come from an underlying session rather than from
  retransmission, and which reports when that session ends and a suffix may have been lost.

### Modified Capabilities

- `simulation`: gains the session network model — reliable ordered delivery within a session,
  session ends on partition, crash or explicit break, suffix loss, and epochs. The fair-loss
  behaviour is unchanged and remains the default.
- `protocol-core`: gains the means by which a scope ending — of which a session end is the first
  real instance — reaches the protocol that must react to it.

## Impact

- **This is where the deferred scope mechanism gets a consumer.** `docs/scope-annotated-modules.md`
  proposed representing a scope's end as a separate associated type, verified the mechanism
  compiles, and deferred it for want of anything that needed it. Two candidates have since been
  rejected or left hypothetical: uniform reliable broadcast's synchrony assumption fails the
  well-formedness condition, having no boundary any module can observe, and a deduplication window
  is plausible but nothing is yet windowed. A session end is different: the simulator produces it,
  a protocol observes it, a test injects it. It satisfies Definition 2a outright.
- **Constraint 5 is not being broken.** No socket is opened and no runtime is added. Modelling what
  a session transport provides is the same work the simulator already does for fair-loss; writing
  the adapter that speaks to a real one is what comes last.
- **Cost.** Per-pair ordering is a real change to the delivery queue, not a configuration flag, and
  a fifth associated type touches every existing protocol — each declaring that it has no scopes.
