# recon

Distributed message-passing algorithms — links, failure detection, broadcast, eventually consensus
— written in Rust so that the code reads as the algorithm.

Every protocol is a synchronous state machine: it consumes an event, emits effects, and never
touches a socket, a clock or a thread-local RNG. The network is a deterministic simulator that
runs entirely in one test process, so a failing run is reproducible from its seed. Nothing here
opens a port, and a build guard fails the commit that tries to.

## How this is ordered

Six constraints, which is what the build guards enforce. They are ordering rules rather than style
preferences, and the numbering is referred to throughout.

1. **Algorithms before transport.** No `TcpStream`, no reconnect logic, no multi-process shell
   scripts until several protocols run against an in-memory network in a single test process.
2. **The protocol core is sans-IO.** A protocol is a synchronous state machine that consumes
   events and emits effects. It never awaits, never reads a clock, never calls `thread_rng`, never
   touches a socket. Time and randomness arrive through the context parameter so they can be made
   virtual and seeded.
3. **The simulator is the deliverable, not the test harness.** Seeded RNG, virtual clock, a
   priority queue of scheduled deliveries, and knobs for latency, loss, duplication, reordering,
   partition and crash/restart. Correctness is asserted as properties over the delivery trace, and
   a failing run is reproducible from its seed.
4. **Compose statically; extract the DSL, don't design it.** Parents own children as concrete typed
   fields and re-wrap child effects rather than re-encoding them. Write two or three protocols by
   hand before writing any macro to remove the boilerplate.
5. **Transport last.** When protocols work under simulation, the network layer is a thin adapter:
   best-effort `send`, plus a session/epoch-changed event. QUIC over TCP-plus-reconnect-logic — it
   supplies connection identity, multiplexing and framing, which removes the need for a
   hand-rolled multiplexer entirely.
6. **Build the abstractions in order.** Fair-loss link → perfect link → failure detector → best-effort
   broadcast → reliable broadcast → uniform reliable broadcast → consensus. Each abstraction is tested
   against its stated guarantees before the next begins.

## Getting started

```bash
git clone https://github.com/cmsd2/recon && cd recon
cargo test --workspace          # 391 tests, all in-process, a few seconds
./scripts/check.sh              # the full gate: fmt, clippy, build, test, project guards
```

Rust 1.97 or newer, edition 2024. No system dependencies, no services to start, no network access
required at run time. `openspec` (1.10.0, installed via volta) is needed only to work on the
planning artifacts under `openspec/`, not to build or test.

Two-minute tour of what the thing actually does:

```bash
cargo test -p recon-protocols --test broadcast_over_sessions -- --nocapture
```

That suite runs reliable and uniform reliable broadcast through the same schedule — a relay lost
to a session ending — and shows the first leaving a correct process without the message where the
second cannot. It is the shortest path to seeing what the project is for.

## The crates

| Crate | What it is |
|---|---|
| [`recon-core`](crates/recon-core) | The `Protocol` trait, the effect vocabulary, `Cx`, `Time`, `NodeId`, `SessionEvent`, error conventions. Everything else depends on this and nothing else. |
| [`recon-sim`](crates/recon-sim) | The deterministic simulator. It *is* the fair-loss network, and it is the project's standard of evidence — seeded RNG, virtual clock, fault injection, and a trace that properties are asserted over. |
| [`recon-protocols`](crates/recon-protocols) | The protocols themselves, one module each, transcribed from the book with the pseudocode quoted above the implementation. |

### recon-core

Four events in, three effects out.

```rust
pub trait Protocol {
    type Cmd; type Ind; type Msg; type Timer; type Scope;
    fn on_cmd(&mut self, cmd: Self::Cmd, cx: &mut ProtoCx<'_, Self>);
    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>);
    fn on_timer(&mut self, token: Self::Timer, cx: &mut ProtoCx<'_, Self>);
    fn on_scope_end(&mut self, scope: Self::Scope, cx: &mut ProtoCx<'_, Self>) {}
}

pub enum Effect<M, I, T> {
    Send { to: NodeId, msg: M },
    Indicate(I),
    SetTimer { after: Duration, token: T },
}
```

Stable storage is supplied through the context, like time and randomness, rather than emitted as an
effect: `cx.storage()` offers a `Meta` value that is replaced and an `Entry` sequence that is
appended, plus a read from a position. A write is durable when it returns, which is the only point
at which a driver can synchronise with a synchronous protocol — so a process cannot be seen by its
peers to have made a promise it has no record of. A protocol that keeps nothing declares both types
`Infallible`, and then a write cannot be constructed for it.

`Scope` is the interval a guarantee holds over — a session, an incarnation, a deadline. A protocol
with no scopes writes `type Scope = Infallible`, and a scope end for it cannot be constructed.

Composition is static: a parent owns its children as concrete typed fields and re-wraps their
effects. `Cx::with_child` forwards a child's indications; a parent that transforms them holds the
child as a `recon_core::Child<P>` and calls `run`, which collects the child's indications and hands
them back for the parent to handle. Which one a layer uses is decided by one question — does it
transform its child's indications, or pass them on? A parent that keeps durable state and composes a
child that does names the child's part of its record with a `Slot` (`slot!(Parent, field)`) and
calls `run_durable`.

Files: [`protocol.rs`](crates/recon-core/src/protocol.rs) ·
[`effect.rs`](crates/recon-core/src/effect.rs) · [`cx.rs`](crates/recon-core/src/cx.rs) ·
[`time.rs`](crates/recon-core/src/time.rs) · [`node.rs`](crates/recon-core/src/node.rs) ·
[`session.rs`](crates/recon-core/src/session.rs) · [`error.rs`](crates/recon-core/src/error.rs)

### recon-sim

```rust
let mut s: Sim<MyProtocol> = Sim::new(
    Config::default().seed(7).sessions().synchronous(Duration::from_millis(20)),
    &[A, B, C, D],
    |me| MyProtocol::new(me, ALL),
);
s.command(A, Cmd::Broadcast(1));
s.partition(&[&[A, B, C], &[D]]);
s.run_for(Duration::from_millis(500));
s.heal();
```

Two network models. The default is **fair-loss**: messages may be lost, duplicated, delayed and
reordered, with each knob configurable. `Config::sessions()` switches to the **session model**,
which is what TCP or QUIC gives you — reliable and ordered within a session, and when the session
ends an unknown suffix of what was in flight is simply gone. Sessions re-establish on their own,
without either process sending, because that is what a reconnecting link does.

Faults: `crash` then `restart` for failure, `suspend` then `resume` for a stall, plus
`partition`, `heal` and `break_session`. The two pairs are distinct and not interchangeable: a
crash loses volatile state and takes the startup branch on the way back, while a stall keeps its
state and is *handed back* every timer, delivery and scope event that came due while it was away —
dropping one would lose a message inside a session that never ended. What a stall does take is the
clock. Properties are asserted over `s.trace()`, never over protocol internals.

Files: [`sim.rs`](crates/recon-sim/src/sim.rs) · [`config.rs`](crates/recon-sim/src/config.rs) ·
[`trace.rs`](crates/recon-sim/src/trace.rs) · [`codec.rs`](crates/recon-sim/src/codec.rs)

## The protocols

Each module states, in its own documentation, whether it is a **transcription** (faithful to the
page, inheriting the book's omissions — which include garbage collection) or an **implementation**
(same guarantees, state bounded by something other than how long it has been running), and what
bounds its space. See [`docs/bounded-space.md`](docs/bounded-space.md) for why that distinction is
load-bearing.

The bottom abstraction, fair-loss links, is not a module: it is what the simulator provides.

### Over fair-loss links — the book's own sequence

| Abstraction | Module | Book | Status | Space |
|---|---|---|---|---|
| Fair-loss link | [`fair_loss_link.rs`](crates/recon-protocols/src/fair_loss_link.rs) | Module 2.1 | the simulator's own guarantee, named | none |
| Stubborn link | [`stubborn_link.rs`](crates/recon-protocols/src/stubborn_link.rs) | Module 2.2, Alg. 2.1 | academic | unbounded |
| Perfect link | [`perfect_link.rs`](crates/recon-protocols/src/perfect_link.rs) | Module 2.3, Alg. 2.2 | academic as written | unbounded |
| Perfect failure detector | [`perfect_failure_detector.rs`](crates/recon-protocols/src/perfect_failure_detector.rs) | Module 2.6, Alg. 2.5 | deployable where synchrony is real | bounded by membership |
| Eventually perfect failure detector, ◇P | [`eventually_perfect_failure_detector.rs`](crates/recon-protocols/src/eventually_perfect_failure_detector.rs) | Module 2.8, Alg. 2.7 | **implementation** | bounded by membership |
| Detector port | [`detector.rs`](crates/recon-protocols/src/detector.rs) | — | — | — |
| Best-effort broadcast | [`best_effort_broadcast.rs`](crates/recon-protocols/src/best_effort_broadcast.rs) | Module 3.1, Alg. 3.1 | deployable | bounded by membership |
| Reliable broadcast | [`reliable_broadcast.rs`](crates/recon-protocols/src/reliable_broadcast.rs) | Module 3.2, Alg. 3.3 | transcription | unbounded |
| Uniform reliable broadcast | [`uniform_reliable_broadcast.rs`](crates/recon-protocols/src/uniform_reliable_broadcast.rs) | Module 3.3, Alg. 3.4 | transcription | unbounded |
| Uniform reliable broadcast, majority-ack | [`majority_ack_uniform_reliable_broadcast.rs`](crates/recon-protocols/src/majority_ack_uniform_reliable_broadcast.rs) | Module 3.3, Alg. 3.5 | transcription, **no failure detector** | unbounded |
| Flooding consensus | [`flooding_consensus.rs`](crates/recon-protocols/src/flooding_consensus.rs) | Module 5.1, Alg. 5.1 | academic, fail-stop | bounded by membership and rounds |
| Probabilistic broadcast | [`probabilistic_broadcast.rs`](crates/recon-protocols/src/probabilistic_broadcast.rs) | Module 3.7, Alg. 3.9 | **implementation** | bounded by a retention window |
| Lazy probabilistic broadcast | [`lazy_probabilistic_broadcast.rs`](crates/recon-protocols/src/lazy_probabilistic_broadcast.rs) | Module 3.7, Alg. 3.10–3.11 | **implementation** | bounded by a retention window |
| Eventual leader detector, Ω | [`eventual_leader_detector.rs`](crates/recon-protocols/src/eventual_leader_detector.rs) | Module 2.9, Alg. 2.8 | **implementation**, over ◇P | bounded by membership |
| Epoch-change | [`epoch_change.rs`](crates/recon-protocols/src/epoch_change.rs) | Module 5.3, Alg. 5.5 | **implementation** | bounded by membership |
| Read/write epoch consensus | [`epoch_consensus.rs`](crates/recon-protocols/src/epoch_consensus.rs) | Module 5.4, Alg. 5.6 | **implementation** | bounded by membership |
| Leader-driven consensus — Paxos | [`leader_driven_consensus.rs`](crates/recon-protocols/src/leader_driven_consensus.rs) | Module 5.2, Alg. 5.7 | **implementation** | bounded by membership |

### Over session links — what a deployment would run

The stubborn link belongs to the classroom: in a deployment TCP and QUIC already retransmit, and
the deployable link needs *less* state than the perfect link, not more.

There is **no second set of modules** for this. Every broadcast above takes its link as a type
parameter bounded on [`link.rs`](crates/recon-protocols/src/link.rs), so the session stack is the
same modules with a different type argument. [`stacks.rs`](crates/recon-protocols/src/stacks.rs)
names the ready-made ones, so a caller need not know how a layer wraps its payload:

```rust
use recon_protocols::stacks::{
    BestEffortBroadcastOverSessions, UniformReliableBroadcastOverSessions,
};

type Beb = BestEffortBroadcastOverSessions<u32>;
type Urb = UniformReliableBroadcastOverSessions<u32>;
```

Supplying a link of your own is the same shape, with the layer's `Carried<P>` naming what that link
must carry:

```rust
type Beb = BestEffortBroadcast<u32, MyLink<u32>>;
type Urb = UniformReliableBroadcast<u32, MyLink<uniform_reliable_broadcast::Carried<u32>>>;
```

| Abstraction | Module | Status | Space |
|---|---|---|---|
| Link port | [`link.rs`](crates/recon-protocols/src/link.rs) | — | — |
| Ready-made stacks | [`stacks.rs`](crates/recon-protocols/src/stacks.rs) | — | — |
| Session link | [`session_link.rs`](crates/recon-protocols/src/session_link.rs) | deployable | bounded by membership |
| Gossip over sessions | `ProbabilisticBroadcastOverSessions`, `LazyProbabilisticBroadcastOverSessions` in `stacks.rs` | **the real-world set** | bounded by a retention window; idle cost zero |

There were four forked `session_*` broadcast modules until the link became a parameter — around
2,000 lines whose algorithms were the originals with the link swapped underneath, and in which the
2026-08 audit found a quoted clause gone stale in one fork and not its sibling. Their tests survive
unchanged and now exercise the base modules; the modules themselves are gone.

There is a third stack besides these two: the fail-recovery protocols, below.

The interesting result is that the two broadcast abstractions **diverge** here. Reliable broadcast relays
once and keeps identifiers rather than payloads, so a relay lost to a session ending is never
retried and its agreement is scoped to the sessions that carried it. Uniform reliable broadcast
keeps payloads and consults a failure detector, so between resending on re-establishment and
accusing a peer that never returns there is no third outcome. Both halves are tested, and the
contrast is the point of `tests/broadcast_over_sessions.rs`.

### Over stable storage — the fail-recovery model

A crash-stop protocol tells the layer above `⟨ Deliver | m ⟩` once. A crash-recovery protocol
cannot: it may crash immediately afterwards, and then nothing anywhere knows the indication
happened — the message is lost in a notification that no longer exists. So these protocols write
the message into a **durable log**, and the indication says only that the log may have changed. The
layer above reads it, and must be idempotent, because the same log arrives again after every
restart.

| Protocol | Module | Book | Status | Space |
|---|---|---|---|---|
| Logged perfect link | [`logged_link.rs`](crates/recon-protocols/src/logged_link.rs) | Module 2.4, Alg. 2.3 | transcription | unbounded, **on disk** |
| Stubborn broadcast | [`stubborn_broadcast.rs`](crates/recon-protocols/src/stubborn_broadcast.rs) | §3.5 | deployable | bounded by membership |
| Logged uniform reliable broadcast | [`logged_uniform_reliable_broadcast.rs`](crates/recon-protocols/src/logged_uniform_reliable_broadcast.rs) | Module 3.6, Alg. 3.8 | transcription | unbounded, **on disk** |
| Logged epoch-change | [`logged_epoch_change.rs`](crates/recon-protocols/src/logged_epoch_change.rs) | Module 5.6, Alg. 5.8 | **implementation** | bounded by membership, plus what the stubborn children hold |
| Logged read/write epoch consensus | [`logged_epoch_consensus.rs`](crates/recon-protocols/src/logged_epoch_consensus.rs) | Module 5.7, Alg. 5.9 | **implementation** | bounded by membership, plus what the stubborn children hold |
| Logged leader-driven consensus — Paxos | [`logged_leader_driven_consensus.rs`](crates/recon-protocols/src/logged_leader_driven_consensus.rs) | Module 5.5, Alg. 5.10–5.11 | **implementation** | bounded by membership, plus what the stubborn children hold |

Two things change besides the indication. **Startup becomes a branch** — a process with nothing in
storage is initialised, one with something is recovered, exactly one runs, and both can emit
effects. And **retransmission stops being waste**: a process that was down when a message was sent
has no record of it and no way to ask, so the only thing that reaches it is a sender that never
stopped trying. That is why stubborn broadcast does not deduplicate, and why these protocols are
built over it rather than over the perfect link.

`logged_link` buys one thing, and its suite shows it directly: no-duplication holds **across a
restart**, where the perfect link — whose record is volatile — delivers the same message a second
time on the same schedule.

### The real-world set

Most of what is here is the book's algorithm kept faithful enough to read against the page, and
that is what it is for. A **small number** are maintained as things that would actually ship, and
they are held to a second standard besides correctness: they run over the **session link** rather
than the stubborn one, and their resource use is checked — minimal messages for the work, and no
growth in state or in send rate with how long they have been running. Today that set is the two
gossip protocols, and both now meet the standard. `tests/probabilistic_broadcast_over_sessions.rs`
asserts a broadcast's cost as an identity — `k` sends per receipt with rounds to live, `Σ kⁱ` per
broadcast when nothing is lost — and that an idle gossip sends **nothing**;
`tests/lazy_probabilistic_broadcast_over_sessions.rs` asserts that a session ending is a loss the
recovery phase repairs, at exactly `k` requests per gap. Identity at both layers names the
originator's incarnation, so a restarted originator's broadcasts are neither discarded as duplicates
by the eager layer nor as already-delivered by the lazy one — and a receiver keeps state for at most
two incarnations of each originator, so a restart costs a purge rather than a leak.
Multi-Paxos will join the set when it is written; single-instance Paxos will not — it is the book's
stepping stone, and is kept as one.

### Detectors versus quorums

Both stacks carry uniform reliable broadcast twice, and the pair is the point. Algorithm 3.4
delivers once every process still *believed correct* has relayed a message; that belief comes from
a perfect failure detector, and one wrong belief splits the delivery permanently. Algorithm 3.5
asks a different question of the same record — has **more than half** relayed it? — and the
detector comes out entirely, heartbeats and all.

What replaces a detector that must never be wrong is a majority that must be correct: `N > 2f`, a
standing property of the deployment rather than a moment-to-moment property of the network. When
*that* assumption fails, the majority versions **block rather than diverge**, which is a repairable
failure where a split delivery is not. Over session links it removes more than a dependency: the
all-ack version needs a peer to be *accused* before it can stop waiting for it, and under a quorum
nobody is ever waited for individually, so a peer absent for hours is not a stranger when it
returns.

This is the same trade the leader-driven consensus algorithms make, available here for one
function.

### Consensus, and what it rests on

Flooding consensus is the last of the fair-loss protocols, and the only one whose limitation is
not about space. Its state is bounded — one proposal set and one heard-from set per round, at most
`N` rounds — but its **agreement rests entirely on the failure detector never being wrong**. The
book's own proof of agreement invokes strong accuracy by name, and one false suspicion splits the
decision permanently.

The asymmetry is the reason it is worth having written. Losing the detector's *accuracy* costs
safety: two correct processes decide differently, and nothing detects or repairs it. Losing its
*completeness* costs only liveness: everyone blocks, but nobody is wrong. `tests/flooding_consensus.rs`
provokes the first with a partition inside synchronous mode, then heals it and shows both decisions
still standing — because a decision is irrevocable and both were taken before the system stabilised.

That is what a perfect failure detector is worth, and it is why the algorithms that get deployed
are built the other way round.

### Paxos, and what it does instead

`leader_driven_consensus` is that other way round, and the pair with flooding consensus is the point
of having written both. Ω elects a leader, epoch-change turns leadership into a numbered sequence of
epochs, and each epoch is one abortable read/write consensus: the leader reads from a majority,
adopts the highest-timestamped value anyone had already accepted, writes to a majority, and decides.
Two majorities intersect, so a value decided in one epoch is what every later epoch reads back.

The detector is allowed to be **wrong**. Two processes may each believe they lead, in overlapping
epochs, and the suite runs mostly in exactly that condition — with a companion test confirming
leadership really was disputed, because an agreement assertion over a run with one unchallenged
leader proves nothing. What an inaccurate detector costs here is termination, not agreement:
`tests/leader_driven_consensus.rs` runs the partition that splits flooding consensus and the
minority simply waits.

Termination is stated as conditional and tested that way — a correct majority and a detector that
eventually settles — which is what FLP requires and what flooding consensus pretends away.

The fail-recovery version, Algorithms 5.8 to 5.11, is built too: the epoch a process has entered,
the value it has accepted and the decision it reached are durable before anything reveals them, and a
process that dies inside the write comes back either having accepted or not, never having promised
without a record. Its suite runs crashes, recoveries and a lying detector in the same run, with a
non-vacuity half asserting all three actually happened.

That stack is the first here in which a protocol keeping durable state composes children that keep
durable state, and it needed a core change: `recon_core::Slot` names the part of a parent's record
that belongs to a child, and `Cx::with_durable_child_consuming` hands the child a store backed by it.
The child's write becomes a read-modify-write of the parent's record — **one write, not two**, so a
crash cannot land between a parent's record and its child's. Only the metadata is scoped; a child
that *appends* still cannot be composed, and the signature says so rather than a comment.

### Next

Two tracks. The **protocol** track continues the book's sequence; the **evidence** track builds what
would let a failure be found rather than anticipated. They are independent until the last item,
where they meet.

```
protocol track                        evidence track
──────────────                        ──────────────
1. accrual detector                   A. non-transitive partitions
2. defensive re-announcement          B. per-node clocks, and skew
3. Stop                               C. invocations in the trace
4. a replicated-log port              D. indeterminate outcomes
5. multi-Paxos ┐                      E. shrinking
   ZAB         ├── over it ───┐       F. logging and tracing
   VR, Raft…   ┘              │       G. a concurrent workload
                              └───────┴──▶  H. a checker, written once
                                             against the port
```

`H` is the only item needing both tracks, and it is last for a reason given under it.

**The evidence track is shared infrastructure, and that is what makes it worth its cost.** A
history model, a nemesis, a shrinker and a checker are built once and then serve multi-Paxos, ZAB,
viewstamped replication and anything else that replicates a log — the same way `link.rs` serves
every broadcast and `detector.rs` now serves Ω. None of it is scaffolding for one protocol.

#### What "Jepsen tests" means here, and what it does not

Not the tool. Jepsen is Clojure, drives a real cluster over SSH, and cannot reproduce a failure from
a seed — all three are things constraints 1, 2 and 3 rule out for now. What is worth having is its
**discipline**: record a history of client operations as intervals, inject faults while it runs, and
then *check the history against a model* rather than against a property somebody thought to assert.

Measured against that, most of the machinery is already here. The simulator is a better nemesis host
than a real cluster — seeded, reproducible, and with faults a real cluster cannot express, such as
`crash_on_next_write`, which kills a process *inside* the write and lets the seed decide whether it
landed. What is missing is the history and the checker, and items `C` to `H` are that.

#### 1. An accrual detector

Algorithm 2.7 adapts its timeout by adding Δ on every false suspicion and never subtracting, which
is a ratchet: one bad period leaves detection permanently sluggish, and nothing reports that it has.
`eventually_perfect_failure_detector` fixes the ratchet and caps the growth. The shape a deployment
actually wants goes one further — a **φ-accrual** detector (Hayashibara et al., 2004; Cassandra and
Akka both use one) keeps a bounded window of inter-arrival times and reports a *suspicion level*
rather than a verdict, leaving the threshold to the caller. That is the point of it: Ω picking a
leader can be aggressive, because being wrong costs one epoch, while a layer deciding to stop waiting
for an acknowledgement must be conservative, because being wrong costs safety. One detector, a
threshold each, priced by what a mistake costs that caller. A boolean detector forces one answer on
everybody. It needs the detector port `decreasing-eventually-perfect-detector` built.

#### 2. Defensive re-announcement of standing facts

The current leader, the current epoch, the membership: things that are *state*, not events, and that
a process joining late or recovering has no way to ask for. `epoch_change` already carries one repair
for this — a process tells a leader where it has reached, because an edge-triggered `⟨Ω, Trust⟩`
never tells a leader that never changed its mind — and that repair is narrow, reactive, and was found
by a test failing rather than by design. The general form is that a holder of a standing fact
re-announces it periodically, so a process that missed the edge converges without anyone having had
to anticipate how it missed it. It costs standing traffic against a stack that is otherwise silent
when idle, so it wants measuring rather than assuming, and it belongs to the real-world set.

#### 3. `Stop`

Every logged protocol here inherits an unbounded outstanding set from the stubborn children, because
nothing ever retires a transmission — see [`docs/bounded-space.md`](docs/bounded-space.md).

#### 4–5. A replicated-log port, and the protocols that implement it

The first object in this repository with a *concurrent* interface: a log that many clients append to
and read from, with operations overlapping in time. Everything up to here decides one value once,
which is why `H` cannot come earlier — see under it.

The port matters more than any one protocol behind it. Multi-Paxos, ZAB, viewstamped replication and
Raft solve the same problem by different routes, and this repository's habit is to build such a pair
and hold both to the same properties: uniform reliable broadcast against its majority-ack twin,
flooding consensus against Paxos. Doing that here means the *suite* belongs to the port and the
implementations are type arguments, exactly as the broadcasts take a link and Ω now takes a detector.

That is also what pays for the evidence track. A checker written against the port checks every
protocol behind it; a nemesis schedule that breaks one can be replayed against the others. The
comparison is the point — where they differ is in what they assume, and a shared suite is what makes
the difference visible rather than asserted.

#### A. Non-transitive partitions

`Sim::partition` takes groups, so a partition is symmetric **and transitive**: the network is always
a set of islands. Real ones are not.

```
    A ←──────→ B        A reaches B
               │        B reaches C
               ↕        A does NOT reach C
               C
```

`B` sees everyone; `A` and `C` each see a different world and neither is wrong. Quorum intersection
survives it, but *leader election* does not obviously: this whole stack rests on Ω converging, and Ω
converges by every process computing `maxrank` of the same set. This is the cheapest new fault to
add and the likeliest to find something.

#### B. Per-node clocks, and skew

`Sim` holds one global `now`, so there is no way to express two processes disagreeing about the time.
The failure detector is *entirely* about time, and the accrual detector above derives its threshold
from measured intervals — so this is the fault that stack is most exposed to and least tested
against. It is a real change to the simulator rather than a knob.

#### C. Invocations in the trace

The foundational one, and small. `Sim::command` schedules an operation and never records it, and
`TraceEvent` has `Indicated` but nothing for the invocation. So the trace holds completions without
the instants that began them:

```
Jepsen history               this trace
──────────────               ──────────
{:invoke :read  …}           (nothing)
{:ok     :read 3}            Indicated { at, node, ind }
     ▲         ▲
     └─────────┴─ an interval        an instant
```

Linearizability is *defined* over the interval `[invoke, complete]` — an operation may take effect
anywhere inside it — so no checker is possible without this, whatever else is built. It pays for
itself immediately regardless: no test can currently ask how long an operation took, or whether two
of them overlapped.

#### D. Indeterminate outcomes

Jepsen's third result is `:info` — *this may or may not have happened* — and it is the one that
matters most, because it is what a client experiences when its connection drops mid-write. A
`Propose` whose process crashes before any `Decide` is exactly that, and this repository currently
models it as *nothing happened*, which is a claim the code is not entitled to make: the value may
well be sitting in a quorum's `pending`. A checker fed that history would be reasoning from a false
premise. Needs an operation identity spanning invocation and outcome, so it follows `C`.

#### E. Shrinking

The thing this project can do that Jepsen cannot. Jepsen hands you a ten-thousand-operation history
and a failure and you read it; it cannot reliably reproduce, so it cannot minimise. A seeded
deterministic simulator can bisect the fault schedule, the operations and the run length until what
is left is a counterexample somebody can read.

Every diagnosis in these notes was reached by hand-writing a throwaway probe and printing a trace:
the epoch that climbed to 647,309, the send rate that grew 12.6k → 76.6k per window, the leader
trusted by everyone that announced nothing. A shrinker would have handed those over. It needs
nothing that does not already exist, and it improves every suite already written.

#### F. Logging and tracing

What a protocol *says* it is doing, as against what the trace records happening *to* it — `tracing`
spans and structured events at the decision points a reader would want under a real failure: an epoch
entered, a suspicion raised or withdrawn, a quorum reached, a write made durable, a scope ended.
Paired with `E`, that is the debugging story: a minimal schedule, and a narrated run of it.

Two constraints shape it. A protocol reaches for nothing ambient, so a subscriber cannot be a global
the way `tracing` usually installs one — it arrives through `Cx` like time and randomness do, or the
constraint is broken. And the sim's trace is already the standard of evidence, so the two must not
become rival accounts of one run.

#### G. A concurrent workload

A generator that issues overlapping operations against many processes at once, rather than the fixed
scripts every suite writes by hand today. Needs `C`, because an operation without an invocation has
no interval to overlap with.

#### H. A checker, written once against the port

Last, and the ordering is the point: **a checker over a history that is trivially linearizable
proves nothing.** Single-shot consensus decides one value once, and its agreement, validity and
termination are three lines of direct assertion — better than a general checker, not worse. The
question becomes hard only when operations overlap and read each other's writes, which is `4`.

Two models, and which one a protocol claims is part of what it is. A replicated log claims a **total
order**: every process sees the same sequence of entries, which is checkable directly from the
histories without searching. A register above it claims **linearizability**, which is the harder
question and needs the interval from `C` — an operation may take effect anywhere inside it. Written
against the port, both checks apply to every implementation behind it.

One caution particular to this project. A checker is exactly the kind of test
[`tests/method.rs`](crates/recon-protocols/tests/method.rs) exists to reject: it passes trivially on
a history with no concurrency, and Jepsen has no answer to that. Adopting one means extending the
non-vacuity discipline to it — assert the history *contained* overlapping operations, and assert the
checker can fail, by feeding it a mutated history and confirming it is rejected. A checker that has
never rejected anything is a checker nobody has tested.

#### Further out

Running the real thing, against a real cluster, over a real network — which is constraint 5's
territory and not before it. The value of doing `A` to `H` first is that by then a failure Jepsen
finds can be *reproduced* in the simulator from a seed, which is the half Jepsen itself cannot do.

## Examples

There are none yet. The tests are currently the worked examples — `tests/broadcast_over_sessions.rs`
reads as one, and `tests/method.rs` documents how a property is asserted so that it cannot pass
vacuously. An `examples/` directory belongs here once there is something to run that is not a
test; until transport exists under constraint 5, everything interesting is in-process.

## Documentation

| Document | What it says |
|---|---|
| [`docs/bounded-space.md`](docs/bounded-space.md) | Transcriptions versus implementations, the rule that state is bounded by membership or a window or a capacity but never by messages handled, and an audit of which abstractions currently break it. |
| [`docs/conditional-guarantees.md`](docs/conditional-guarantees.md) | Why every guarantee is bounded by a scope, why the end of that scope is a first-class event rather than an implementation detail, and what it means that a layer which cannot bridge must propagate. |
| [`docs/scope-annotated-modules.md`](docs/scope-annotated-modules.md) | The formal companion: the extension to the book's module notation, proved conservative, with composition rules and a lower bound on what a layer can bridge. |

The book throughout is Cachin, Guerraoui & Rodrigues, *Introduction to Reliable and Secure
Distributed Programming*, 2nd edition (Springer, 2011). Module and algorithm numbers refer to it,
and each module quotes the pseudocode it implements so the two can be read against each other.
Departures from the page are stated and justified in the module that departs.

## Specifications

`openspec/specs/` holds the current specification for each capability — what the system must do,
independent of how. `openspec/changes/archive/` holds the proposals that got it there, each with
its design notes and task list.

```
openspec/specs/
├── protocol-core/                     the trait, the effects, composition
├── simulation/                        determinism, faults, sessions, the trace
├── links/                             the port, fair-loss, stubborn, perfect,
│                                      session, logged
├── failure-detection/                 perfect and eventually perfect failure
│                                      detectors, the detector port, eventual
│                                      leader detector
├── broadcast/                         best-effort, reliable, uniform reliable,
│                                      majority-ack, the logged ones, and the
│                                      two probabilistic ones
└── consensus/                         flooding consensus, epoch-change, epoch
                                       consensus, leader-driven consensus, and
                                       the logged version of each of the last
                                       three
```

Work is proposed, applied and archived through OpenSpec. In Claude Code these are slash commands
(note the colon): `/opsx:propose`, `/opsx:apply`, `/opsx:archive`, and `/opsx:explore` for
thinking without committing to anything. Project context and per-artifact rules live in
`openspec/config.yaml`.

## Prior art

- **Cachin, Guerraoui & Rodrigues**, *Introduction to Reliable and Secure Distributed
  Programming*, 2nd ed. (Springer, 2011). The abstractions, the module notation, and the pseudocode
  every abstraction transcribes.
- **The KTH distributed systems course** and its Scala/Kompics DSL, which is where the idea of
  writing these algorithms as composable message-passing components comes from.
- **`quinn-proto`, `rustls`, `raft-rs`** — the prior art for sans-IO protocol cores in Rust. Each
  is a synchronous state machine with the runtime pushed to the edges, which is exactly the shape
  constraint 2 asks for.

## Developing

### The gate

`./scripts/check.sh` must pass in full before every commit. A pre-commit hook runs it. Do not
commit with anything outstanding — warnings accumulate into noise, and noise is how a real
diagnostic gets missed.

```bash
cargo fmt --all                                        # rustfmt.toml is checked in
cargo clippy --workspace --all-targets -- -D warnings  # a lint is a build failure
cargo build --workspace --all-targets
cargo test --workspace
```

### The guards

Four mechanical checks. They are part of the build because the failure modes they catch are silent
at runtime rather than loud — each one is a bug that would otherwise be found late, or never.

| Guard | Forbids | Because |
|---|---|---|
| [`check-ordered-maps.sh`](scripts/check-ordered-maps.sh) | `HashMap` / `HashSet` in the three crates | iteration order varies per process and silently breaks seed reproducibility |
| [`check-error-types.sh`](scripts/check-error-types.sh) | `io::Error` for domain failures, and the literal `"json decoding error"` | flattening distinct failures into one string makes them indistinguishable in a running cluster |
| [`check-no-transport.sh`](scripts/check-no-transport.sh) | sockets, async runtimes, `.await` | constraint 1: algorithms before transport, and this is what keeps it honest |
| `cargo clippy -D warnings` | any lint | warnings accumulate and hide real diagnostics |

`check-no-transport.sh` is meant to be **deleted deliberately**, in the commit that introduces
transport under constraint 5. Do not weaken it; delete it, or leave it alone.

### Running one thing

```bash
cargo test -p recon-protocols --test perfect_link     # one suite
cargo test -p recon-protocols --test method           # the method's own tests
cargo test -p recon-sim -- the_same_seed              # by name
cargo test --workspace -- --nocapture                 # with output
```

### Adding an abstraction

1. Propose it — `/opsx:propose "..."` — and let the proposal say what guarantee is being added and
   what it costs. An abstraction that weakens a guarantee to a scope is a change with a specification, not
   a cleanup commit.
2. Write the module with the pseudocode quoted above the implementation, and state in its
   documentation whether it is a transcription or an implementation, what bounds its space, and
   every departure from the page.
3. Compose statically. The parent owns the child as a `Child<P>` field and re-wraps its effects;
   it does not re-encode them. `child.run(cx, wrap, f)` returns the child's indications, the parent
   handles them, `child.reclaim(inds)` gives the buffer back. Timings for the leader-driven family
   travel as a `Timing`, not as positional durations.
4. Test it against its stated guarantees, and **assert non-vacuity**: an absence-of-violation
   property is satisfied by a protocol that does nothing.
   [`tests/method.rs`](crates/recon-protocols/tests/method.rs) demonstrates that failure and
   guards against it.
5. Register it in `crates/recon-protocols/src/lib.rs`, **add it to the protocol table in this
   README**, run `./scripts/check.sh`, and archive the change with `/opsx:archive` so the
   specification is synced.

### Where the tests live

| Suite | Covers | Tests |
|---|---|---|
| [`recon-core/tests/core_contract.rs`](crates/recon-core/tests/core_contract.rs) | the trait, effects, composition, determinism, and a durable child inside a durable parent | 29 |
| [`recon-sim/tests/simulation.rs`](crates/recon-sim/tests/simulation.rs) | determinism, faults, sessions, storage, the trace, timer handles, stepping by event | 83 |
| [`recon-protocols/tests/method.rs`](crates/recon-protocols/tests/method.rs) | how a property is asserted so it cannot pass vacuously | 10 |
| [`tests/link_port.rs`](crates/recon-protocols/tests/link_port.rs), `foreign_link.rs` | that both links satisfy the port, that a protocol is not a link by accident, and that a link this project never wrote carries the stack up to consensus | 6 / 3 |
| `tests/alloc_probe.rs` | what one delivery costs in allocations | 2 |
| `tests/stubborn_link.rs`, `perfect_link.rs`, `session_link.rs` | the links | 14 / 16 / 11 |
| `tests/perfect_failure_detector.rs` | completeness and accuracy, where accuracy is lost, and both sides of a stall | 17 |
| `tests/best_effort_broadcast.rs`, `reliable_broadcast.rs`, `uniform_reliable_broadcast.rs` | the broadcasts over perfect links | 11 / 15 / 17 |
| `tests/best_effort_broadcast_over_sessions.rs`, `broadcast_over_sessions.rs` | the same broadcast modules over a **session link**, and where reliable and uniform diverge | 6 / 17 |
| `tests/majority_ack_uniform_reliable_broadcast.rs`, `majority_ack_over_sessions.rs` | the same guarantees without a failure detector, over each link | 18 / 16 |
| `tests/logged_link.rs`, `stubborn_broadcast.rs`, `logged_uniform_reliable_broadcast.rs` | the fail-recovery model: durable logs, recovery, what a restart forgets, and what recovery must put back | 15 / 7 / 18 |
| [`tests/flooding_consensus.rs`](crates/recon-protocols/tests/flooding_consensus.rs) | consensus, what a false suspicion costs it, and that a layer ignores another layer's timer | 23 |
| `tests/probabilistic_broadcast.rs`, `lazy_probabilistic_broadcast.rs` | gossip and its recovery phase — coverage asserted over many seeds against a stated threshold, and asserted **not** to be total; a restarted originator's broadcasts delivered, at both layers | 22 / 18 |
| `tests/probabilistic_broadcast_over_sessions.rs`, `lazy_probabilistic_broadcast_over_sessions.rs` | the real-world set's standard: cost as an identity, silence when idle, a session ending propagated once and repaired by recovery, a restart survived | 6 / 5 |
| `tests/eventually_perfect_failure_detector.rs`, `detector_port.rs` | ◇P: a suspicion withdrawn, a timeout that moves both ways, and what the cap costs — swept against a latency rather than asserted | 13 / 5 |
| `tests/eventual_leader_detector.rs`, `epoch_change.rs`, `epoch_consensus.rs` | Ω, the epochs it drives, and the quorum core Paxos's safety argument lives in — each with a test that its send rate is flat in time, and that leadership can **return** to a recovered process | 11 / 13 / 19 |
| [`tests/leader_driven_consensus.rs`](crates/recon-protocols/tests/leader_driven_consensus.rs) | Paxos, run mostly where the leader detector is **wrong** — with a non-vacuity half reading from the trace that a rival began before the old epoch had finished everywhere, and progress resuming when a healed partition restores the majority | 15 |
| `tests/logged_epoch_change.rs`, `logged_epoch_consensus.rs` | the same two abstractions over stable storage: durable before visible, what a restart must find, dying inside the write, and that a redelivered announcement is answered once | 11 / 12 |
| [`tests/logged_leader_driven_consensus.rs`](crates/recon-protocols/tests/logged_leader_driven_consensus.rs) | Paxos under crashes, recoveries **and** a lying detector at once, with a non-vacuity half for all three, and dying inside the decision write | 12 |

516 across the suites above, plus nine unit tests inside `recon-core` and four doctests — two
`compile_fail` on the link and detector ports, two worked examples of a storage slot — 529 in total,
all in one process, no ports opened.

## Licence

Apache 2.0. See [`LICENSE-2.0`](LICENSE-2.0).
