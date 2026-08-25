# What the session model cost

Tasks 4.1 and 4.2.

## The scope mechanism cost two lines per unaffected protocol

Six existing protocols gained `type Scope = core::convert::Infallible;` and a doc line. Nothing
else: **twelve insertions, zero deletions, zero behaviour**. All 155 tests passed unchanged the
moment the declarations were in.

It was as cheap in practice as it looked on paper, and one thing was cheaper. The design proposed
in `docs/scope-annotated-modules.md` anticipated needing a fourth mapper in composition, alongside
those for messages, indications and timers. **It did not.**

The reason is a direction that was not obvious until it was implemented. Effects travel *up*:
a child emits, a parent re-wraps, hence the mappers. A scope ending travels *down*: it originates
outside the stack and each layer routes it to whichever child cares — exactly like `on_msg` and
`on_timer`. So a parent handles one by matching its own scope enum and calling the child, using
the composition helper it already has:

```rust
fn on_scope_end(&mut self, MyScope::Link(s): MyScope, cx: &mut ProtoCx<'_, Self>) {
    let link = &mut self.link;
    cx.with_child_consuming(.., .., &mut inbox, |ccx| link.on_scope_end(s, ccx));
}
```

What travels back up is an *indication* — a layer that cannot restore its guarantee says so in its
own terms. That is what `bridges` and `propagates` mean in the notation, and both are covered by
tests: a parent that repairs the lapse raises nothing, a parent that cannot reports one of its own.

## Per-pair ordering cost the delivery queue less than feared

The risk register put this first: the queue is the source of determinism, and a subtle change
there breaks reproducibility everywhere and silently.

It did not need a new queue. Delivery is still keyed by `(time, sequence)`; the session path
simply computes a delivery time that cannot precede the last one for the same ordered pair:

```rust
let at = match self.last_delivery.get(&(from, to)) {
    Some(prev) if *prev >= earliest => *prev + Duration::from_nanos(1),
    _ => earliest,
};
```

Monotonic per pair, so FIFO follows from the ordering the queue already had. Latency still varies;
a message simply cannot overtake one sent earlier the same way.

**Determinism needed nothing beyond the existing seeding.** Every draw — latency, and the cut
point for a lost suffix — comes from the run's generator in a fixed order. `session_runs_are_
deterministic` asserts byte-identical traces across eight seeds while also asserting that differing
seeds differ, and the pre-existing determinism tests pass untouched on the fair-loss default.

## One design error caught, and it was a layering one

`SessionEnded` was first defined in `recon-sim`, with the session link converting from it. That
made `recon-protocols` depend on the simulator — a protocol crate depending on its test harness,
which is the direction this project exists to keep straight.

It is a domain concept, not a simulator one: any driver reports the same thing, because it is what
a real endpoint learns. Moved to `recon-core`, the conversion disappeared entirely — the link's
`Scope` *is* the core type — and the dependency went with it. Worth a mechanical check of its own
if another crate is ever tempted the same way.

## The session link is the first protocol that satisfies the space rule

One `BTreeMap<NodeId, u64>` and nothing else. `state_does_not_grow_with_messages` sends and
receives a thousand messages and asserts the footprint is still zero, then confirms that two scope
endings produce exactly two entries.

Set against the perfect link, which obtains the same guarantees by retransmitting for ever and
remembering every identifier it has seen, this is the concrete demonstration that
`docs/bounded-space.md` claimed: **the deployable link needs less state than the academic one, not
more.** It has no sequence number on the wire, no retransmission buffer and no deduplication set,
because within a session the transport already does that work.

## What this did not do

The six existing rungs still run on fair-loss, unchanged and passing. Moving them onto the session
link is a separate change, and a real one: each layer must decide what a session ending means for
*its* guarantee — bridge it by resending, or propagate it upward — which is the question
`docs/conditional-guarantees.md` frames and which no amount of plumbing decides.
