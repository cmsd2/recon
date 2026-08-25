# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status: the tree is empty on purpose

There is no Rust code in this repository right now. The `clean-slate` branch deliberately
removed the previous implementation (three crates plus an archived gossip prototype) rather
than porting it. `docs/postmortem.md` is the governing document for what comes next — read it
before proposing architecture, because the *ordering* it prescribes is the entire point of the
restart.

The short version: this project is for writing distributed message-passing algorithms —
broadcast, failure detection, consensus — in Rust, in a form where the code reads as the
algorithm. The first attempt spent seventeen months on the transport layer, wrote the
connection manager four separate times, and never reached the algorithms.

## Commands

There is no Cargo project yet, so there is nothing to build or test. Do not invent
`cargo` invocations for crates that do not exist; check `ls` and `git ls-files` first.

What does work today:

```bash
openspec --version          # 1.10.0, installed globally via volta
```

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
- **Guards are part of the build.** They encode two failure modes that are silent at runtime
  rather than loud, which is exactly why they are mechanical checks and not review notes.

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
