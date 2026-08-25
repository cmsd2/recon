# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Distributed message-passing algorithms — broadcast, failure detection, consensus — written in
Rust so that the code reads as the algorithm. `docs/postmortem.md` governs the ordering, and the
ordering is the entire point: the first attempt spent seventeen months on the transport layer,
wrote the connection manager four separate times, and never reached the algorithms.

Three crates, all of them sans-IO:

- **`recon-core`** — the `Protocol` trait, the effect vocabulary, `Cx`, `Time`, error conventions.
- **`recon-sim`** — the deterministic simulator. It *is* the fair-loss network, and it is the
  project's standard of evidence.
- **`recon-protocols`** — the ladder so far: stubborn link, perfect link, best-effort broadcast.

Rungs are transcribed from Cachin, Guerraoui & Rodrigues, with the pseudocode quoted in each
module's documentation and every departure from the page stated and justified there.

## Commands

```bash
./scripts/check.sh                      # everything, and the gate for every commit
cargo test --workspace
cargo test -p recon-protocols --test perfect_link          # one suite
cargo test -p recon-protocols --test method                # the method's own tests
cargo test -p recon-sim -- the_same_seed                   # by name
openspec --version                      # 1.10.0, installed globally via volta
```

Nothing opens a socket and nothing spawns a runtime; the whole suite runs in-process.

In Claude Code, OpenSpec is driven by slash commands (note the colon):

```
/opsx:propose "your idea"   # also: explore, apply, update, sync, archive
```

Project context and per-artifact rules for OpenSpec live in `openspec/config.yaml`.

## Before every commit

Run `./scripts/check.sh`. It must pass in full. Do not commit with anything outstanding —
warnings accumulate into noise, and noise is how a real diagnostic gets missed.

```bash
./scripts/check.sh          # fmt, clippy -D warnings, build, test, project guards
```

Individually, if you need to isolate a failure:

```bash
cargo fmt --all             # formatting is not optional; rustfmt.toml is checked in
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --all-targets
cargo test --workspace
./scripts/check-ordered-maps.sh   # BTreeMap only: HashMap iteration order breaks replay
./scripts/check-error-types.sh    # no io::Error for domain failures
```

Rules, not preferences:

- **Zero compiler warnings.** Fix them; do not `#[allow]` them without a written reason.
- **Zero clippy warnings.** `-D warnings` is the standard, so a lint is a build failure.
- **`cargo fmt` before every commit.** `rustfmt.toml` sets `use_small_heuristics = "Max"`
  so short struct literals stay on one line — the defaults pull effect and message
  constructions apart, which works against code meant to read as the algorithm.
- **Guards are part of the build.** They encode failure modes that are silent at runtime rather
  than loud, which is exactly why they are mechanical checks and not review notes.

Four guards run, and each exists because of a specific way this project has failed or could:

| Guard | Forbids | Because |
|---|---|---|
| `check-ordered-maps.sh` | `HashMap` / `HashSet` in the three crates | iteration order varies per process and silently breaks seed reproducibility |
| `check-error-types.sh` | `io::Error` for domain failures, and the literal `"json decoding error"` | the first attempt flattened seven distinct failures into one string |
| `check-no-transport.sh` | sockets, async runtimes, `.await` | constraint 1 — the failure mode that consumed seventeen months |
| `cargo clippy -D warnings` | any lint | warnings accumulate into noise and hide real diagnostics |

`check-no-transport.sh` is meant to be **deleted deliberately**, in the commit that introduces
transport under constraint 5. Do not weaken it; delete it, or leave it alone.

## The constraints that govern the rewrite

These come from `docs/postmortem.md` §5. They are ordering rules, not style preferences —
each one exists because its absence killed the first attempt.

1. **Algorithms before transport.** No `TcpStream`, no reconnect logic, no multi-process shell
   scripts until several protocols run against an in-memory network in a single test process.
   Work on transport before that point is the documented failure mode repeating.

2. **The protocol core is sans-IO.** A protocol is a synchronous state machine that consumes
   events and emits effects. It never awaits, never reads a clock, never calls `thread_rng`,
   never touches a socket. Time and randomness arrive through the context parameter so they can
   be made virtual and seeded. `quinn-proto`, `rustls` and `raft-rs` are the prior art.

3. **The simulator is the deliverable, not the test harness.** Seeded RNG, virtual clock, a
   priority queue of scheduled deliveries, and knobs for latency, loss, duplication, reordering,
   partition and crash/restart. Correctness is asserted as properties over the delivery trace.
   A failing run must be reproducible from its seed.

4. **Compose statically; extract the DSL, don't design it.** Parents own children as concrete
   typed fields and re-wrap child effects rather than re-encoding them. Write two or three
   protocols by hand before writing any macro to remove the boilerplate. Building the framework
   first is the mistake this repository already made.

5. **Transport last.** When protocols work under simulation, the network layer is a thin
   adapter: best-effort `send`, plus a session/epoch-changed event. Prefer QUIC (`quinn`) over
   TCP-plus-reconnect-logic — it supplies connection identity, multiplexing and framing, which
   removes the need for a hand-rolled multiplexer entirely.

6. **Climb the ladder in order.** Fair-loss link → perfect link → failure detector →
   best-effort broadcast → reliable broadcast → uniform reliable broadcast → consensus. Each
   rung is tested against its stated guarantees before the next begins. Reliable broadcast is
   the milestone that proves the composition model holds.

Errors get `thiserror` types per layer. The string `"json decoding error"` should never appear.

## Conventions this code already follows

- **Ordered maps only** in protocol and simulator state. Enforced; see the table above.
- **`Time` is a newtype over `Duration`**, not `Instant` — an instant cannot be constructed at an
  arbitrary value, and a run must be replayable. `Duration` is fine and is used directly for spans.
- **Composition picks one of two forms**, and which one is decided by a single question: does the
  layer transform its child's indications, or pass them on? Forwarding layers use
  `Cx::with_child`; transforming layers use `Cx::with_child_consuming`, which collects the child's
  indications for the parent to handle after the child call returns.
- **Wire types nest, and are encoded exactly once** at the bottom boundary. No intermediate
  representation is ever materialised — that, not nesting, is what cost the first attempt.
- **A layer that adds no per-hop state adds no wire field.** Three protocols currently share one
  header, the perfect link's message identifier.
- **Departures from the book are documented in the module**, with the pseudocode quoted above the
  implementation so the two can be read against each other.
- **Every property test asserts non-vacuity.** An absence-of-violation property is satisfied by a
  protocol that does nothing; `crates/recon-protocols/tests/method.rs` demonstrates that and
  guards against it.

## Guarantees are conditional — read `docs/conditional-guarantees.md`

The formal companion is `docs/scope-annotated-modules.md`: the extension to the book's module
notation, proved conservative, with the composition rules and the lower bound on what a layer can
bridge.

The ladder's abstractions are idealised: stubborn links retransmit forever, and "perfect link"
assumes a link that never ends. Neither survives contact with a real transport — TCP is a perfect
link *within* a session and a liar across one.

The reframing that governs everything above best-effort broadcast: **every guarantee is bounded
by a scope** — a session, a process incarnation, a cancellation, a deadline — and the end of that
scope is a first-class event on the port, never an implementation detail. A layer bridges a scope
ending only if its redundancy outlives it: memory survives a reconnect, stable storage survives a
restart, other processes survive this one dying. **A layer that cannot bridge must propagate.**
Silently absorbing a scope end is the bug the first attempt shipped.

Two things follow for anyone writing a new rung:

- Layers above the link may depend on its `Cmd` and `Ind` types and nothing else. That is the
  seam a session-aware or logged implementation gets swapped through.
- `Sim::crash` currently preserves state, so it models a pause rather than a crash. Do not rely
  on it to test what a restarted process actually faces.

## Anti-patterns, all of them load-bearing history

Each of these is a real decision from the first attempt, with the consequence it produced:

- **Rewriting the link layer.** It happened four times on three framework bets. If a change
  starts by restructuring connection management, stop and check whether it is actually needed.
- **String-keyed layer composition.** Layers found each other via `multiplex_key: String` and
  `format!("{}/upb", key)`. A typo became a silently undelivered message instead of a compile
  error. Layer boundaries must be typed.
- **Type erasure at every boundary.** Because composition was dynamic, each layer called
  `serde_json::to_value` on the way down and `from_value` on the way up — one message encoded
  and parsed three times. Serialize once, at the wire.
- **`io::Error` for everything.** Domain failures were flattened to
  `io::Error::new(ErrorKind::Other, "...")`, discarding the real cause and making failures in a
  running cluster indistinguishable.
- **Welding algorithms to the runtime.** Protocols held a reactor handle, a real timer and a
  thread RNG, so nothing could be run twice the same way and nothing could be tested without
  opening ports. This is what constraint 2 exists to prevent.

## Reference material

The previous implementation is checked out as detached worktrees alongside this repo. Read
these as notes when reimplementing — **do not port them**; the post-mortem documents four
concrete bugs in the gossip code that would come with.

| Path | Ref | Contents |
|---|---|---|
| `../recon-ref/master` | `373a7b1` | `archive/recon-gossip/` (`upb.rs`, `lpb.rs` — the only algorithm code ever written) plus the three crates |
| `../recon-ref/link` | `090fd89` | Link v2 — channel-based `link.rs`, `transport.rs`, `pub_sub.rs` |
| `../recon-ref/actix` | `8ad6fa9` | Link v3 — actix `tcp_server.rs`, `connection_manager.rs`, `connection_table.rs` |

`upb.rs` and `lpb.rs` are transcriptions of Cachin, Guerraoui & Rodrigues algorithms 3.9 and
3.10. Their value is that the translation from pseudo-code to concrete data structures — sequence
tracking, pending maps, timeout skip-over — has already been done once.

Two ideas from the old `recon-link/src/conn.rs` are worth carrying forward, as ideas rather than
code: the **session-epoch contract** (every packet tagged with a session id that increments on
reconnect, with a control message announcing the new epoch, so the layer above is told that a
suffix may have been lost instead of pretending reconnection is invisible), and the
**stale-write reconnect** (if the transport refuses a write past a deadline, tear the connection
down rather than wait — a half-open-TCP defence).

## Vintage note

Anything read from the reference worktrees is futures 0.1 / tokio-core 0.1 on Rust edition 2015 —
`Poll`, `Async`, `AsyncSink`, `try_ready!`, `extern crate`, `Box<Future<...>>` without `dyn`.
That style is history, not a model. New code targets a current edition with `async`/`await`
available at the edges, while the protocol cores stay synchronous per constraint 2.
