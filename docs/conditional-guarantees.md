# Conditional guarantees

A design note, not a specification. It records a reframing that should shape the rungs above
best-effort broadcast, and the eventual transport layer. Nothing here is implemented yet, and
some of it deliberately should not be until it has a second consumer.

## The problem with the ladder as written

The abstractions in Cachin, Guerraoui & Rodrigues are idealised in two ways that matter as soon
as the code leaves a simulator.

**Stubborn links retransmit forever.** `sent` never shrinks, bandwidth never abates, and there
is no notion of giving up. No running system does this.

**TCP is a perfect link within a session and a liar across one.** Inside a connection you get
ordered, reliable, duplicate-free delivery. Across a reconnect, an unknown suffix of the stream
was lost. A stack built on "perfect link" does not merely degrade at that moment — it carries on
believing a guarantee it no longer holds.

The book concedes the gap. From the discussion of perfect links in Chapter 2: *"The details of
how the perfect links abstraction is implemented are not relevant for understanding the
fundamental principles of many distributed algorithms. On the other hand, when developing actual
distributed applications, these details become relevant."* Chapter 2.7's **logged perfect links**
and the fail-recovery model are the book's own answer to half of it.

## The reframing: every guarantee is bounded by a scope

A guarantee is never absolute. It holds *for as long as some condition holds*, and the useful
engineering question is which condition, and what happens at its edge.

| Scope | Ends when | What may have been lost |
|---|---|---|
| **Session** | the transport reconnects | an unknown suffix of what was in flight |
| **Incarnation** | the process restarts | all state not written to stable storage |
| **Cancellation** | something above says stop | whatever had not yet been delivered |
| **Deadline** | patience runs out | whatever had not yet been acknowledged |

Written out, "PL1: reliable delivery" is really *"if sender and recipient are correct **and the
session persists**, the message is delivered"*. The clause in bold is currently invisible, which
is exactly why it gets forgotten.

**Rule: the end of a scope is a first-class event on the port, not an implementation detail.**
It must be a variant the layer above has to match on, so that ignoring reconnection is a compile
error rather than a silent assumption. The failure mode being avoided is the previous attempt's,
where reconnection was invisible to everything above the link.

## Bridging: which layers can close a gap, and why

Some layers can absorb a scope ending and preserve their guarantee. Others cannot, and must say
so rather than pretend.

**What a layer can bridge is determined by where its redundancy lives.**

| Redundancy lives in | Survives | So it can bridge |
|---|---|---|
| memory | session change | a reconnect — resend what was unacknowledged |
| stable storage | process restart | a crash — the book's *logged* variants |
| other processes | this process dying | a sender crash — reliable broadcast's whole purpose |

That gives a second reading of the ladder. Each rung is not merely "stronger"; it bridges one
more class of failure than the rung below:

```
fair-loss link        bridges nothing — it is the failure
stubborn link         message loss                (within a session, within an incarnation)
perfect link          duplication                 (same scope)
session-aware link    session change              (memory: resend the unacknowledged)
logged link           process restart             (stable storage)
best-effort broadcast nothing new — a sender crash is fatal to its guarantee
reliable broadcast    sender crash                (redundancy across processes)
uniform reliable bc.  sender crash after some have delivered
consensus             disagreement under a minority of crashes
```

Read that way, best-effort broadcast is not a weak version of reliable broadcast. It is the rung
that bridges nothing new, which is precisely why the next rung exists.

**A layer that cannot bridge must propagate.** Silently absorbing a scope end is the bug. If a
perfect link cannot tell whether its peer merely reconnected or restarted with an empty
`delivered` set, it must pass that uncertainty upward rather than resolve it by assumption.

## Consequences for the port

The link port should distinguish two events that are easy to conflate and behave differently:

- **`SessionChanged { peer, epoch }`** — the transport re-established. The peer's state is
  intact; what was in flight may not be. Bridgeable by resending unacknowledged messages.
- **`PeerRestarted { peer, incarnation }`** — the peer lost its state. Its deduplication set is
  gone, so resending may now produce duplicates it can no longer detect. *Not* bridgeable
  without logging.

Telling these apart requires the peer to advertise an incarnation number that survives across
its own restarts — the same device as a boot id or a session id in real systems. Without it, a
reconnect and a restart are indistinguishable, and only the pessimistic reading is safe.

Both variants should be **additive**: the textbook implementation over the simulator never emits
them, so every existing proof and test stays valid, while every layer written from now on is
forced to decide what it does about them.

## What the simulator cannot currently express

`Sim::crash` sets a flag and preserves the protocol's state, so `restart` resumes with everything
intact. That is a pause, not a crash. No real process recovers its memory.

So the simulator today models crash-stop with suspension, and cannot exercise the case a restarted
process actually faces: having forgotten what it delivered. That gap matters by the time uniform
reliable broadcast and consensus arrive, and it is cheaper to close in `recon-sim` than to
discover at rung six.

## What not to build yet

Nothing here justifies restructuring the three protocols that exist.

- **Do not make layers generic over their child yet.** There is no second link implementation to
  be generic over, and building the abstraction before its second consumer is the failure this
  project already documented. The retrofit is contained: best-effort broadcast owns one field and
  makes three four-line composition calls.
- **Do not weaken the perfect link's guarantee yet.** The unbounded version is what the ladder's
  proofs assume. The bounded, session-aware one is a *sibling* implementation of the same port,
  and it belongs with the transport work, which comes last.

What is worth doing early, because the simulator can already produce the fault:

1. Model scope-end events in `recon-sim`, driven by crash/restart, so they can be tested before
   any transport exists.
2. Make crash actually lose state, with an opt-in for the current suspend-and-resume behaviour.
3. Write the boundary down: layers above the link may depend on its `Cmd` and `Ind` types and
   nothing else. That is the seam a second implementation will be swapped through.
