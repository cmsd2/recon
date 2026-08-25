## Context

See `proposal.md` — Why. Governing constraints are `docs/postmortem.md` §5 and `CLAUDE.md`; the
conventions already followed are listed there and are not restated.

Five rungs exist. Four are single-child stacks; the fifth, the failure detector, is a leaf that
composes nothing. This is the first layer whose structure is not a stack, and most of the design
below follows from that one fact.

Algorithm 3.4 is reproduced here because three of its features are unlike anything below it:

```text
upon event ⟨ urb, Broadcast | m ⟩ do
    pending := pending ∪ {(self, m)};
    trigger ⟨ beb, Broadcast | [DATA, self, m] ⟩;

upon event ⟨ beb, Deliver | p, [DATA, s, m] ⟩ do
    ack[m] := ack[m] ∪ {p};
    if (s, m) ∉ pending then
        pending := pending ∪ {(s, m)};
        trigger ⟨ beb, Broadcast | [DATA, s, m] ⟩;

upon event ⟨ P, Crash | p ⟩ do
    correct := correct \ {p};

function candeliver(m) is  return correct ⊆ ack[m];

upon exists (s, m) ∈ pending such that candeliver(m) ∧ m ∉ delivered do
    delivered := delivered ∪ {m};
    trigger ⟨ urb, Deliver | s, m ⟩;
```

## Goals / Non-Goals

**Goals:**

- Uniform agreement demonstrated in the case that distinguishes this rung: a process delivers, then
  crashes, and the survivors still deliver.
- A composition shape for two children that does not degrade into the string-keyed multiplexing
  the previous attempt used.
- Enough evidence to settle, or knowingly re-defer, the macro question.

**Non-Goals:**

- Algorithm 3.5, the majority-ack variant. It is a different failure model, not a refinement.
- Garbage collection of `ack` and `pending`. The book leaves it out and so does this; unbounded
  growth is acceptable in a simulator and is a real change later.
- The `Scope` associated type. See decision 5.

## Decisions

### 1. Two children, composed as owned fields, multiplexed by an enum on the wire

Both children send messages, so this layer's `Msg` must distinguish them:

```rust
enum Wire<P> { Broadcast(<Beb as Protocol>::Msg), Detector(Heartbeat) }
```

*Why an enum:* this is the first place the stack needs multiplexing at all, and it is worth being
explicit that this is the thing the previous attempt got wrong. It used `multiplex_key: String`
with `format!("{}/upb", key)`, so a typo produced a silently undelivered message. An enum makes a
mis-wiring a compile error, costs one tag byte, and — importantly — was not built until a layer
needed it.

*Alternative considered:* giving the detector its own transport, so no multiplexing is needed.
Rejected: it pushes the problem into the driver, where there is no type to check it with, and it
is exactly how a hand-rolled multiplexer starts.

### 2. The delivery condition is a function called after every state change

Algorithm 3.4's last clause is a predicate over state, not an event. It becomes a private method
called at the end of each handler that can change what it reads — `ack` growing on a beb-delivery,
`correct` shrinking on a crash indication.

*Why not evaluate it lazily or on a timer:* a timer would make delivery latency depend on a tick
rather than on the algorithm, and would be untestable against the book's guarantee of *eventual*
delivery. Calling it where the state changes is both faithful and cheap.

*Cost:* it is O(pending) per state change in the obvious implementation. Acceptable at this scale,
and noted rather than optimised.

### 3. Deduplicate on an identifier, not on content

As with the perfect link and reliable broadcast: `ack` and `delivered` are keyed by an identifier
carrying the originator and a per-sender sequence number, so identical content broadcast twice is
delivered twice. The book's `ack[m]` assumes messages are unique across senders.

### 4. `correct` starts as every process and only shrinks

Seeded from the membership at construction, reduced on each crash indication. A process never
returns to it, matching the detector's permanence.

*Consequence worth stating:* if the detector wrongly accuses a correct process, `candeliver`
becomes satisfiable too early and uniform agreement can break. That is the timing assumption
failing, and the specification says so.

### 5. The inherited timing assumption stays prose

URB's guarantees hold only while the detector is accurate, which holds only while delivery is
bounded. This is stated in the module documentation and the specification — as the book states it
by labelling algorithms fail-stop or fail-silent — and is deliberately *not* expressed with the
scope annotation of `docs/scope-annotated-modules.md`.

*Why not:* that document's Definition 2a permits a module to tag a property only with a scope whose
ends its own interface and state determine. URB cannot observe synchrony breaking; the failure
arrives as a detector mistake, indistinguishable from correct detection. An assumption a layer
depends on but cannot detect is not a scope, and tagging with one would produce an obligation no
implementation could discharge and no test could exercise.

This reverses an expectation recorded in the failure detector's notes, which named URB as the
second consumer that decision was waiting for. It is not: a genuine second consumer needs an
observable boundary.

### 6. Whether the two composition helpers justify a macro is decided by reading them

The reliable-broadcast notes concluded no macro, and named a layer owning several children as what
would reopen it. This is that layer. The decision is deferred to task 4.1 and made on the code
rather than in advance — but the prior is that a helper differs from its sibling in the part that
matters, and two helpers that differ only in which child they call are a stronger case for
generation than three handlers that differ in the same way were.

## Risks / Trade-offs

**A wrongly accused process silently weakens the guarantee** → and the resulting test failure would
look like a URB defect rather than an assumption violation. Mitigation: run the guarantee suites in
synchronous mode with the detector configured from the network's bound, and add one test that
withdraws the assumption and expects the guarantee to break, so the dependency is visible rather
than implicit.

**Delivery that never fires, and a suite that passes anyway** → every absence-of-violation property
here is satisfied by delivering nothing, and `candeliver` is exactly the sort of condition that can
be wrong in the never-true direction. Mitigation: the non-vacuity guard already established —
assert minimum delivery counts alongside every agreement assertion.

**`ack` and `pending` grow without bound** → acknowledged and out of scope, but it means long runs
consume memory in proportion to messages broadcast. Mitigation: none in this change; recorded so
that the first long-running scenario does not treat it as a surprise.

**Two children make the composition harder to follow than the algorithm** → which would be the
first time in this project the plumbing outweighed the protocol. Mitigation: task 4.1 measures it,
and the module quotes the pseudocode so the two can be read against each other.

## Open Questions

- **Whether the detector should be started by this layer or by the layer above.** It needs a
  `Start` command; whether URB issues it on construction or exposes it is a small interface choice
  that changes no requirement.
- **The shape of the identifier type.** Reliable broadcast has `BroadcastId { origin, seq }`;
  whether this layer reuses that type, defines its own, or the two are eventually unified is a
  tidying question with no bearing on behaviour.
