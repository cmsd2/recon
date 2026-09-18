# recon

[![CI](https://github.com/cmsd2/recon/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/cmsd2/recon/actions/workflows/ci.yml)
[![API documentation](https://img.shields.io/badge/docs-API%20reference-blue)](https://cmsd2.github.io/recon/)

Distributed algorithms in Rust: links, failure detectors, broadcast, consensus and a replicated
log, written so that each module reads against the page it came from.

A protocol here is a synchronous state machine. It consumes an event and emits effects, and it never
touches a socket, a clock or a random number generator; time, randomness and storage arrive through
a context parameter. A deterministic simulator supplies the network, the clock and the faults, so
every run is reproducible from its seed and the whole test suite executes in one process. Nothing
opens a port, and a build guard fails any commit that tries.

Most modules transcribe Cachin, Guerraoui & Rodrigues, *Introduction to Reliable and Secure
Distributed Programming* (2nd ed., 2011), with the pseudocode quoted above the implementation and
every departure stated in the module. Multi-Paxos comes from van Renesse & Altinbuken, *Paxos Made
Moderately Complex* (2015).

## Getting started

```bash
git clone https://github.com/cmsd2/recon && cd recon
cargo test --workspace          # 702 tests, all in-process
./scripts/check.sh              # the full gate: fmt, clippy, build, test, docs, guards
```

Rust 1.97 or newer, edition 2024. No system dependencies, no services, no network access at run
time. `openspec` (1.10.0) is needed only to work on the planning artifacts under `openspec/`.

For a first look at what the project is for:

```bash
cargo test -p recon-protocols --test broadcast_over_sessions -- --nocapture
```

That suite runs reliable and uniform reliable broadcast through the same schedule, a relay lost to a
session ending, and shows the first leaving a correct process without the message where the second
cannot.

## How the work is ordered

[`docs/postmortem.md`](docs/postmortem.md) is the account of the first attempt, which spent
seventeen months on the transport layer, wrote the connection manager four times, and never reached
the algorithms. Six constraints came out of it, and the build guards enforce the ones that can be
enforced mechanically. The numbering is used throughout.

1. **Algorithms before transport.** No `TcpStream`, no reconnect logic, no multi-process scripts
   until several protocols run against an in-memory network in one test process.
2. **The protocol core is sans-IO.** A protocol never awaits, never reads a clock, never calls
   `thread_rng`, never touches a socket. Time and randomness come through the context so they can be
   made virtual and seeded.
3. **The simulator is the deliverable.** Seeded RNG, virtual clock, a priority queue of scheduled
   deliveries, and knobs for latency, loss, duplication, reordering, partition and crash/restart.
   Correctness is asserted as properties over the delivery trace.
4. **Compose statically; extract the DSL, don't design it.** Parents own children as typed fields
   and re-wrap child effects. Write two or three protocols by hand before writing any macro.
5. **Transport last.** When protocols work under simulation, the network layer is a thin adapter:
   best-effort `send` plus a session-changed event. QUIC rather than TCP plus reconnect logic,
   because it supplies connection identity, multiplexing and framing.
6. **Build the abstractions in order.** Fair-loss link, perfect link, failure detector, best-effort
   broadcast, reliable broadcast, uniform reliable broadcast, consensus. Each is tested against its
   stated guarantees before the next begins.

## The crates

API documentation is published from `master` at **<https://cmsd2.github.io/recon/>**. Most of what
is worth reading is in the module documentation: the quoted pseudocode, the departures from the
page, and the space bound each module claims.

| Crate | What it is |
|---|---|
| [`recon-core`](crates/recon-core) | The `Protocol` trait, the effect vocabulary, `Cx`, `Time`, `NodeId`, `SessionEvent`, storage slots, error conventions. Everything depends on this and it depends on nothing else. |
| [`recon-sim`](crates/recon-sim) | The deterministic simulator: seeded RNG, virtual clock, fault injection, and a trace that properties are asserted over. It *is* the fair-loss network. |
| [`recon-protocols`](crates/recon-protocols) | The protocols, one module each. |

### recon-core

```rust
pub trait Protocol {
    type Cmd; type Ind; type Msg;      // from above, to above, on the wire
    type Meta; type Entry;             // durable state: one rewritten value, one appended sequence
    type Scope;                        // the interval a guarantee holds over
    type Note;                         // what the protocol narrates, if anything

    fn on_cmd(&mut self, cmd: Self::Cmd, cx: &mut ProtoCx<'_, Self>);
    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>);
    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>);
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {}
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {}
    fn on_scope_event(&mut self, scope: Self::Scope, cx: &mut ProtoCx<'_, Self>) {}
}

pub enum Effect<M, I> {
    Send { to: NodeId, msg: M },
    Indicate(I),
    SetTimer { after: Duration, id: TimerId },
}
```

Four things are worth knowing before reading a module.

**Storage is supplied through the context**, like time and randomness, rather than emitted as an
effect. `cx.storage()` offers a `Meta` value that is replaced and an `Entry` sequence that is
appended. A write is durable when it returns, which is the only point at which a driver can
synchronise with a synchronous protocol, so a process cannot be seen by its peers to have made a
promise it has no record of. A protocol that keeps nothing declares both types `Infallible`, and a
write cannot be constructed for it. The same trick covers `Scope` and `Note`: `Infallible` means the
event cannot exist.

**A timer is a handle, not a type.** `Cx::set_timer` returns a `TimerId` the driver issued and the
same handle comes back to `on_timer`. A layer that registers no timer declares nothing about timers,
and inserting a layer leaves the timers beneath it alone. The price is that every expiry is offered
to every layer, so a layer that registered a timer compares before acting.

**Composition is static.** A parent owns its children as typed fields and re-wraps their effects.
One question decides the form: does the layer transform its child's indications or pass them on?
Forwarding layers use `Cx::with_child`; transforming layers hold a `Child<P>` and call `run`, which
hands the child's indications back for the parent to handle. A parent that keeps durable state and
composes a child that does names the child's part of its record with a `Slot` and calls
`run_durable`, so the child's write is a read-modify-write of one record rather than two writes with
a crash-sized interval between them. `SeqSlot` does the same for the appended half, and `KeyedSlot`
serves a family of children, one record per key.

**Narration goes into the trace.** A record of effects says what a protocol did, and is silent about
a decision whose outcome was to do nothing. `cx.note(..)` records the decision, and a test can then
require the narration to agree with the run. A note's vocabulary belongs to the run rather than to a
layer, as a `TimerId` does, so it passes through composition untouched.

Files: [`protocol.rs`](crates/recon-core/src/protocol.rs) ·
[`effect.rs`](crates/recon-core/src/effect.rs) · [`cx.rs`](crates/recon-core/src/cx.rs) ·
[`child.rs`](crates/recon-core/src/child.rs) · [`store.rs`](crates/recon-core/src/store.rs) ·
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

**Two network models.** The default is fair-loss: messages may be lost, duplicated, delayed and
reordered, each knob configurable. `Config::sessions()` switches to the session model, which is what
TCP or QUIC gives: reliable and ordered within a session, and when the session ends an unknown
suffix of what was in flight is gone. Sessions re-establish on their own, because that is what a
reconnecting link does.

**Faults.** `crash` then `restart` for failure; `suspend` then `resume` for a stall; `sever`,
`reconnect`, `partition`, `heal` and `break_session` for the network; `crash_on_next_write` for
dying inside a write, with the seed deciding whether it landed. The two pairs differ: a crash loses
volatile state and takes the startup branch on the way back, while a stall keeps its state and is
handed back every timer, delivery and scope event that came due while it was away. What a stall does
lose is the clock. Connectivity is between pairs, so `sever(A, C)` on three processes leaves a
bridge: `A` reaches `B`, `B` reaches `C`, `A` does not reach `C`, and none of the three is wrong
about what it can see. `partition` is the special case where the severed pairs span two groups.

**The trace.** Properties are asserted over `s.trace()`, never over protocol internals. `command`
returns an `OpId`, and the trace records when the process handled the operation, or that it never
did and why. `record_notes()` adds what protocols said to what happened to them, and
`enable_tracing()` renders every recorded event to a `tracing` subscriber with the process and the
run's virtual time. Both are off by default, and narrating cannot change a run.

**Stepping by event.** `command` schedules; it does not run. `step_now()` dispatches everything due
at the current instant and leaves the clock alone; `step()` dispatches one event. A test that needs
"sent but not yet delivered" uses those rather than a duration guessed shorter than the latency.

**Shrinking.** A `Scenario` holds a run as a value, a configuration with its seed, a membership,
timed `Step`s and a horizon, and `shrink` reduces a failing one against a predicate to a fixed
point, rendering the result as Rust to paste. A reduced scenario is a different run that also fails,
not a prefix of the original, which is why every candidate is re-run rather than reasoned about.

Files: [`sim.rs`](crates/recon-sim/src/sim.rs) · [`config.rs`](crates/recon-sim/src/config.rs) ·
[`trace.rs`](crates/recon-sim/src/trace.rs) · [`codec.rs`](crates/recon-sim/src/codec.rs) ·
[`scenario.rs`](crates/recon-sim/src/scenario.rs) · [`shrink.rs`](crates/recon-sim/src/shrink.rs) ·
[`narrate.rs`](crates/recon-sim/src/narrate.rs)

## The protocols

Each module states in its own documentation whether it is a **transcription** (faithful to the page,
inheriting the book's omissions, which include garbage collection) or an **implementation** (the
same guarantees, with state bounded by something other than how long it has been running), and what
bounds its space. [`docs/bounded-space.md`](docs/bounded-space.md) says why the distinction
matters. The status and space columns below repeat what each module claims for itself.

The bottom abstraction, fair-loss links, is not a module: the simulator provides it.

### Over fair-loss links: the book's sequence

| Abstraction | Module | Book | Status | Space |
|---|---|---|---|---|
| Fair-loss link | [`fair_loss_link.rs`](crates/recon-protocols/src/fair_loss_link.rs) | Module 2.1 | the simulator's own guarantee, named | none |
| Stubborn link | [`stubborn_link.rs`](crates/recon-protocols/src/stubborn_link.rs) | Module 2.2, Alg. 2.1 | academic | unbounded |
| Perfect link | [`perfect_link.rs`](crates/recon-protocols/src/perfect_link.rs) | Module 2.3, Alg. 2.2 | academic as written | unbounded |
| Perfect failure detector | [`perfect_failure_detector.rs`](crates/recon-protocols/src/perfect_failure_detector.rs) | Module 2.6, Alg. 2.5 | deployable where synchrony is real | bounded by membership |
| Eventually perfect failure detector, ◇P | [`eventually_perfect_failure_detector.rs`](crates/recon-protocols/src/eventually_perfect_failure_detector.rs) | Module 2.8, Alg. 2.7 | **implementation** | bounded by membership |
| Detector port | [`detector.rs`](crates/recon-protocols/src/detector.rs) | — | port | — |
| Eventual leader detector, Ω | [`eventual_leader_detector.rs`](crates/recon-protocols/src/eventual_leader_detector.rs) | Module 2.9, Alg. 2.8 | **implementation**, over ◇P | bounded by membership |
| Best-effort broadcast | [`best_effort_broadcast.rs`](crates/recon-protocols/src/best_effort_broadcast.rs) | Module 3.1, Alg. 3.1 | deployable | bounded by membership |
| Reliable broadcast | [`reliable_broadcast.rs`](crates/recon-protocols/src/reliable_broadcast.rs) | Module 3.2, Alg. 3.3 | transcription | unbounded |
| Uniform reliable broadcast | [`uniform_reliable_broadcast.rs`](crates/recon-protocols/src/uniform_reliable_broadcast.rs) | Module 3.3, Alg. 3.4 | transcription | unbounded |
| Uniform reliable broadcast, majority-ack | [`majority_ack_uniform_reliable_broadcast.rs`](crates/recon-protocols/src/majority_ack_uniform_reliable_broadcast.rs) | Module 3.3, Alg. 3.5 | transcription, **no failure detector** | unbounded |
| Probabilistic broadcast | [`probabilistic_broadcast.rs`](crates/recon-protocols/src/probabilistic_broadcast.rs) | Module 3.7, Alg. 3.9 | **implementation** | bounded by a retention window |
| Lazy probabilistic broadcast | [`lazy_probabilistic_broadcast.rs`](crates/recon-protocols/src/lazy_probabilistic_broadcast.rs) | Module 3.7, Alg. 3.10–3.11 | **implementation** | bounded by a retention window |
| Flooding consensus | [`flooding_consensus.rs`](crates/recon-protocols/src/flooding_consensus.rs) | Module 5.1, Alg. 5.1 | academic, fail-stop | bounded by membership and rounds |
| Epoch-change | [`epoch_change.rs`](crates/recon-protocols/src/epoch_change.rs) | Module 5.3, Alg. 5.5 | **implementation** | bounded by membership |
| Read/write epoch consensus | [`epoch_consensus.rs`](crates/recon-protocols/src/epoch_consensus.rs) | Module 5.4, Alg. 5.6 | **implementation** | bounded by membership |
| Leader-driven consensus (Paxos) | [`leader_driven_consensus.rs`](crates/recon-protocols/src/leader_driven_consensus.rs) | Module 5.2, Alg. 5.7 | **implementation** | bounded by membership |

### Over session links: what a deployment would run

The stubborn link belongs to the classroom. TCP and QUIC already retransmit, and the deployable link
needs less state than the perfect link, not more.

There is no second set of modules for this. Every broadcast above takes its link as a type
parameter bounded on [`link.rs`](crates/recon-protocols/src/link.rs), so the session stack is the
same modules with a different type argument. [`stacks.rs`](crates/recon-protocols/src/stacks.rs)
names the ready-made ones:

```rust
use recon_protocols::stacks::{
    BestEffortBroadcastOverSessions, UniformReliableBroadcastOverSessions,
};

type Beb = BestEffortBroadcastOverSessions<u32>;
type Urb = UniformReliableBroadcastOverSessions<u32>;
```

Supplying a link of your own has the same shape, with the layer's `Carried<P>` naming what that link
must carry:

```rust
type Beb = BestEffortBroadcast<u32, MyLink<u32>>;
type Urb = UniformReliableBroadcast<u32, MyLink<uniform_reliable_broadcast::Carried<u32>>>;
```

| Abstraction | Module | Status | Space |
|---|---|---|---|
| Link port | [`link.rs`](crates/recon-protocols/src/link.rs) | port | — |
| Ready-made stacks | [`stacks.rs`](crates/recon-protocols/src/stacks.rs) | — | — |
| Session link | [`session_link.rs`](crates/recon-protocols/src/session_link.rs) | deployable | bounded by membership |
| Gossip over sessions | `ProbabilisticBroadcastOverSessions`, `LazyProbabilisticBroadcastOverSessions` in `stacks.rs` | **the real-world set** | bounded by a retention window; idle cost zero |

The two broadcast abstractions **diverge** here, and
[`tests/broadcast_over_sessions.rs`](crates/recon-protocols/tests/broadcast_over_sessions.rs) is
built around the contrast. Reliable broadcast relays once and keeps identifiers rather than
payloads, so a relay lost to a session ending is never retried and its agreement is scoped to the
sessions that carried it. Uniform reliable broadcast keeps payloads and consults a failure detector,
so between resending on re-establishment and accusing a peer that never returns there is no third
outcome.

### Over stable storage: the fail-recovery model

A crash-stop protocol tells the layer above `⟨ Deliver | m ⟩` once. A crash-recovery protocol
cannot, because it may crash immediately afterwards and then nothing anywhere knows the indication
happened. So these protocols write the message into a durable log, and the indication says only
that the log may have changed. The layer above reads it, and must be idempotent, because the same
log arrives again after every restart.

| Protocol | Module | Book | Status | Space |
|---|---|---|---|---|
| Logged perfect link | [`logged_link.rs`](crates/recon-protocols/src/logged_link.rs) | Module 2.4, Alg. 2.3 | transcription | unbounded, **on disk** |
| Stubborn broadcast | [`stubborn_broadcast.rs`](crates/recon-protocols/src/stubborn_broadcast.rs) | §3.5 | deployable in the fail-recovery model | bounded by membership |
| Logged uniform reliable broadcast | [`logged_uniform_reliable_broadcast.rs`](crates/recon-protocols/src/logged_uniform_reliable_broadcast.rs) | Module 3.6, Alg. 3.8 | transcription | unbounded, **on disk** |
| Logged epoch-change | [`logged_epoch_change.rs`](crates/recon-protocols/src/logged_epoch_change.rs) | Module 5.6, Alg. 5.8 | **implementation** | bounded by membership, plus what the stubborn children hold |
| Logged read/write epoch consensus | [`logged_epoch_consensus.rs`](crates/recon-protocols/src/logged_epoch_consensus.rs) | Module 5.7, Alg. 5.9 | **implementation** | bounded by membership, plus what the stubborn children hold |
| Logged leader-driven consensus (Paxos) | [`logged_leader_driven_consensus.rs`](crates/recon-protocols/src/logged_leader_driven_consensus.rs) | Module 5.5, Alg. 5.10–5.11 | **implementation** | bounded by membership, plus what the stubborn children hold |
| Total-order log port | [`total_order_log.rs`](crates/recon-protocols/src/total_order_log.rs) | — | port | none |
| Consensus-based total-order broadcast | [`consensus_based_total_order_broadcast.rs`](crates/recon-protocols/src/consensus_based_total_order_broadcast.rs) | Module 6.1, Alg. 6.1 | transcription | unbounded |
| Logged uniform total-order broadcast | [`logged_uniform_total_order_broadcast.rs`](crates/recon-protocols/src/logged_uniform_total_order_broadcast.rs) | Module 6.12, Alg. 6.12 | transcription | unbounded, in stable storage too |
| Multi-Paxos, the Synod protocol | [`multi_paxos_synod.rs`](crates/recon-protocols/src/multi_paxos_synod.rs) | **not in Cachin**: vRA (2015) §2 and §4.1–4.2, Figs. 4, 6, 7 | **implementation** | bounded by membership and the collection window |
| Multi-Paxos, the replica | [`multi_paxos_replica.rs`](crates/recon-protocols/src/multi_paxos_replica.rs) | **not in Cachin**: vRA (2015) §2.1 and §4.2, Fig. 1 | **implementation** | bounded by a retention window; the ordered sequence is exempt |

Two more things change in this model. Startup becomes a branch: a process with nothing in storage
is initialised, one with something is recovered, and exactly one of the two runs. And
retransmission stops being waste: a process that was down when a message was sent has no record of
it and no way to ask, so the only thing that reaches it is a sender that never stopped trying. That
is why stubborn broadcast does not deduplicate, and why these protocols are built over it rather
than over the perfect link.

The total-order log port has three implementations, and the differences are the point. The
consensus-based and logged uniform members are Algorithm 6.1 in the crash-stop and fail-recovery
models, and between the two exactly one thing differs: whether the sequence survives a restart. The
Multi-Paxos replica is a different algorithm, one consensus per leader rather than one per round,
so six appends become six commanders outstanding together where the pair run one instance at a
time. The shared suite runs every property against all three.

### The real-world set

Most of what is here is the book's algorithm kept faithful enough to read against the page. A small
number of modules are maintained as things that would ship, and they meet a second standard besides
correctness: they run over the session link rather than the stubborn one, and their resource use is
measured. Minimal messages for the work, and no growth in state or in send rate with how long they
have been running.

The set is the two gossip protocols and both halves of Multi-Paxos.

**Gossip.** `tests/probabilistic_broadcast_over_sessions.rs` asserts a broadcast's cost as an
identity, `k` sends per receipt with rounds to live and `Σ kⁱ` per broadcast when nothing is lost,
and that an idle gossip sends nothing. `tests/lazy_probabilistic_broadcast_over_sessions.rs` asserts
that a session ending is a loss the recovery phase repairs, at exactly `k` requests per gap.
Identity at both layers names the originator's incarnation, so a restarted originator's broadcasts
are neither discarded as duplicates by the eager layer nor as already delivered by the lazy one, and
a receiver keeps state for at most two incarnations of each originator.

**Multi-Paxos.** The message cost is an identity too: phase two costs one request and one reply per
other acceptor per entry plus one decision to every other process, and phase one is paid per
leadership change rather than per entry, which is what Multi-Paxos buys over one consensus instance
per entry. Getting there meant finding that the protocol was spending 3.6× that, because the sweep
interval doubled as the retransmission interval and every suite configured it below the delivery
bound. The state bound is the source's own §4.2: leaders and acceptors discard everything below the
slot that `f + 1` replicas have applied past, and a replica keeps its duplicate filter for a
retention window. Both bounds are conditional and both conditions are stated. What Multi-Paxos still
lacks is durability, which is §4.3 and a separate obligation. Single-instance Paxos stays out: it is
the book's stepping stone and is kept as one.

### Detectors versus quorums

Both stacks carry uniform reliable broadcast twice, and the pair is the point. Algorithm 3.4
delivers once every process still *believed correct* has relayed a message; that belief comes from a
perfect failure detector, and one wrong belief splits the delivery permanently. Algorithm 3.5 asks a
different question of the same record, whether more than half have relayed it, and the detector
comes out entirely, heartbeats and all.

What replaces a detector that must never be wrong is a majority that must be correct: `N > 2f`, a
standing property of the deployment rather than a moment-to-moment property of the network. When
that assumption fails, the majority versions block rather than diverge, which is repairable where a
split delivery is not. Over session links the all-ack version needs a peer to be accused before it
can stop waiting for it; under a quorum nobody is waited for individually, so a peer absent for
hours is not a stranger when it returns.

### Consensus, and what it rests on

Flooding consensus is the last of the fair-loss protocols and the only one whose limitation is not
about space. Its state is bounded, but its agreement rests entirely on the failure detector never
being wrong: the book's proof invokes strong accuracy by name, and one false suspicion splits the
decision permanently. Losing the detector's *accuracy* costs safety, two correct processes deciding
differently with nothing to detect or repair it. Losing its *completeness* costs only liveness.
`tests/flooding_consensus.rs` provokes the first with a partition inside synchronous mode, heals it,
and shows both decisions still standing.

`leader_driven_consensus` is built the other way round. Ω elects a leader, epoch-change turns
leadership into a numbered sequence of epochs, and each epoch is one abortable read/write consensus:
the leader reads from a majority, adopts the highest-timestamped value anyone had already accepted,
writes to a majority, and decides. Two majorities intersect, so a value decided in one epoch is what
every later epoch reads back. The detector is allowed to be wrong. Two processes may each believe
they lead, and the suite runs mostly in that condition, with a companion test confirming leadership
was disputed, because an agreement assertion over a run with one unchallenged leader proves nothing.
What an inaccurate detector costs here is termination: `tests/leader_driven_consensus.rs` runs the
partition that splits flooding consensus, and the minority waits. Termination is stated as
conditional, on a correct majority and a detector that eventually settles, which is what FLP
requires.

The fail-recovery version, Algorithms 5.8 to 5.11, makes the epoch entered, the value accepted and
the decision reached durable before anything reveals them. A process that dies inside the write
comes back either having accepted or not, never having promised without a record. Its suite runs
crashes, recoveries and a lying detector in the same run, with a non-vacuity half asserting all three
happened.

## What comes next

Two tracks. The **protocol** track continues the book's sequence: an accrual detector, periodic
re-announcement of standing facts, bounding the receiver-side sets that still grow with messages
handled, and the log protocols above Multi-Paxos (ZAB, viewstamped replication, Raft). The
**evidence** track builds what lets a failure be found rather than anticipated: per-node clocks and
skew, indeterminate outcomes in the trace, a concurrent workload generator, and a checker written
once against the total-order log port. Non-transitive partitions, invocations in the trace,
shrinking, and logging and tracing are built.

[`docs/roadmap.md`](docs/roadmap.md) has each item in full, with the decisions already taken: why
indications are not paired to invocations, why re-announcement without an attribution test would
hide the bugs it repairs, and what shrinking turned out to buy when measured against the project's
own defects.

## Examples

There are none yet. The tests are the worked examples: `tests/broadcast_over_sessions.rs` reads as
one, and `tests/method.rs` documents how a property is asserted so that it cannot pass vacuously.
An `examples/` directory belongs here once there is something to run that is not a test, which
waits on transport under constraint 5.

## Documentation

| Document | What it says |
|---|---|
| [`docs/postmortem.md`](docs/postmortem.md) | The first attempt, January 2018 to May 2019: four transport layers, one algorithm, and the six constraints that govern this one. |
| [`docs/bounded-space.md`](docs/bounded-space.md) | Transcriptions versus implementations, the rule that state is bounded by membership, a window or a capacity but never by messages handled, and an audit of which abstractions currently break it. |
| [`docs/conditional-guarantees.md`](docs/conditional-guarantees.md) | Why every guarantee is bounded by a scope, why the end of that scope is a first-class event, and why a layer that cannot bridge a scope ending must propagate it. |
| [`docs/scope-annotated-modules.md`](docs/scope-annotated-modules.md) | The formal companion: an extension to the book's module notation, proved conservative, with composition rules and a lower bound on what a layer can bridge. |
| [`docs/roadmap.md`](docs/roadmap.md) | The protocol and evidence tracks, item by item, with the decisions each one records. |
| [`DEFECTS.md`](DEFECTS.md) | The 2026-08 contract audit: every module read against its quoted pseudocode, twenty defects, and the test that now holds each fix in place. |

Module and algorithm numbers refer to Cachin, Guerraoui & Rodrigues, 2nd edition, except where a
table says otherwise.

## Specifications

`openspec/specs/` holds the current specification for each capability, what the system must do
independent of how. `openspec/changes/archive/` holds the proposals that got it there, each with
design notes and a task list.

```
openspec/specs/
├── protocol-core/                     the trait, the effects, composition
├── simulation/                        determinism, faults, sessions, the trace
├── links/                             the port, fair-loss, stubborn, perfect,
│                                      session, logged
├── failure-detection/                 perfect and eventually perfect failure
│                                      detectors, eventual leader detector
├── broadcast/                         best-effort, reliable, uniform reliable,
│                                      majority-ack, stubborn, logged uniform
│                                      reliable, and the two probabilistic ones
└── consensus/                         flooding consensus, epoch-change, epoch
                                       consensus, leader-driven consensus, the
                                       logged version of each of the last three,
                                       the total-order log port and its members,
                                       and the Multi-Paxos Synod protocol
```

Work is proposed, applied and archived through OpenSpec. In Claude Code these are slash commands:
`/opsx:propose`, `/opsx:apply`, `/opsx:archive`, and `/opsx:explore` for thinking without
committing to anything. Project context and per-artifact rules live in `openspec/config.yaml`.

## Prior art

- **Cachin, Guerraoui & Rodrigues**, *Introduction to Reliable and Secure Distributed Programming*,
  2nd ed. (Springer, 2011). The abstractions, the module notation, and the pseudocode.
- **van Renesse & Altinbuken**, 'Paxos Made Moderately Complex', *ACM Computing Surveys* 47(3),
  2015. Multi-Paxos, with the state reductions of §4 on the page. Cross-checked against Liu, Chand &
  Stoller, 'Moderately Complex Paxos Made Simple', PPDP '19, whose TLA+-checked specification is the
  source of three liveness fixes and one acceptor fix the module applies.
- **The KTH distributed systems course** and its Scala/Kompics DSL, where the idea of writing these
  algorithms as composable message-passing components comes from.
- **`quinn-proto`, `rustls`, `raft-rs`**: sans-IO protocol cores in Rust, each a synchronous state
  machine with the runtime pushed to the edges, which is the shape constraint 2 asks for.

## Developing

### The gate

`./scripts/check.sh` must pass in full before every commit. A pre-commit hook runs it, and CI runs
the same script on `master` and on every pull request, so the two cannot disagree about what "clean"
means. It aggregates rather than stopping at the first failure, so one run names everything that is
wrong.

```bash
cargo fmt --all                                        # rustfmt.toml is checked in
cargo clippy --workspace --all-targets -- -D warnings  # a lint is a build failure
cargo build --workspace --all-targets
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

### The guards

Five mechanical checks run inside the gate. Each catches a failure that is silent at run time.

| Guard | Forbids | Because |
|---|---|---|
| [`check-ordered-maps.sh`](scripts/check-ordered-maps.sh) | `HashMap` / `HashSet` in the three crates | iteration order varies per process and silently breaks seed reproducibility |
| [`check-error-types.sh`](scripts/check-error-types.sh) | `io::Error` for domain failures, and the literal `"json decoding error"` | flattening distinct failures into one string makes them indistinguishable in a running cluster |
| [`check-no-transport.sh`](scripts/check-no-transport.sh) | sockets, async runtimes, `.await` | constraint 1, algorithms before transport |
| `cargo clippy -D warnings` | any lint | warnings accumulate and hide real diagnostics |
| `cargo doc -D warnings` | a broken intra-doc link | a docstring naming something that is not there asserts a contract the code does not have, and the link renders as plain text with no error. It had happened four times when the check was added |

`check-no-transport.sh` is meant to be deleted deliberately, in the commit that introduces transport
under constraint 5. Do not weaken it.

Two more checks stay outside the gate, because each rebuilds a crate under a feature and runs a
suite again, a minute against the seconds the others cost. Both work the same way: break the thing
a test claims to protect, and require the red.

[`check-durability-tests.sh`](scripts/check-durability-tests.sh) builds with
`--features lose-storage-on-restart`, which makes `Sim::restart` discard what was written, and
requires the twenty-five tests registered in the script to fail. One that still passes is
reading the network rather than the disk, and in this project the network is always an available
answer: the stubborn children retransmit everything they have ever sent on every tick, so the
backlog in flight holds a full copy of the run. The audit that produced the list found two tests
with identical structure and intent where only one leaked, and three whose stated purpose was
durability and which could not have detected its absence. Run it when touching recovery, storage, or
any test that restarts a process.

[`check-safety-tests.sh`](scripts/check-safety-tests.sh) is the same instrument pointed at
agreement. It compiles three mutations of `multi_paxos_synod`, each removing one clause the safety
argument rests on: a leader that ignores what the majority reported, an acceptor that accepts below
its own promise, and a leader that proposes into a collected slot. Every test registered against a
mutation must fail under it. Agreement admits silent substitution easily, since "at most one
proposal chosen per slot" is satisfied by a run that chooses nothing. The guard has already caught
one regression: a later change to the leader left most of the registered tests green under the first
mutation, and the fix was a leadership handover in the schedules, the only thing that puts a
contradicting proposal in front of a leader once proposals are forwarded. Run it when touching
`multi_paxos_synod.rs` or its suite.

### Running one thing

```bash
cargo test -p recon-protocols --test perfect_link     # one suite
cargo test -p recon-protocols --test method           # the method's own tests
cargo test -p recon-sim -- the_same_seed              # by name
cargo test --workspace -- --nocapture                 # with output
```

### Adding an abstraction

1. Propose it with `/opsx:propose "..."`, and let the proposal say what guarantee is added and what
   it costs. An abstraction that weakens a guarantee to a scope is a change with a specification,
   not a cleanup commit.
2. Write the module with the pseudocode quoted above the implementation. State in its documentation
   whether it is a transcription or an implementation, what bounds its space, and every departure
   from the page.
3. Compose statically. The parent owns the child as a `Child<P>` field and re-wraps its effects;
   `child.run(cx, wrap, f)` returns the child's indications, the parent handles them, and
   `child.reclaim(inds)` gives the buffer back. Timings for the leader-driven family travel as a
   `Timing`, not as positional durations.
4. Test it against its stated guarantees, and assert non-vacuity: an absence-of-violation property
   is satisfied by a protocol that does nothing.
   [`tests/method.rs`](crates/recon-protocols/tests/method.rs) demonstrates that failure and guards
   against it.
5. Register it in `crates/recon-protocols/src/lib.rs`, add it to the protocol table in this README,
   run `./scripts/check.sh`, and archive the change with `/opsx:archive` so the specification is
   synced.

### Where the tests live

| Suite | Covers | Tests |
|---|---|---|
| [`recon-core/tests/core_contract.rs`](crates/recon-core/tests/core_contract.rs) | the trait, effects, composition, determinism, a durable child inside a durable parent, and a child's narration passing through untouched | 31 |
| [`recon-sim/tests/simulation.rs`](crates/recon-sim/tests/simulation.rs) | determinism, faults, sessions, storage, the trace, timer handles, stepping by event, severing pairs | 91 |
| [`recon-sim/tests/invocations.rs`](crates/recon-sim/tests/invocations.rs) | an operation's beginning recorded when it was handled rather than scheduled, and one that never began recorded with why | 10 |
| [`recon-sim/tests/narration.rs`](crates/recon-sim/tests/narration.rs) | a note reaching the trace with its process and instant, a decision to do nothing leaving only its note, and narrating changing nothing | 8 |
| [`recon-sim/tests/scenario.rs`](crates/recon-sim/tests/scenario.rs) | a run described as a value, and the reduction of a failing one: what comes back still fails, reduces twice to the same answer, and renders as Rust that the test compiles and runs | 15 |
| [`recon-protocols/tests/method.rs`](crates/recon-protocols/tests/method.rs) | how a property is asserted so it cannot pass vacuously | 10 |
| [`tests/link_port.rs`](crates/recon-protocols/tests/link_port.rs), `foreign_link.rs` | both links satisfy the port, a protocol is not a link by accident, and a link this project never wrote carries the stack up to consensus | 6 / 3 |
| `tests/alloc_probe.rs` | what one delivery costs in allocations | 2 |
| `tests/stubborn_link.rs`, `perfect_link.rs`, `session_link.rs` | the links | 14 / 16 / 11 |
| `tests/perfect_failure_detector.rs` | completeness and accuracy, where accuracy is lost, and both sides of a stall | 17 |
| `tests/eventually_perfect_failure_detector.rs`, `detector_port.rs` | ◇P: a suspicion withdrawn, a timeout that moves both ways, what the cap costs, and two correct processes suspecting each other for ever across a bridge | 14 / 5 |
| `tests/best_effort_broadcast.rs`, `reliable_broadcast.rs`, `uniform_reliable_broadcast.rs` | the broadcasts over perfect links | 11 / 15 / 17 |
| `tests/best_effort_broadcast_over_sessions.rs`, `broadcast_over_sessions.rs` | the same modules over a session link, and where reliable and uniform diverge | 6 / 17 |
| `tests/majority_ack_uniform_reliable_broadcast.rs`, `majority_ack_over_sessions.rs` | the same guarantees without a failure detector, over each link | 18 / 16 |
| `tests/probabilistic_broadcast.rs`, `lazy_probabilistic_broadcast.rs` | gossip and its recovery phase: coverage over many seeds against a stated threshold, asserted not to be total, and a restarted originator's broadcasts delivered at both layers | 22 / 18 |
| `tests/probabilistic_broadcast_over_sessions.rs`, `lazy_probabilistic_broadcast_over_sessions.rs` | the real-world set's standard: cost as an identity, silence when idle, a session ending propagated once and repaired, a restart survived | 6 / 5 |
| `tests/logged_link.rs`, `stubborn_broadcast.rs`, `logged_uniform_reliable_broadcast.rs` | the fail-recovery model: durable logs, what a restart forgets, and what recovery must put back | 15 / 7 / 18 |
| [`tests/flooding_consensus.rs`](crates/recon-protocols/tests/flooding_consensus.rs) | consensus, what a false suspicion costs it, and a layer ignoring another layer's timer | 23 |
| `tests/eventual_leader_detector.rs`, `epoch_change.rs`, `epoch_consensus.rs` | Ω, the epochs it drives, and the quorum core: each with a flat send rate in time, leadership returning to a recovered process, and what a bridge does; `epoch_change` also checks its narration against the run | 12 / 18 / 19 |
| [`tests/leader_driven_consensus.rs`](crates/recon-protocols/tests/leader_driven_consensus.rs) | Paxos, run mostly where the leader detector is wrong, with the trace confirming a rival began before the old epoch finished everywhere; progress resuming when a healed partition restores the majority; agreement across a bridge whose two quorums share one process | 17 |
| `tests/logged_epoch_change.rs`, `logged_epoch_consensus.rs` | the same two over stable storage: durable before visible, what a restart must find, dying inside the write, a redelivered announcement answered once | 11 / 12 |
| [`tests/logged_leader_driven_consensus.rs`](crates/recon-protocols/tests/logged_leader_driven_consensus.rs) | Paxos under crashes, recoveries and a lying detector at once, with a non-vacuity half for all three, and dying inside the decision write | 12 |
| [`tests/total_order_log.rs`](crates/recon-protocols/tests/total_order_log.rs) | the shared suite, written against the port and run against all three implementations: total order, validity, no duplication, the read and its prefix consistency, a flat send rate, survivors still ordering after a permanent crash, and a non-vacuity half requiring overlapping operations | 29 |
| [`tests/logged_uniform_total_order_broadcast.rs`](crates/recon-protocols/tests/logged_uniform_total_order_broadcast.rs) | what only the fail-recovery member claims: the sequence survives a restart from its own storage, with the restarted process cut off from the network first so the retransmission backlog cannot rebuild it | 7 |
| [`tests/multi_paxos_synod.rs`](crates/recon-protocols/tests/multi_paxos_synod.rs) | the Synod protocol: a checker fed from the trace after every event; a seeded sweep beside hand-driven schedules for the edges randomness misses; the lost-message liveness violations with one message dropped; §4.4's colocated deployment; §4.1's state reduction; the cost identity at two membership sizes; and §4.2's collection, including agreement across a collection that left the acceptors remembering nothing | 59 |
| [`tests/multi_paxos_replica.rs`](crates/recon-protocols/tests/multi_paxos_replica.rs) | the replica of Figure 1: a passive process's append ordered by the leader, the window and its release, a command that loses its slot and comes back, decisions delivered in reverse and again, Liu et al.'s fourth liveness violation driven with one decision dropped, and §4.2's retention window with a stranded replica catching up from a peer | 21 |
| [`tests/shrinking_a_real_defect.rs`](crates/recon-protocols/tests/shrinking_a_real_defect.rs) | the shrinker against a defect this project had, put back behind a test-only switch | 3 |

687 across the suites above, plus nine unit tests inside `recon-core` and six doctests (four of them
`compile_fail`): 702 in total, all in one process. One further test is `#[ignore]`d because it
generates `rendered_scenario.rs.inc` rather than checking anything; the test that compares the
committed output against the renderer does the checking.

## Licence

Apache 2.0. See [`LICENSE-2.0`](LICENSE-2.0).
