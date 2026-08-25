## Context

See `proposal.md` — Why. Governing constraints are `docs/postmortem.md` §5 and `CLAUDE.md`; the
two documents this change acts on are `docs/conditional-guarantees.md`, which argued for the
session-epoch contract, and `docs/scope-annotated-modules.md`, which proposed and verified the
mechanism but deferred it for want of a consumer.

Six rungs exist. All of them sit, directly or through children, on a fair-loss network. This adds
a second network model rather than replacing the first, because the stubborn and perfect links are
specified against fair-loss and would otherwise become untestable.

## Goals / Non-Goals

**Goals:**

- A network model that resembles what the stack would actually run on.
- The reconnection case — an unknown lost suffix — producible and assertable for the first time.
- A scope mechanism whose first use is real rather than anticipated.
- A link that is bounded by membership, and is therefore the first deployable one.

**Non-Goals:**

- Converting the existing rungs. Their composition is judged separately, on evidence this change
  produces.
- Any transport. No socket, no runtime; this models what a session transport gives, and writing
  the adapter that speaks to one is constraint 5's business.
- Congestion control, flow control, or a send window. Real transports have them; this models the
  guarantees a protocol above can rely on, not the mechanism that produces them.

## Decisions

### 1. Sessions are per unordered pair, not per direction

One session between each pair of processes, ending for both directions at once.

*Why:* it is what TCP and QUIC give — a connection is bidirectional, and losing it loses both
directions. Modelling directions independently would let a protocol observe a state no real
transport produces, which is the failure this change exists to stop.

### 2. FIFO within a session, enforced in the delivery queue

Within a session, messages are delivered in send order. Across a boundary, ordering restarts.

*Why:* the queue currently draws latency per message independently, so messages overtake. A
protocol tested that way has been exercised against reordering TCP never produces and never
against the ordering it does — so it may be unsafe on TCP for a reason the tests cannot show.

*Cost, stated plainly:* this is a real change to the delivery queue rather than a configuration
flag. Latency still varies, but a message may not be delivered before one sent earlier on the same
pair, so the queue must respect a per-pair sequence as well as a timestamp.

*Alternative considered:* keep independent latency and let the session add only reliability. Cheaper,
and rejected — it would produce a model less like the target than the one it replaces.

### 3. A session end discards an unknown suffix, not a known one

On ending, an arbitrary suffix of what was in flight is dropped. The endpoints are told the session
ended; they are not told what was lost.

*Why:* this is the honest contract and the whole point. A real endpoint cannot know how much of its
last write arrived. A model that reported the exact loss would let a protocol be written that
cannot exist.

### 4. Scope ending reaches a protocol through a separate associated type

`Protocol` gains a fifth associated type for the scopes a protocol's guarantees depend on, and a
handler for one ending. A protocol with none declares an uninhabited type and writes no handler.

*Why this and not a message variant:* putting a session end in `Ind` or `Msg` infects every layer
that does not care, forcing each to match a case it can never see. An uninhabited scope type costs
one line per unaffected protocol and makes an ending impossible to construct for it — the compiler
enforces the absence rather than a convention.

*Why now:* the mechanism was verified against the compiler when it was proposed, and deferred for
want of a consumer. Two candidates failed. Uniform reliable broadcast's synchrony assumption has no
boundary any module can observe, so it fails the well-formedness condition. A deduplication window
would qualify but nothing is windowed yet. A session end is produced by the simulator, observed by
the link, and injectable by a test — it satisfies the condition outright, and it is the first thing
that does.

*Cost:* a fifth associated type on six existing protocols, each declaring it has no scopes, and a
fourth mapper in composition.

### 5. The session link holds state per peer and nothing per message

Its guarantees come from the session, so it needs no retransmission buffer and no deduplication
set — within a session the transport does not duplicate. It holds an epoch per peer and nothing
else that grows.

*Why this matters beyond tidiness:* it is the first link in the repository that satisfies the rule
in `docs/bounded-space.md`, and the demonstration that the deployable link needs *less* state than
the academic one, not more. Its specification carries that as a requirement, so a later change
cannot quietly reintroduce a per-message record.

### 6. The existing rungs are not converted

They stay on fair-loss, unchanged and passing.

*Why:* converting four working rungs in the same change that introduces what they would move onto
would make a failure ambiguous between the new model and the new composition. The conversion is a
separate change with its own evidence — and it is a real one, because a layer above a session link
must decide what a session end means to *its* guarantee, which is the question
`docs/conditional-guarantees.md` frames as bridging or propagating.

## Risks / Trade-offs

**Per-pair ordering makes the delivery queue harder to reason about, and the queue is the source of
determinism** → a subtle change there breaks reproducibility everywhere, and silently. Mitigation:
the existing determinism tests run unchanged on the fair-loss default, and the session mode gets
its own — same seed, byte-identical trace — before anything is built on it.

**A fifth associated type is a wide, shallow change** → six protocols and every composition call,
all mechanical, all at once. Mitigation: it is mechanical, the compiler finds every site, and the
uninhabited declaration means an unaffected protocol gains one line and no behaviour.

**The session model becomes the default in new tests because it is pleasanter** → and the fair-loss
behaviour that six rungs depend on stops being exercised. Mitigation: the existing suites are not
migrated, and the mode stays opt-in per run.

**"Unknown suffix" is easy to implement as "always everything" or "always nothing"** → either would
pass a loose test while modelling nothing. Mitigation: assert that across many seeds the amount
lost varies, and that both extremes occur.

## Open Questions

- **Whether a session should be able to end without either process being at fault** — a transport
  can drop a connection for its own reasons. Adding an explicit break knob covers it; whether the
  model should also do it spontaneously at some rate is a fidelity question that changes no
  requirement here.
- **What the session link should do about messages it was asked to send while no session exists.**
  Queue, refuse, or drop. It affects the link's interface but not the guarantees above it, and is
  better settled against a real caller.
