## Context

See `proposal.md` — Why. The governing constraints are in `docs/postmortem.md` §5 and `CLAUDE.md`.

Two facts shape every decision below. First, the tree is empty: there is no existing code to
accommodate, so the cost of these choices is entirely in what they make expensive *later*.
Second, every rung of the ladder that follows — failure detector, reliable broadcast, uniform
reliable broadcast, consensus — inherits the `Protocol` trait, the effect vocabulary, and the
wire convention established here. These are the decisions most expensive to revisit, which is
why they are settled before any code is written rather than discovered during it.

## Goals / Non-Goals

**Goals:**

- Protocol code that reads as the algorithm, close enough to the pseudocode in Cachin, Guerraoui
  & Rodrigues that the two can be compared line by line.
- Total reproducibility: a seed determines a run completely, so a failure is a number.
- Each protocol unit-testable as a plain function, with no runtime, no ports, and no async.
- A composition model that the compiler checks, so a mis-wired layer is a build error.

**Non-Goals:**

- Performance. The simulator should be fast enough to run thousands of schedules, but no
  optimisation is warranted before there is something to measure.
- Generality of the effect vocabulary. It covers what three protocols need; later rungs extend it.
- A stable public API. Everything here is expected to move until reliable broadcast lands.

## Decisions

### 1. The protocol core is synchronous and sans-IO

A `Protocol` is a state machine with handlers for commands from above, messages from peers, and
timer fires. It emits effects. It never awaits, never reads a clock, never draws randomness from
the ambient environment, never touches a socket.

*Why:* this is the single decision that makes the simulator possible. Time and randomness become
injectable, so runs are reproducible; handlers become plain functions, so they are testable
without a runtime.

It also has a consequence worth stating explicitly, because it is the strongest practical argument
for the pattern: **state transitions become atomic with respect to cancellation.** A handler is a
synchronous call that cannot be suspended or dropped part-way, so the torn-state failure mode
endemic to `select!` loops over async handlers cannot occur. The eventual tokio driver awaits in
exactly one place per iteration; everything after that await is synchronous to the end of the body.

*Alternatives considered:* `async fn` handlers — rejected; it welds protocols to a runtime, makes
determinism unobtainable, and reintroduces cancellation hazards. Prior art for the chosen approach:
`quinn-proto`, `rustls`, `raft-rs`.

### 2. Not actors

The topology below — single owner of mutable state, messages in, effects out, no shared locks — is
actor-shaped, and deliberately so. The algorithm does not live inside the actor.

*Why not an actor library:* an actor has one mailbox, so its message type must be the sum of
everything it can receive, collapsing four genuinely distinct directions (command down, indication
up, wire in, timer fire) into one type. The compiler can then no longer tell you that a child's
indication came from your child. `ActorRef<M>` erases which instance you hold; replies need
correlation machinery; `Send + 'static` propagates everywhere; and delivery can fail at runtime
where a direct method call cannot. That last property is `multiplex_key: String` in better
clothing — a link resolved at runtime that the compiler could have resolved at build time.

More fundamentally: **Rust's type system is already a component model.** Ownership is supervision,
fields are wiring, method calls are triggers, enums are ports. An actor framework supplies those
things for runtimes that cannot express ownership, and running it alongside Rust's own means
maintaining two structures that must agree.

*Note:* Kompics — the framework this project is modelled on — uses typed directional *ports*, not
mailboxes, and ports map onto associated types and owned fields almost one-to-one. Actors would
have been a step away from what made Kompics readable, not toward it.

### 3. Composition is a static tree of concrete fields

A parent owns its children as typed fields and re-wraps their effects. No registries, no string
keys, no dynamic lookup.

**Static types, not fixed cardinality.** A layer may own many instances of a child in a map —
multi-decree consensus needs one instance per slot, a failure detector one timer per peer. The
tree is static in type and dynamic in count; the compiler still checks every edge. Ordered maps,
not hash maps — see Risks.

### 4. Effects are written through a sink; the core has no allocation policy

Handlers take `&mut Cx<Self>` and call `cx.send`, `cx.indicate`, `cx.set_timer`. They do not
return `Vec<Effect>`. `Cx` does not own a buffer: it writes through an `EffectSink` trait, and the
driver chooses what backs it.

*Why not return a vector:* it allocates per event, and per event *per layer* once composed. The
simulator runs millions of events.

*Why a sink rather than a `&mut Vec`:* an earlier iteration of this design had `Cx` hold
`&mut Vec<Effect<..>>`. Composing then required each parent to own a scratch buffer as a struct
field, take it, hand it down, drain it, and put it back — roughly twelve lines per handler for a
protocol doing nothing, and a wart on every protocol struct. Writing through a sink lets a parent
install a mapping adapter that translates each child effect *as it is emitted*, so no intermediate
buffer exists at all. Composition becomes a single call, and allocates less than the buffered
version it replaces.

*Why this matters beyond ergonomics:* it moves the allocation decision out of the core entirely.
An ordinary driver passes a `Vec`, reused across events, giving amortised allocation — which is
the right default for almost every caller. A `no_std` or latency-sensitive driver passes a
fixed-capacity sink. A test passes a counting sink. Protocol code is identical in all three cases,
because a protocol only ever calls `cx.send(..)`. The core should not have an opinion here, and
with a sink it does not need one.

*Costs:* one virtual call per effect per level of nesting — negligible against the work the
simulator does per delivery, but real. Mappers are `fn` pointers so enum variant constructors
coerce directly; a mapper needing captured state would require generics instead. `Cx` internals
are more intricate than a plain buffer, which is complexity moved from every protocol into one
place.

*What it gives up:* handlers no longer look like pure functions, which is genuinely nicer to test.
That is recovered with a `step` helper that runs one event against a throwaway sink and returns
the effects, restoring `assert_eq!(step(&mut p, ev, ..), [..])` ergonomics without imposing it on
production paths.

### 5. Time is a newtype over `std::time::Duration`

`Time` is a point in a run, represented as an offset from its start. It is not
`std::time::Instant` and not any runtime's instant type: neither can be constructed at an
arbitrary value, which is what a replayable run requires.

*Why `Duration` and not a raw integer:* the constraint was only ever on `Instant`. `Duration` is
a pure value type with no clock behind it, so it satisfies arbitrary construction completely. It
also brings tested saturating arithmetic and the full constructor family, instead of hand-rolled
nanosecond conversions. And its range is `(u64 secs, u32 nanos)` rather than a `u64` of
nanoseconds, which lifts the ceiling from roughly 584 years to 584 billion — a bound that stops
being worth thinking about. The cost is 16 bytes per `Time` instead of 8, which shows up in the
simulator's queue key and is negligible there.

*Why still a newtype:* a `Time` is a point and a `Duration` is a span. `Time + Duration -> Time`
and `Time - Time -> Duration` are the only meaningful combinations, and the wrapper is what stops
one being passed where the other belongs.

*Why not a datetime library:* `chrono`, `time` and `jiff` all model civil and zoned time — wall
clocks, calendars, time zones. A protocol has no use for any of that; it needs elapsed time and
nothing else. A datetime type in the core would be an unnecessary dependency and an invitation to
reason about wall-clock semantics that the simulator cannot reproduce. If human-readable
timestamps are ever wanted for trace output or logging, that belongs at the edge, converting from
a run's start instant.

*Why now:* this type is in every handler signature that touches time. Retrofitting it is a
mechanical but wide change.

### 6. Timers route exactly like messages

A timer token set by a child comes back through the parent's timer enum, re-wrapped the same way
messages are. If timers ever need a different routing mechanism than messages, that is evidence
the composition model is wrong.

### 7. Nested typed values on the wire, encoded once at the bottom

Layers pass typed values down. Only the bottom boundary serialises, exactly once. No intermediate
representation is ever materialised.

*Why this is safe:* the previous attempt's "encoded three times" problem is routinely
misdiagnosed as a cost of nesting. It was not. It came from `serde_json::to_value` building an
intermediate `Value` tree at every layer boundary. Nested types with a single binary encode at the
wire produce the same bytes as hand-rolled header prefixing, in one pass, with no intermediate
allocation. The rule to enforce is *never materialise an intermediate*, not *keep the wire flat*.

*How deep this actually goes:* shallower than the composition tree, because layers that add no
per-hop state add no wire fields. Of the three protocols here, stubborn link adds nothing (it
retransmits identical bytes) and best-effort broadcast adds nothing (it is pure fan-out). Only
perfect link contributes a header — one message identifier. **Three protocols, one wire header.**
Depth accumulates only from reliable broadcast upward.

*Alternatives considered:* a flat per-node wire enum with layers matching their own variants —
rejected, it turns typed routing back into a runtime match. Each layer encoding its payload to
bytes — rejected, that is precisely the failure being avoided.

### 8. Layers are generic over their payload

`PerfectLink<P>`, not `PerfectLink` hardcoded to the broadcast message type.

*Why:* not speculative reuse, which would be the framework-first instinct that killed the first
attempt. The reason is testability: `PerfectLink<TestMsg>` can be unit-tested in isolation without
any layer above it existing. One type parameter, and isolated testability is the project's whole
point. Type aliases keep the depth out of everyday signatures.

### 9. Fair-loss links are the simulator, not a protocol

The bottom rung of the ladder is the network model. The simulator *provides* fair-loss semantics —
messages may be dropped, duplicated, reordered, delayed — and the first protocol actually
implemented is the stubborn link that retransmits over it.

### 10. The simulator moves typed values, with an opt-in codec check

Deliveries carry typed values rather than encoded bytes, so runs are fast and codec problems
cannot masquerade as protocol problems. A `--check-codec` mode round-trips every delivery through
the wire codec, so serialisation bugs remain findable on demand without being paid for in every run.

### 11. Errors are per-layer `thiserror` types

No `io::Error` for domain failures, no stringly-typed causes. The previous attempt flattened seven
distinct failures into `io::Error::new(ErrorKind::Other, "json decoding error")`.

## Risks / Trade-offs

**Nondeterminism leaking in through `HashMap` iteration order** → Rust's default hasher is seeded
randomly per process, so iterating a `HashMap` to pick gossip targets or fan out a broadcast makes
runs unreproducible even with a seeded RNG. This defeats the entire premise and fails silently.
Mitigation: `BTreeMap`/`BTreeSet` throughout protocol and simulator state, with a lint or review
rule forbidding `HashMap` in those crates.

**Property assertions that pass vacuously** → a suite asserting "no message delivered twice" is
satisfied trivially if the simulator never delivers anything, or never actually injects the faults
it claims to. Mitigation: assert positively on the fault injection itself (a run configured for 30%
loss must show losses in its trace) and on delivery counts, not only on absence of violations.

**Sink indirection obscuring where effects end up** → with mapping adapters stacked one per
layer, an effect emitted deep in the stack passes through several translations before reaching the
driver's buffer, and a wrong mapper is a silently mis-routed message rather than a compile error
within the adapter chain. Mitigation: mappers are enum variant constructors, so a wrong one is
usually a type error; composition tests assert the re-wrapped shape at each boundary rather than
only at the top.

**Deep generic nesting degrading compile times and error messages** → real but not yet; with one
wire header across three protocols there is nothing to feel. Revisit at reliable broadcast, where
depth starts accumulating.

**Scope creep toward transport** → the documented failure mode, four times over. Mitigation: the
proposal names sockets, tokio and quinn as out of scope; treat any task that reaches for them as a
signal the change is wrong rather than the constraint.

**Over-abstracting before three protocols exist** → also documented history. Mitigation: no macro,
no DSL, no trait beyond what these three need. Boilerplate is left visible on purpose so it can be
measured before anything is built to remove it.

## Open Questions

- **Binary codec choice** (`bincode` vs `postcard` vs another). Affects only the wire boundary and
  the opt-in codec check; no spec or task depends on which is chosen.
- **Crate layout** — one crate versus separate core and simulator crates. Deferrable until the
  dependency direction is visible in practice; it does not change any requirement.
- **Whether the scoped child view of decision 4 is needed at all.** Requires a profile that cannot
  exist yet.
