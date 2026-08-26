## Context

See `proposal.md` — Why. Two pieces of existing groundwork matter.

`docs/conditional-guarantees.md` already defines the **incarnation** scope and the lattice
`session ⊑ incarnation ⊑ always`, and already names stable storage as the redundancy that bridges
a restart. This change implements an entry that document has been carrying unimplemented.

`Sim::crash` already rebuilds a process from its constructor, so amnesia is real and testable
today. What is missing is only the other half — somewhere for anything to survive.

## Goals / Non-Goals

**Goals:**

- A durable-storage primitive that a protocol reaches only through effects, so constraint 2 holds
  as written.
- Two independent consumers, so the primitive is shaped by real use rather than by anticipation.
- The fail-recovery indication shape landed at the bottom of the stack, where it is visible.

**Non-Goals:**

- Collecting any durable set. Transcriptions, labelled, as everything above the failure detector
  already is.
- A bounded delivered-cursor. It is the obvious follow-up and bounding changes the guarantee to a
  scope, which is a change with a proposal of its own.
- Storage that can be lost or corrupted independently of a crash. Disk failure is a different fault
  class and modelling it now would buy nothing these algorithms can act on.
- Ω, epoch consensus, or anything leader-driven. This change exists to make them possible.

## Decisions

**Storage is an effect, not a call on the context.** `Cx` supplies time and randomness, which are
*reads*. A durable write is not a read, it takes milliseconds, and a blocking `cx.store()` would
reintroduce exactly what constraint 2 exists to prevent — a protocol that waits. So `Effect::Store`
joins send, indicate and set-timer, and the driver performs the write.

*Alternative considered:* the driver inspects the protocol's state after each event and persists
what changed. Rejected — it makes durability invisible at the call site, so a reader cannot tell
from the algorithm where the write happens, and the book's algorithms place `store` explicitly
because the placement is the correctness argument.

**The ordering rule is a driver obligation, not an awaited event.** A protocol may emit a store and
then a send from the same event and rely on the write being durable first. The alternative — a
`Stored` completion event the protocol waits for — is more honest about latency but splits every
algorithm across two events and makes the book's pseudocode unreadable against the implementation.
Raft implementations take the batch-ordering approach for the same reason.

The cost is that the rule is a *promise the driver makes*, not something the type system enforces,
so it needs a test rather than a compiler. `simulation`'s delta specifies the observable form:
a crash between a store and a send leaves the write durable and the message unsent.

**Durable state is a declared associated type.** `type Durable` on `Protocol`, with
`Infallible`-style impossibility for protocols that keep nothing — the same trick `type Scope`
already uses, so that a store effect cannot be constructed for a protocol that has no durable
state. What survives a crash is then legible from the interface rather than from a convention
about which fields get written.

*Alternative considered:* an untyped key-value store. Rejected — this project's whole position on
layer boundaries is that they are typed, and "which keys does this protocol own" is exactly the
string-keyed composition the post-mortem records as a failure.

**A parent may compose only a child that stores nothing, and this is enforced.** Found while
implementing, not while designing. The other effects re-wrap with a variant constructor —
`fn(CM) -> PM` — because a child's message *becomes* one of the parent's messages. Durable state
does not work that way: a parent's durable state contains its own fields **and** its child's, and
a `fn(CD) -> PD` has no access to the parent's, so there is nothing correct for it to return.

`with_child` therefore takes a durable mapper alongside the other three, and for a child that
stores nothing the only function that can be written is `absurd` — the total function out of
`Infallible`. A storing child is then not silently broken; it fails to compile, because no mapper
exists to pass.

All three protocols here have children that store nothing: Algorithm 2.3 is over the stubborn
link, and Algorithm 3.8 over stubborn broadcast, neither of which writes anything. So the
restriction costs nothing now, and the real design — whatever it is: a slot per participant, a
parent that drives its child's writes explicitly, a path-indexed store — is left to the first rung
that actually needs it. That is constraint 4 applied to storage: two or three by hand first.

**Recovery is an event, following the book.** Algorithm 2.3 has `⟨ Recovery ⟩` distinct from
`⟨ Init ⟩`, and both Algorithms 2.3 and 3.8 *do things* on recovering — re-indicating the retrieved
log, re-broadcasting what is pending. Those are effects, so recovery must be an event that can emit
them, not a constructor.

This overturns the reading in `docs/conditional-guarantees.md` that an incarnation boundary needs
no event because "there is nobody to notify". That is true of the incarnation *ending*, which the
ending process cannot observe. The *beginning* is observable by the process itself, and it is the
beginning that carries the obligations.

**The simulator models an interrupted write as all-or-nothing, chosen by the seed.** A crash while a
write is outstanding leaves either the whole new value or the whole previous one, decided by the
seeded source, with no way for the recovering process to tell which. Torn values within a single
write are not modelled: real storage stacks go to some lengths to prevent them, and an algorithm
that has to defend against them is defending against a different fault class. What must be modelled
is the *uncertainty*, because every one of these algorithms has a window where it does not know
whether its last write landed.

**Logged uniform reliable broadcast uses stubborn broadcast, not the logged link.** This is worth
stating because it is surprising. The book's logged rungs do not stack: Algorithm 2.3 is over
stubborn links, Algorithm 3.7 is over stubborn links, Algorithm 3.8 is over stubborn *broadcast*.
Each keeps its own log.

The reason is that a perfect link's deduplication record is volatile, so after a restart it would
re-deliver anyway — the dedup buys nothing a logged layer above does not already do for itself, and
the *retransmission* is what a recovered process needs. So stubborn broadcast is required, and is
added here.

The consequence for this change's scope claim: the second consumer proves that one primitive serves
two independent users. It does **not** prove that the new indication shape composes upward, because
nothing composes on it. That is a weaker claim than it might look, and it is the honest one.

**`ack` is deliberately not durable, and the specification says why.** Algorithm 3.8 stores
`pending` and `delivered` and rebuilds `ack` by re-broadcasting on recovery. Writing `ack` too
would cost a write per acknowledgement, to save work that retransmission does anyway, and would
make the durable state grow with traffic rather than with messages. Getting this wrong in the
direction of storing more looks safer and is worse.

## Risks / Trade-offs

- **The ordering rule is a promise, not a type.** A driver could perform effects out of order and
  every test would still pass unless one asserts it. → The simulation delta specifies the
  observable form, and the suite asserts it by crashing between the two.
- **The crash-during-write fault produces flaky-looking tests**, since the same seed range yields
  both outcomes. → Tests that exercise it assert over a *set* of seeds — that both outcomes occur,
  and that the algorithm's guarantees hold in each — rather than fixing a seed and asserting one.
- **The new effect variant breaks every exhaustive match** in the simulator and in tests. → It is a
  compile error everywhere it matters, which is the point of an enum; the work is mechanical and
  bounded.
- **A protocol emits a store on every change**, making the write rate proportional to traffic. Real
  for these transcriptions, since they store whole sets. → Stated in each module and in the specs;
  the fix is the bounded-cursor follow-up, not a change of primitive.
- **"It survived a crash" is asserted from protocol state rather than the trace**, which a refactor
  could falsify. → Storage activity appears in the trace, and durability properties are asserted
  from there.
- **Three protocols in one change is a lot**, and a mistake in the primitive is found three times
  over. → The primitive lands first with a test protocol in the core suite, before any of the three
  is written.
