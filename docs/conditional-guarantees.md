# Conditional guarantees

A design note, not a specification. The formal argument — what the scope annotation means, why
every proof in Cachin, Guerraoui & Rodrigues survives it untouched, and what it does and does not
add — is in [`scope-annotated-modules.md`](scope-annotated-modules.md). It records a reframing that should shape the abstractions above
best-effort broadcast, and the eventual transport layer. Nothing here is implemented yet, and
some of it deliberately should not be until it has a second consumer.

## The problem with the abstractions as written

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
| stable storage | process restart | a crash — the book's *logged* variants, now implemented |
| other processes | this process dying | a sender crash — reliable broadcast's whole purpose |

That gives a second reading of the whole sequence. Each abstraction is not merely "stronger"; it bridges one
more class of failure than the abstraction below:

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

Read that way, best-effort broadcast is not a weak version of reliable broadcast. It is the abstraction
that bridges nothing new, which is precisely why the next abstraction exists.

**A layer that cannot bridge must propagate.** Silently absorbing a scope end is the bug. If a
perfect link cannot tell whether its peer merely reconnected or restarted with an empty
`delivered` set, it must pass that uncertainty upward rather than resolve it by assumption.

## Consequences for the port

The link port gains exactly one event:

- **`SessionChanged { peer, epoch }`** — the transport session ended and a new one began.
  Anything in flight may have been lost. Bridgeable by resending what was unacknowledged.

  *As built, this is two events rather than one:* `SessionEvent::Ended { peer, epoch }` names the
  epoch that ended, and `SessionEvent::Established { peer, epoch }` the one now in force. One
  event conflates them, and the conflation costs the layer above the only thing it can act on.
  At the moment a session ends the next epoch is not a fact and may never become one — the peer
  may be gone for ever — so an ending cannot carry it. And the ending is not when to resend:
  anything sent then goes nowhere. Every resend clause in this repository fires on
  `Established`, and nothing fires on `Ended`.

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
*my* restart that lets me deliver again.

That boundary needs no event **at its end**, because the process that would raise one is the one
that ceased to exist. Its *beginning* is a different matter, and the distinction is easy to lose.
An incarnation begins at the process's own `⟨Init⟩` or `⟨Recovery⟩` — exactly one of them, as the
book has it — and both are observable by the process itself and can produce effects. That is where
the work goes: retrieving what survived, re-announcing it, writing down what a first start must
remember. A constructor cannot serve, because it runs in both cases and emits nothing.

The session events are **additive**, and stayed so: the textbook implementation over the
simulator never emits them — its `Scope` is uninhabited, so it cannot — and every existing proof
and test stays valid, while every layer written since is forced to decide what it does about
them.

## What the simulator can and cannot express

`Sim::crash` rebuilds the protocol from its constructor, so a crash genuinely loses volatile state
and `crash` then `restart` is amnesia. `Sim::suspend` with `Sim::resume` is the pause, and it is a
*stall*: everything that came due while the process was away — timers, deliveries carried by a
session that stayed up, scope events — is held and dispatched on resume, and no startup branch
re-runs over state that was never lost. A restarted process therefore does face what it actually
faces: having forgotten what it delivered. A resumed one faces what *it* actually faces: having
missed nothing except the passage of time.

That asymmetry is the sim obeying its own rule. Dropping an in-session delivery to a suspended
process would lose a message with no `SessionEnded` to account for it, which is the thing this
document forbids of every layer; holding it is the only alternative that does not require ending
the session. But the clock is not held, so a process stalled past a timeout comes back with
measurements it made honestly and cannot trust — see
`crates/recon-protocols/src/perfect_failure_detector.rs`, which accuses every peer in exactly that
case and says why that is the synchrony assumption failing rather than a bug.

What is still missing is the other half — **nothing survives a crash**, because there is no stable
storage. An abstraction that must remember an epoch, a promise or a decision across an incarnation has
nowhere to put it, so the logged variants of these abstractions cannot be written at all. That is
the gap to close before anything in the fail-recovery model.

## What not to build yet

Nothing here justifies restructuring the three protocols that exist.

- **Do not make layers generic over their child yet.** There is no second link implementation to
  be generic over, and building the abstraction before its second consumer is the failure this
  project already documented. The retrofit is contained: best-effort broadcast owns one field and
  makes three four-line composition calls.
- **Do not weaken the perfect link's guarantee yet.** The unbounded version is what the book's
  proofs assume. The bounded, session-aware one is a *sibling* implementation of the same port,
  and it belongs with the transport work, which comes last.

What is worth doing early, because the simulator can already produce the fault:

1. Model scope-end events in `recon-sim`, driven by crash/restart, so they can be tested before
   any transport exists.
2. ~~Make crash actually lose state, with an opt-in for the current suspend-and-resume
   behaviour.~~ Done: `crash` rebuilds from the constructor, `suspend` preserves.
3. ~~Write the boundary down: layers above the link may depend on its `Cmd` and `Ind` types and
   nothing else. That is the seam a second implementation will be swapped through.~~ Done, and it
   turned out to need building rather than writing down.

   `crates/recon-protocols/src/link.rs` holds the port. A link keeps its own `Cmd` and `Ind` —
   pinning them to one pair admits the perfect link and excludes the session link, whose `Ind` has
   three variants — and supplies two translations instead: build a send, and say what an indication
   means. Every layer above bounds on `Link` and names no implementation, so the same broadcast
   composes over the perfect link, the session link, or an application's own driver.

   The sentence above was true and inert for as long as it was documentation. Needing a different
   link once produced four forked broadcast modules, around 2,000 lines whose algorithms were the
   originals with the link swapped underneath; the 2026-08 audit found a quoted clause gone stale
   in one fork and not its sibling. Those four are gone, and what replaced them is a type parameter.

   The half of this the type system does enforce is which layers may depend on a boundary.
   `ScopedLink` is the claim that a link can observe one; the session link makes it and the perfect
   link does not, because PL2 is scoped to the recipient's incarnation and the link cannot see that
   incarnation end. A layer that repairs a scope ending bounds on `ScopedLink`, so composing it over
   a link that reports none is a compile error rather than a stack that waits for ever. That is *a
   layer that cannot bridge must propagate*, checked rather than asked for.

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
                                                 (as built: Ended | q, e — which ends it —
                                                  and Established | q, e′, which begins the
                                                  successor and is where a bridge acts)

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
    nothing      the incarnation *ends* with nobody left to notify, and the peer's
                 incarnation is not observable from here at all. It *begins* at this
                 process's own ⟨Init⟩ or ⟨Recovery⟩, which is where the work goes.
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

**It makes the structure of the sequence explicit.** Reading the abstractions by their tags shows where each
one strengthens a scope: reliable broadcast takes best-effort broadcast's validity from
`[sender's incarnation]` to `[always]`, and it does so by moving the redundancy from the sender's
memory onto other processes. That is the same table as the previous section, arrived at from the
notation rather than from the implementation.

### The cost, honestly

Every module definition grows by six or eight lines, and most of the early abstractions will be tagged
`[always]` or `[session]` with nothing interesting to say. The notation earns its keep from
reliable broadcast upward, where bridging is the entire content of each abstraction, and it is largely
ceremony below that.

It is also a private extension. Anyone reading this code against the book will meet a notation
the book does not use, so the mapping has to be stated once — here — and referenced rather than
re-explained per module.

## Keeping the book versions and the real versions side by side

The obvious worry about adding a session event is that it infects everything. It would, if it
were a variant of the link's `Ind`: every layer above would have to match a case the textbook
stack can never produce, and the algorithms would fill with dead branches — the opposite of code
that reads like the page.

It does not infect them if scope events are their **own associated type** rather than an
indication.

```rust
trait Protocol {
    type Cmd;
    type Ind;
    type Msg;
    type Timer;
    type Scope;                                   // the notation's `Scopes:` section

    fn on_scope_event(&mut self, _s: Self::Scope, _cx: &mut ProtoCx<'_, Self>) {}
    // ...and the driver's `Event::ScopeEvent(s)` is what dispatches to it.
}
```

The book version declares that it has no scope conditions, and writes no handler:

```rust
impl Protocol for PerfectLink<P> {
    type Scope = core::convert::Infallible;       // every property [always]
    // no on_scope_event — nothing to write, and no branch to read past
}
```

The session-aware version declares what it is bounded by, and handles it:

```rust
impl Protocol for SessionLink<P> {
    type Scope = SessionEvent;                    // Ended and Established, and nothing more
    fn on_scope_event(&mut self, s: SessionEvent, cx: &mut ProtoCx<'_, Self>) { … }
}
```

Two properties make this work, and both were checked rather than assumed:

- **A scope event cannot be constructed for the book version.** `Infallible` is uninhabited, so
  there is no value to pass. A driver generic over `P` cannot inject one; the impossibility is a
  type error, not a convention.
- **An exhaustive match on an uninhabited type needs zero arms.** Where a handler is written
  generically, `match s {}` is complete, so nothing is ever unreachable-by-comment.

**This is built, and one thing about it did not turn out as sketched above.** `Scope` does *not*
behave like `Msg`, `Ind` and `Timer` under composition, and `with_child` takes no fourth mapper.

Messages, indications and timers are re-wrapped at each boundary because each layer has its own
vocabulary for them. A scope has no such vocabulary: it is a fact about the transport underneath
the whole stack, and every layer that cares about it cares about the *same* fact. So the concrete
`SessionEvent` — the bottom layer's type — is what the driver hands to the top, and each parent
routes it down unchanged to the child that owns the link:

```rust
fn on_scope_event(&mut self, event: SessionEvent, cx: &mut ProtoCx<'_, Self>) {
    // Routed down so the link can record it and report it back up. This layer takes no other
    // action: it has nothing to resend.
    self.with_beb(cx, |beb, ccx| beb.on_scope_event(event, ccx));
}
```

A mapper would have bought a renaming of something nobody renames. What the *Bridges /
Propagates* distinction turns into is therefore not two ways of re-wrapping but two ways of
answering: a layer that **bridges** handles the event, resends what it must, and emits nothing
upward; a layer that **propagates** raises an indication of its own — `Ind::SessionEnded`,
`Ind::SessionEstablished` — so the layer above it can act. Every session-aware module here does
both: it routes the event down, and it reports upward, because a layer that cannot bridge must
propagate and the ones that *can* bridge still owe the report.

**The handler is `on_scope_event`, not `on_scope_end`, and the name was corrected once the code
existed.** A scope *beginning* travels the same path: `SessionEvent::Established` is what a
resend clause fires on, and it is the only event on which a resend can succeed. Definition 2 is
written in terms of endings because an ending is what threatens a guarantee, but the port has to
carry both — naming a loss with no event on which to repair it would be naming a problem and
withholding the answer. See `session_uniform_reliable_broadcast`, whose added clause is triggered
by an establishment and by nothing else.

**The cost was the ceremony predicted.** A fifth associated type on every protocol and a
`type Scope = Infallible;` line on every textbook abstraction — but no fourth mapper, and no dead
branch anywhere: the transcriptions are uninhabited and write no handler at all.
