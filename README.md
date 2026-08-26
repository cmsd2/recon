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
cargo test --workspace          # 261 tests, all in-process, a few seconds
./scripts/check.sh              # the full gate: fmt, clippy, build, test, project guards
```

Rust 1.97 or newer, edition 2024. No system dependencies, no services to start, no network access
required at run time. `openspec` (1.10.0, installed via volta) is needed only to work on the
planning artifacts under `openspec/`, not to build or test.

Two-minute tour of what the thing actually does:

```bash
cargo test -p recon-protocols --test session_broadcast -- --nocapture
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

`Scope` is the interval a guarantee holds over — a session, an incarnation, a deadline. A protocol
with no scopes writes `type Scope = Infallible`, and a scope end for it cannot be constructed.

Composition is static: a parent owns its children as concrete typed fields and re-wraps their
effects. `Cx::with_child` forwards a child's indications; `Cx::with_child_consuming` collects them
for a parent that transforms them. Which one a layer uses is decided by one question — does it
transform its child's indications, or pass them on?

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

Faults: `crash`, `suspend`, `restart`, `partition`, `heal`, `break_session`. Properties are
asserted over `s.trace()`, never over protocol internals.

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
| Stubborn link | [`stubborn_link.rs`](crates/recon-protocols/src/stubborn_link.rs) | Module 2.2, Alg. 2.1 | academic | unbounded |
| Perfect link | [`perfect_link.rs`](crates/recon-protocols/src/perfect_link.rs) | Module 2.3, Alg. 2.2 | academic as written | unbounded |
| Perfect failure detector | [`perfect_failure_detector.rs`](crates/recon-protocols/src/perfect_failure_detector.rs) | Module 2.6, Alg. 2.5 | deployable where synchrony is real | bounded by membership |
| Best-effort broadcast | [`best_effort_broadcast.rs`](crates/recon-protocols/src/best_effort_broadcast.rs) | Module 3.1, Alg. 3.1 | deployable | bounded by membership |
| Reliable broadcast | [`reliable_broadcast.rs`](crates/recon-protocols/src/reliable_broadcast.rs) | Module 3.2, Alg. 3.3 | transcription | unbounded |
| Uniform reliable broadcast | [`uniform_reliable_broadcast.rs`](crates/recon-protocols/src/uniform_reliable_broadcast.rs) | Module 3.3, Alg. 3.4 | transcription | unbounded |
| Uniform reliable broadcast, majority-ack | [`majority_ack_uniform_reliable_broadcast.rs`](crates/recon-protocols/src/majority_ack_uniform_reliable_broadcast.rs) | Module 3.3, Alg. 3.5 | transcription, **no failure detector** | unbounded |
| Flooding consensus | [`flooding_consensus.rs`](crates/recon-protocols/src/flooding_consensus.rs) | Module 5.1, Alg. 5.1 | academic, fail-stop | bounded by membership and rounds |

### Over session links — what a deployment would run

The stubborn link belongs to the classroom: in a deployment TCP and QUIC already retransmit, and
the deployable link needs *less* state than the perfect link, not more. These abstractions are the same
algorithms over a link that can end.

| Abstraction | Module | Status | Space |
|---|---|---|---|
| Session link | [`session_link.rs`](crates/recon-protocols/src/session_link.rs) | deployable | bounded by membership |
| Best-effort broadcast | [`session_best_effort_broadcast.rs`](crates/recon-protocols/src/session_best_effort_broadcast.rs) | deployable | bounded by membership |
| Reliable broadcast | [`session_reliable_broadcast.rs`](crates/recon-protocols/src/session_reliable_broadcast.rs) | transcription | unbounded |
| Uniform reliable broadcast | [`session_uniform_reliable_broadcast.rs`](crates/recon-protocols/src/session_uniform_reliable_broadcast.rs) | transcription | unbounded |
| Uniform reliable broadcast, majority-ack | [`session_majority_ack_uniform_reliable_broadcast.rs`](crates/recon-protocols/src/session_majority_ack_uniform_reliable_broadcast.rs) | transcription, **no failure detector** | unbounded |

The interesting result is that the two broadcast abstractions **diverge** here. Reliable broadcast relays
once and keeps identifiers rather than payloads, so a relay lost to a session ending is never
retried and its agreement is scoped to the sessions that carried it. Uniform reliable broadcast
keeps payloads and consults a failure detector, so between resending on re-establishment and
accusing a peer that never returns there is no third outcome. Both halves are tested, and the
contrast is the point of `tests/session_broadcast.rs`.

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

### Next

Eventually perfect failure detection (Module 2.8, `◇P`) and eventual leader election (`Ω`), then
the leader-driven family. Two things are missing for them: `Ω` and the detector beneath it, and
**stable storage** — a crash now genuinely loses volatile state, so an abstraction that must remember an
epoch or a promise across an incarnation has nowhere to put it.

## Examples

There are none yet. The tests are currently the worked examples — `tests/session_broadcast.rs`
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
├── links/                             stubborn, perfect, session
├── failure-detection/                 perfect failure detector
├── broadcast/                         best-effort, reliable, uniform reliable,
│                                      and the three session variants
└── consensus/                         flooding consensus
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
3. Compose statically. The parent owns the child as a concrete typed field and re-wraps its
   effects; it does not re-encode them. Extract a private helper per composing protocol rather
   than reaching for a macro — a helper leaves the borrow structure visible where a derive would
   hide it.
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
| [`recon-core/tests/core_contract.rs`](crates/recon-core/tests/core_contract.rs) | the trait, effects, composition, determinism | 17 |
| [`recon-sim/tests/simulation.rs`](crates/recon-sim/tests/simulation.rs) | determinism, faults, sessions, the trace | 54 |
| [`recon-protocols/tests/method.rs`](crates/recon-protocols/tests/method.rs) | how a property is asserted so it cannot pass vacuously | 10 |
| `tests/stubborn_link.rs`, `perfect_link.rs`, `session_link.rs` | the links | 13 / 16 / 10 |
| `tests/perfect_failure_detector.rs` | completeness and accuracy, and where accuracy is lost | 13 |
| `tests/best_effort_broadcast.rs`, `reliable_broadcast.rs`, `uniform_reliable_broadcast.rs` | the broadcasts over perfect links | 11 / 15 / 17 |
| `tests/session_best_effort_broadcast.rs`, `session_broadcast.rs` | the broadcasts over session links, and where the two diverge | 6 / 16 |
| `tests/majority_ack_uniform_reliable_broadcast.rs`, `session_majority_ack_…rs` | the same guarantees without a failure detector, and what that changes | 18 / 15 |
| [`tests/flooding_consensus.rs`](crates/recon-protocols/tests/flooding_consensus.rs) | consensus, and what a false suspicion costs it | 21 |

261 in total, all in one process, no ports opened.

## Licence

Apache 2.0. See [`LICENSE-2.0`](LICENSE-2.0).
