## Why

Everything built here decides one value once, or delivers one message. Nothing has a **concurrent
interface** — an object many clients act on at the same time, whose operations overlap and read each
other's writes. That absence is what the evidence track has run into: item `G`, a concurrent
workload, has nothing to issue overlapping operations against, and item `H` says outright that "a
checker over a history that is trivially linearizable proves nothing".

A totally ordered log is that object, and it is the next thing the book builds. Both of its
algorithms already have every dependency they need in this repository.

**This is a transcription, and says so.** The book's construction is a teaching device: it runs one
consensus instance per round, in lock-step — "processes keep on moving sequentially from one round to
the other" — so every entry pays a full consensus, and `unordered` and `delivered` grow without
bound. Multi-Paxos, which elides the first phase while a leader is stable, is **not in Cachin at
all**; the practical writeups are elsewhere. Building the page faithfully means inheriting its
omissions and stating them, exactly as `docs/bounded-space.md` requires — not fixing them here.

## What Changes

- **A port**, `TotalOrderLog<V>`, on the model of `link.rs` and `detector.rs`: an implementation
  keeps its own `Cmd` and `Ind`, and the port supplies the translations a layer above needs — build
  an append, build a read, classify an indication. The suite belongs to the port and the
  implementations are type arguments.
- **Two implementations, held to the same properties.** The repository's habit, and the reason to
  have a port at all:
  - **`ConsensusBasedTotalOrderBroadcast`** — Algorithm 6.1, over reliable broadcast and consensus,
    crash-stop.
  - **`LoggedUniformTotalOrderBroadcast`** — the fail-recovery variant, over logged uniform reliable
    broadcast and logged uniform consensus, whose entries survive a restart.
- **One departure, stated: the port offers a read.** The book's interface is total-order broadcast —
  `Broadcast(m)` and `Deliver(p, m)`, with no read at all. But both algorithms already maintain
  `delivered`, the totally ordered sequence, and a log's clients read it. So `read(from)` exposes
  what the page keeps and does not offer. It is served **locally**: the claim is a total order, not
  that a read sees the latest append.
- **A shared suite**, run against both: total order, agreement, validity, no duplication — and, for
  the logged one, that the sequence survives a restart.

Deliberately not in scope: multi-Paxos, and any bounding of `unordered` or `delivered`. Both belong
to a change with a proposal of its own, because bounding weakens a guarantee to a scope and
multi-Paxos is a different source.

## Capabilities

### New Capabilities

- `consensus/total-order-log-port`: what a layer above a totally ordered log may depend on, and the
  whole of what it may
- `consensus/consensus-based-total-order-broadcast`: Algorithm 6.1 — a totally ordered sequence
  agreed by one consensus instance per round, in the crash-stop model
- `consensus/logged-uniform-total-order-broadcast`: the same order in the fail-recovery model, where
  the sequence survives a restart

## Impact

`recon-protocols`: a `total_order_log` port module and two implementations, each composing modules
that already exist — `reliable_broadcast` with `leader_driven_consensus`, and
`logged_uniform_reliable_broadcast` with `logged_leader_driven_consensus`. Both hold a consensus
instance per round, so both replace a child as the round advances, which `Child::replace` already
does for the epoch consensus inside Paxos.

**Three core changes, each found by the compiler during implementation and approved as it appeared.** Composition took its mapper as a
function pointer — `wrap: fn(P::Msg) -> M` on `Child::run`, `msg: fn(CM) -> M` on all three
`Cx::with_child*` — and a pointer captures nothing, so a parent cannot stamp its child's messages
with its own state. Every layer so far was fine because the stamp lives in the *child*:
`epoch_consensus` writes `Tagged { ets, .. }` because the epoch is its own identity. Here the round
is *this* layer's concept and the consensus has never heard of rounds, so this is the first layer
that must stamp from outside. The mappers widen to `impl Fn`, which `Effect::map` already took.

**`SeqSlot`, the sequence half of `Slot`.** The fail-recovery member keeps a durable record of its
own and composes `logged_uniform_reliable_broadcast`, which appends — and a `Slot` scopes metadata
only. `store.rs` had already described the missing half in detail and declined to build it: "nothing
needs it… building it now would be the framework before its second consumer." This change is the
second consumer, and what was built is what that paragraph described.

**`KeyedSlot`, for a family of durable children.** One consensus instance per round, each keeping its
own record, so the parent needs a place *per round* rather than one fixed place. The key is passed as
**data** rather than captured, so a slot remains one fixed function of a key — which is what `Slot`'s
own note about not capturing was actually protecting, and it stands unchanged.

No simulator changes. `README.md`'s roadmap item `4–5`, its spec tree, its protocol
tables and its counts.
