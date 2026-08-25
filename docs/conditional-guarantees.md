# Conditional guarantees

A design note, not a specification. The formal argument — what the scope annotation means, why
every proof in Cachin, Guerraoui & Rodrigues survives it untouched, and what it does and does not
add — is in [`scope-annotated-modules.md`](scope-annotated-modules.md). It records a reframing that should shape the rungs above
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

The link port gains exactly one event:

- **`SessionChanged { peer, epoch }`** — the transport session ended and a new one began.
  Anything in flight may have been lost. Bridgeable by resending what was unacknowledged.

**And no more than that.** It is tempting to add a second event distinguishing "the peer
reconnected" from "the peer restarted and forgot everything", because the two have very different
consequences: after a restart the peer's deduplication set is gone, so resending may produce
duplicates it can no longer detect. But a link cannot tell them apart. On the wire a reconnect
looks identical whether the peer rebooted or a switch blipped. Distinguishing them requires the
peer to advertise an incarnation identifier that is monotonic across its own restarts, which
requires stable storage at the peer and a handshake to convey it — none of which is a transport
concern. A link reporting a peer restart would be asserting something it has no means to know.

This is the reason the higher protocols carry epochs of their own. View numbers in viewstamped
replication, terms in Raft, and incarnation numbers in gossip membership are not transport facts
relayed upward; they exist at that level *because* the level below cannot supply them, and must
be obtained instead from persistence or from agreement. `scope-annotated-modules.md` makes this
precise as Theorem 8 and its corollaries.

There is a second conflation worth avoiding. The scope of "no duplication" is the **local**
process's incarnation, not the peer's: it is *my* deduplication set that is volatile, so it is
*my* restart that lets me deliver again. That boundary needs no event at all — it is the
process's own `⟨Init⟩`, which the book's notation already has, and there is nobody to notify,
because the process that would raise the event is the one that ceased to exist.

`SessionChanged` should be **additive**: the textbook implementation over the simulator never
emits it, so every existing proof and test stays valid, while every layer written from now on is
forced to decide what it does about it.

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

## Extending the book's notation to say this

The module blocks in Cachin, Guerraoui & Rodrigues state properties as prose with an implicit
universal scope. Module 2.3, perfect point-to-point links, reads:

```
Properties:
    PL1: Reliable delivery: If a correct process p sends a message m to a correct
         process q, then q eventually delivers m.
    PL2: No duplication: No message is delivered by a process more than once.
    PL3: No creation: If some process q delivers a message m with sender p, then m
         was previously sent to q by process p.
```

Read against the previous section, those three properties do not share a scope, and the prose
hides the difference:

- **PL1** holds while the session persists — or while a retransmission buffer outlives it.
- **PL2** holds while the `delivered` set persists. A process that restarts having forgotten what
  it delivered can duplicate again.
- **PL3** is pure safety. It holds unconditionally.

### The proposed extension

Two additions to a module block: a **Scopes** section naming the conditions, and a bracketed
scope tag on each property. Plus a **Bridges / Propagates** section stating what the module does
when a scope it depends on ends.

```
Module 2.3′: Session-aware perfect point-to-point links

Name: PerfectPointToPointLinks, instance pl.

Scopes:
    session(q)       the transport session with q; ends on reconnect
    incarnation(p)   p's volatile state; ends when p restarts

Events:
    Request:    ⟨ pl, Send | q, m ⟩
    Indication: ⟨ pl, Deliver | p, m ⟩
    Indication: ⟨ pl, SessionChanged | q, e ⟩     ends session(q)

Properties:
    PL1 [session(q)]   Reliable delivery: if a correct p sends m to a correct q,
                       then q eventually delivers m.
    PL2 [incarnation]  No duplication: no message is delivered by a process more
                       than once.
    PL3 [always]       No creation: if q delivers m with sender p, then m was
                       previously sent to q by p.

Bridges:
    session(q)   by retaining unacknowledged messages in memory and resending them,
                 restoring PL1 across the boundary.
Propagates:
    nothing      the incarnation boundary is this process's own ⟨Init⟩; there is
                 nobody to notify, and the peer's incarnation is not observable
                 from here at all.
```

### How to read a scope tag

`P [S]` means: **P holds over any interval throughout which S holds continuously.** An
unconditional property is the degenerate case, `[always]`. This is the ordinary way of scoping a
temporal property, so nothing new is being invented — it is only being written down where the
book leaves it implicit.

### Why it is worth the extra lines

**It maps onto the code exactly.** A scope in the notation is a variant on the port; ending a
scope is emitting that variant; *Bridges* is handling it; *Propagates* is re-emitting it. The
notation and the type say the same thing, which is what makes the annotation load-bearing rather
than decorative.

**Each tag is a test specification.** `PL2 [incarnation(q)]` says directly: write a run in which
`q` restarts having lost state, and check that duplication becomes possible. That test does not
exist yet, and neither does the simulator capability to write it — which is itself the point. A
property tagged `[always]` should have a test that survives every fault the simulator can inject;
a property tagged with a scope should have a test that it does *not* survive that scope ending,
unless the module claims to bridge it.

**It makes the ladder's structure explicit.** Reading the rungs by their tags shows where each
one strengthens a scope: reliable broadcast takes best-effort broadcast's validity from
`[sender's incarnation]` to `[always]`, and it does so by moving the redundancy from the sender's
memory onto other processes. That is the same table as the previous section, arrived at from the
notation rather than from the implementation.

### The cost, honestly

Every module definition grows by six or eight lines, and most of the early rungs will be tagged
`[always]` or `[session]` with nothing interesting to say. The notation earns its keep from
reliable broadcast upward, where bridging is the entire content of each rung, and it is largely
ceremony below that.

It is also a private extension. Anyone reading this code against the book will meet a notation
the book does not use, so the mapping has to be stated once — here — and referenced rather than
re-explained per module.

## Keeping the book versions and the real versions side by side

The obvious worry about adding `SessionChanged` is that it infects everything. It would, if it
were a variant of the link's `Ind`: every layer above would have to match a case the textbook
stack can never produce, and the algorithms would fill with dead branches — the opposite of code
that reads like the page.

It does not infect them if scope ends are their **own associated type** rather than an indication.

```rust
trait Protocol {
    type Cmd;
    type Ind;
    type Msg;
    type Timer;
    type Scope;                                   // the notation's `Scopes:` section

    fn on_scope_end(&mut self, _s: Self::Scope, _cx: &mut ProtoCx<'_, Self>) {}
}
```

The book version declares that it has no scope conditions, and writes no handler:

```rust
impl Protocol for PerfectLink<P> {
    type Scope = core::convert::Infallible;       // every property [always]
    // no on_scope_end — nothing to write, and no branch to read past
}
```

The session-aware version declares what it is bounded by, and handles it:

```rust
impl Protocol for SessionLink<P> {
    type Scope = SessionScope;                    // SessionChanged, and nothing more
    fn on_scope_end(&mut self, s: SessionScope, cx: &mut ProtoCx<'_, Self>) { … }
}
```

Two properties make this work, and both were checked rather than assumed:

- **A scope end cannot be constructed for the book version.** `Infallible` is uninhabited, so
  there is no value to pass. A driver generic over `P` cannot inject one; the impossibility is a
  type error, not a convention.
- **An exhaustive match on an uninhabited type needs zero arms.** Where a handler is written
  generically, `match s {}` is complete, so nothing is ever unreachable-by-comment.

`Scope` then behaves exactly like `Msg`, `Ind` and `Timer` under composition: a parent that
**bridges** handles the child's scope end and emits nothing upward; a parent that **propagates**
re-wraps it into its own `Scope` type, the same way it re-wraps messages and timers. The
notation's *Bridges / Propagates* section and the code become the same statement again.

**The cost is real ceremony.** A fifth associated type on every protocol, a `type Scope =
Infallible;` line on every textbook rung, and a fourth mapper in every composition call — all to
serve a layer that does not exist yet.

**So: not now.** This is recorded as the intended mechanism, verified to compile, to be added
when the session-aware link is actually written. Adding it earlier would be building the
abstraction before its second consumer, which is the failure this project already documented.
The retrofit is contained: one associated type, and `Infallible` on each existing protocol.
