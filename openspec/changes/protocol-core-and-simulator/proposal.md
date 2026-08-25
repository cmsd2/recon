## Why

This repository is empty on purpose. A previous attempt spent seventeen months on the transport
layer, rewrote the connection manager four times across three framework bets, and never reached
the distributed algorithms it existed to write (`docs/postmortem.md`). The ordering was the
failure: infrastructure first, algorithms never.

This change inverts that ordering and proves the inversion works. It delivers the smallest thing
that is simultaneously a working protocol stack and a working method: three composed protocols
running against a deterministic in-memory network, with their guarantees asserted as properties
over the delivery trace. No sockets are opened.

Two constraints from the post-mortem make this the *only* sensible first change. Constraint 1
forbids transport work until several protocols run under simulation. Constraint 3 makes the
simulator the deliverable rather than a test harness. A simulator with no protocols would be
speculative infrastructure — the documented failure mode — so the simulator and its first
protocols must land together.

## What Changes

- **New Cargo workspace.** The tree currently has no Rust code at all; this establishes it.
- **A sans-IO protocol core.** A `Protocol` is a synchronous state machine consuming events and
  emitting effects. It never awaits, never reads a clock, never calls `thread_rng`, never touches
  a socket. Time and randomness arrive through a context parameter so both can be made virtual
  and seeded.
- **A deterministic simulator.** Seeded RNG, virtual clock, and a priority queue of scheduled
  deliveries, with knobs for latency, loss, duplication, reordering and partition. It *is* the
  fair-loss link layer — that rung of the ladder is provided by the network model, not
  implemented as a protocol.
- **The first three rungs of the ladder**, composed statically: stubborn link, perfect link over
  it, best-effort broadcast over that.
- **Property assertions over the delivery trace**, replacing log-reading as the verification
  method. A failing run is reproducible from its seed.
- **Per-layer error types** via `thiserror`. The string `"json decoding error"` does not appear.

Explicitly **not** in this change, each deferred by a stated constraint:

- No `TcpStream`, `quinn`, or `tokio` in any form (constraints 1 and 5).
- No macro or DSL to remove composition boilerplate. Two or three protocols get written by hand
  first, so the repetition can be measured rather than guessed at (constraint 4).
- No failure detector, reliable broadcast, uniform reliable broadcast, or consensus. Those are
  later rungs and later changes (constraint 6).
- No TLA+ specification. Deferred to the consensus rung, where exhaustive checking earns its cost.

## Capabilities

### New Capabilities

- `protocol-core`: The synchronous protocol abstraction — event handlers, the effect vocabulary,
  the context through which time and randomness are injected, and the rules governing how a
  parent composes a child protocol and re-wraps its effects.
- `simulation`: The deterministic execution environment — virtual clock, seeded randomness,
  scheduled delivery queue, fair-loss network semantics with configurable fault injection, the
  delivery trace, and the reproducibility guarantee that a seed fully determines a run.
- `links/stubborn-link`: Retransmission over a fair-loss network, giving eventual delivery to a
  correct receiver at the cost of unbounded duplication.
- `links/perfect-link`: Reliable delivery with no duplication and no creation, built by adding
  message-identifier deduplication over the stubborn link.
- `broadcast/best-effort-broadcast`: Fan-out to all processes over perfect links, guaranteeing
  validity when the sender is correct, with no duplication and no creation.

### Modified Capabilities

None. No specs exist yet.

## Impact

- **Code.** All new. The workspace, the core crate, the simulator, and the three protocols.
  Nothing is ported from the reference worktrees at `../recon-ref/`; the archived `upb.rs` and
  `lpb.rs` are read as notes only.
- **Dependencies.** A seeded RNG (`rand` plus an explicit reproducible generator such as
  `rand_chacha`), `serde` with a binary codec for the wire boundary, and `thiserror`. No async
  runtime, no networking crates.
- **Public API.** Establishes the shapes every later rung inherits: the `Protocol` trait, the
  effect enum, the context, and the wire-nesting convention. These are the decisions most
  expensive to change later, which is why `design.md` accompanies this proposal rather than
  following it.
- **Verification method.** Introduces trace-property assertions and seed-based reproduction as
  the project's standard of evidence, replacing the previous attempt's nine-processes-and-a-log-file
  approach.
