## Context

The gossip pair is the real-world set, and the second standard (`CLAUDE.md`, "The real-world set")
is: over session links, and checked for resource use. Both modules are already parameterised over a
link satisfying the port and already propagate a boundary upward as their own indication. What is
missing is narrower than it looks: one hard-wired child, one identity that does not survive its
minter, and a class of test nobody has written.

## Goals / Non-Goals

Goals: the two `OverSessions` stacks compile, run, and carry suites that assert message counts
against the algorithm, quiescence, boundary propagation, recovery across a session ending, and
identity across an originator's restart. Non-goals: changing the eager algorithm's redundancy
(see open questions); durable state in gossip; touching the fair-loss suites beyond what the
identity change requires.

## Decisions

### The originator's incarnation is part of the identifier

`BroadcastId { origin, incarnation: u64, seq }`, with `incarnation` drawn from `cx.rng()` in
`on_init`. Distinct across restarts with overwhelming probability, needs no storage, and is decided
by the one process that knows it restarted. `delivered` and `order` stay keyed per originator; the
window is unchanged.

*Alternative — clear a peer's window on `SessionEstablished { peer }`.* Wrong twice over: the
peer whose session came back is a relayer, not necessarily the originator whose identity is in
question; and the link cannot say whether the peer restarted or merely reconnected, so clearing
would re-deliver across every reconnect. `docs/conditional-guarantees.md` already argues that
incarnation identity must come from the level that owns it, and this is that argument applied.

*Alternative — a durable incarnation counter.* Correct and unnecessary: gossip keeps nothing
durable, and adding a store for one counter would make it the first `Meta` in the crate's most
lightweight module. Random suffices for a `u64`.

### The lazy layer's sender is the originator in one incarnation — found during apply

The proposal fixed identity at the eager layer and missed that the lazy layer has the same hole
one level up: `Data { origin, seq }` and `next[s]` are per originator, and `sn < next[s]` is
silently ignored, so a restarted lazy originator's first messages are dropped everywhere as already
delivered — and, once the eager layer stops deduplicating them, dropped by *this* layer instead.
The real-world workhorse had the bug the change was named for.

`Data` gains `incarnation`; a `Sender { origin, incarnation }` keys `next`, `pending`, `stored`,
their orders and the gap timers; requests carry the incarnation. The layer draws its own incarnation
at `Init` exactly as the eager layer does.

*What bounds it.* A receiver keeps state for the **two** most recent incarnations of each
originator and retires the oldest on hearing a third. One would flip between an incarnation being
retired and the one replacing it while relayed copies of both are still arriving, losing both; more
than two would keep state for processes that cannot exist. `2 × membership × window`, and a restart
costs a purge rather than a leak.

*Alternative — a durable incarnation counter.* Rejected as for the eager layer: nothing here is
durable, and a `u64` from the seeded generator collides with probability `2⁻⁶⁴`.

### Two session links under the lazy module, on one wire

`LazyProbabilisticBroadcast<P, L, G = FairLossLink<pb::Carried<Data<P>>>>` — the gossip child's
link becomes a parameter with the book's default. Over sessions both `L` and `G` are `SessionLink`,
two instances each holding one epoch per peer, both given every scope event. That mirrors how the
simulator drives sessions today — one session per process pair, events raised to the top and routed
down — and how `uniform_reliable_broadcast` already puts a broadcast and a detector on one wire.

*Alternative — one link carrying an enum of both halves.* Requires the gossip child to send through
a link it does not own, which is the string-keyed composition the postmortem forbids in another
form.

### Message counts are asserted as identities, not as ceilings

Algorithm 3.9 sends `k` messages per receipt with `r > 1`, plus `k` for the originator. So over any
run, `sends == k × (1 + receipts_with_ttl_above_one)` exactly, and within sessions — where nothing
is lost — `sends == Σ_{i=1..R} kⁱ` per broadcast exactly. An identity catches a stray retransmission
or a duplicated relay where a ceiling would absorb it. For the lazy module: `requests ≤ gaps
detected`, `answers ≤ requests received by a process that had stored the message`, and `sends == 0`
over any window after the run is quiet.

### The eager suite stays probabilistic over sessions

Within a session nothing is lost, but `picktargets` is still random, so a fanout that cannot cover
the membership still leaves processes out. `PB1` stays `[probabilistic]` over sessions; what
changes is that the only source of loss is a session ending, which the sim can inject and count.

## Risks / Trade-offs

- **Two `SessionLink`s means two copies of the epoch map.** Bounded by membership, twice. Accepted;
  the alternative is a shared-link mechanism nothing else needs.
- **A random incarnation can collide.** With probability 2⁻⁶⁴ per pair of incarnations. Stated in
  the module as the residual, and as the reason a deployment with stable storage might prefer a
  counter.
- **The identity change touches the wire of the fair-loss suites too.** Their assertions are on
  deliveries and counts, not on identifier values; `the_wire_survives_encoding` is the one that
  looks at the shape and it is updated with the shape.

## Migration Plan

1. `BroadcastId` gains `incarnation`; fair-loss suites still pass; one new test in the eager suite
   that a restarted originator's broadcasts are delivered.
2. The lazy module's gossip child becomes a parameter; `Gossiper<P, G>`; defaults preserve today's
   types.
3. `stacks.rs` aliases.
4. The eager over-sessions suite. 5. The lazy over-sessions suite. 6. Docs.

## Open Questions

- **Relay once?** Real-world gossip implementations commonly relay only on first receipt, cutting
  the eager algorithm's `Σ kⁱ` to roughly `k × N`. The book's reading is deliberately not that, and
  `probabilistic_broadcast`'s documentation defends the redundancy as the mechanism behind `PB1`.
  This change does not add a `relay_once` knob; the lazy module is the efficient one and is the
  real-world workhorse. Left open for the multi-Paxos work, where gossip's role becomes concrete.
