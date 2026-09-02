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
- **`recon-protocols`** — the abstractions built so far: stubborn link, perfect link, best-effort broadcast.

Abstractions are transcribed from Cachin, Guerraoui & Rodrigues, with the pseudocode quoted in each
module's documentation and every departure from the page stated and justified there.

`README.md` is the map — the crates, the protocols as they currently stand, the documents and what
each says, the prior art, and how to build, test and add an abstraction. It is the front door for anyone
who has not read this file, and it is **kept current as part of the work**; see below.

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

## Keep `README.md` current

`README.md` is a map of what is actually here, and a map that lags is worse than none — it sends
a reader to an abstraction that does not exist, or omits the one they wanted. No guard can check it, so
it is a standing obligation rather than a mechanical one: **update it in the same commit as the
change that dates it**, not in a sweep afterwards.

What dates it, and what to do:

| Change | Update |
|---|---|
| A new protocol module | Add a row to the right protocol table — module, book reference, status, space bound — and say in the prose what guarantee it adds. Its status and space must match what the module's own documentation claims. |
| An abstraction converted from transcription to implementation | Change its status and space in the table. The distinction is the whole point of `docs/bounded-space.md`; a stale table quietly asserts the opposite of what is true. |
| A new test suite, or a suite that changes size materially | The suite table at the end, and the total. `cargo test --workspace` prints the counts. |
| A new document in `docs/` | A row in the documentation table saying what it *says*, not what it is called. |
| A new capability spec, or a new top-level directory under `openspec/specs/` | The specification tree. |
| A guard added, or `check-no-transport.sh` deleted under constraint 5 | The guard table, and the surrounding prose if a constraint has been discharged. |
| An `examples/` directory, once one exists | Replace the placeholder section, which currently says there are none and why. |
| A new crate, or a change to how the project is built or run | The crate table and the getting-started commands. Verify the commands by running them. |

Check the relative links still resolve when files move. The README claims specific test counts and
specific statuses; if you cannot verify a claim you are about to write, do not write it.

The same-commit rule is not only the README's. A module's quoted pseudocode, its departures
list, and any document in `docs/` that a change dates are updated **in the commit that dates
them** — this repository's method is reading code against its quoted contract, so a stale quote
asserts an algorithm the code deliberately does not implement. When a test forces a change to a
clause, the quote changes in that commit. (This has already happened once: a docstring kept
quoting the deadlocking variant of a resend clause that its own inline comment said a test had
replaced.)

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
   typed fields and re-wrap the child effects that carry the child's vocabulary — messages and
   indications — rather than re-encoding them. A timer is not one of those: it is named by an
   opaque `TimerId` the driver issues, so it passes through composition untouched. Write two or three
   protocols by hand before writing any macro to remove the boilerplate. Building the framework
   first is the mistake this repository already made.

5. **Transport last.** When protocols work under simulation, the network layer is a thin
   adapter: best-effort `send`, plus a session/epoch-changed event. Prefer QUIC (`quinn`) over
   TCP-plus-reconnect-logic — it supplies connection identity, multiplexing and framing, which
   removes the need for a hand-rolled multiplexer entirely.

6. **Build the abstractions in order.** Fair-loss link → perfect link → failure detector →
   best-effort broadcast → reliable broadcast → uniform reliable broadcast → consensus. Each
   abstraction is tested against its stated guarantees before the next begins. Reliable broadcast is
   the milestone that proves the composition model holds.

Errors get `thiserror` types per layer. The string `"json decoding error"` should never appear.

## Conventions this code already follows

- **Ordered maps only** in protocol and simulator state. Enforced; see the table above.
- **`Time` is a newtype over `Duration`**, not `Instant` — an instant cannot be constructed at an
  arbitrary value, and a run must be replayable. `Duration` is fine and is used directly for spans.
- **Composition picks one of two forms**, and which one is decided by a single question: does the
  layer transform its child's indications, or pass them on? Forwarding layers use
  `Cx::with_child`; transforming layers hold the child as a `recon_core::Child<P>` and call
  `child.run(cx, wrap, f)`, which performs `Cx::with_child_consuming` and hands back the child's
  indications for the parent to handle — then `child.reclaim(inds)` returns the buffer. A parent
  handles indications with `&mut self`, which is why the inbox comes back by value rather than as
  a borrow. Neither form takes a timer mapper: a timer has nothing in it belonging to one layer.
  Constraint 4 said to write this by hand two or three times before extracting it; `Child` was
  extracted at sixteen.
- **A durable child is composed through a `Slot`.** Both forms above hand the child a `NoStore`,
  which is right whenever the child keeps nothing. A parent that keeps durable state *and* composes
  a child that does uses `Cx::with_durable_child_consuming`, passing a `recon_core::Slot` naming
  where the child's record lives inside the parent's. The child's write becomes a read-modify-write
  of the parent's whole record — **one write, not two**, because two writes have an interval between
  them and a crash lands in intervals. Only the metadata is scoped: a child that *appends* cannot be
  composed, and the signature enforces that rather than a comment. `Slot`'s documentation says what
  the sequence half would look like, and why it is not built.
- **A timer is a handle, not a type.** `Cx::set_timer` returns a `TimerId` the driver issued, and
  the same handle comes back to `on_timer`. So a layer that registers no timer declares nothing
  about timers, and inserting a layer leaves the timers beneath it alone. The price is that the
  handle carries no routing: an expiry is offered to *every* layer, each composing layer passes it
  to each of its children, and **a layer that registered a timer must compare before acting** —
  `if self.tick != Some(id) { return; }`. Nothing in the type system enforces that; a test in
  `crates/recon-protocols/tests/flooding_consensus.rs` does. Identities must come from one source
  per run, or two layers get the same handle and each accepts the other's expiry: `Sim` owns one,
  and a test driving a stack by hand uses `step_with` rather than `step` for the same reason.
- **Wire types nest, and are encoded exactly once** at the bottom boundary. No intermediate
  representation is ever materialised — that, not nesting, is what cost the first attempt.
- **A layer that adds no per-hop state adds no wire field.** Three protocols currently share one
  header, the perfect link's message identifier.
- **Departures from the book are documented in the module**, with the pseudocode quoted above the
  implementation so the two can be read against each other.
- **Every property test asserts non-vacuity.** An absence-of-violation property is satisfied by a
  protocol that does nothing; `crates/recon-protocols/tests/method.rs` demonstrates that and
  guards against it.
- **Identity is as durable as the state it keys.** An identifier that crosses the wire or lands
  in storage outlives the handler that minted it, so its generator is state with a scope: say
  which scope it survives — incarnation, session, forever — and persist it or re-derive it in
  `on_recovery`. A durable set keyed by a volatile counter is the specific bug to look for; the
  2026-08 audit found it three times, always downstream of the documented id-keyed departure
  from the book's content-keyed identity. A departure's obligations are part of the departure.
- **Durable-before-visible holds in code order.** The write precedes the emission of any effect
  that reveals it, in the handler's own text — never by relying on the driver to buffer effects
  until the handler returns. `Cx` explicitly supports eager sinks.
- **Sequence tests by event, not by duration.** `Sim::command` schedules; it does not run. To
  act on a state such as "sent but not yet delivered", call `Sim::step_now()` — everything due at
  the current instant is dispatched and the clock stays put — rather than `run_for` with a
  duration guessed shorter than the latency. To search for a state one event creates and the
  next may destroy — "exactly one process has decided" — loop on `Sim::step()`, one event at a
  time, rather than `run_for(1 ms)`, which can hold two. A test that depends on a duration being
  short is a test that depends on the latency configuration, silently. No suite uses the old
  idiom for sequencing any more; a duration is still right for "any later instant", which is a
  different thing and says so where it occurs.
- **A fault knob nobody spends is a claim nobody tested.** When the simulator gains a fault —
  `crash_on_next_write`, suspension, session breaks — every protocol whose stated guarantee that
  fault threatens gets a test injecting it, in that protocol's own suite, not only in the
  simulator's. And recovery tests include the process doing something *new* after recovering,
  not only resuming old work: resuming exercises replay, new work exercises what replay forgot
  to restore.
- **Failure analyses cover both roles.** Document and test both the process a failure happens to
  and the processes observing it — an accusation has an accuser and an accused, a stall has a
  staller and its peers. The audits found analyses that covered exactly one side, and the bug
  was on the other.
- **A redundancy claim is tested with the other redundancies removed.** A restarted process can be
  rebuilt by its own storage, by its peers, or by the retransmission backlog in flight at the crash
  instant — a same-instant `crash` then `restart` leaves that backlog intact, and the stubborn
  children keep a full replayable copy of the run in it. A durability test that leaves the network
  up cannot say which mechanism it proved: the fail-recovery total-order broadcast shipped with no
  `on_recovery` at all and its restart suite passed. Crash, drain the backlog against the dead
  process, partition, then restart; only then does surviving state mean storage.
- **A non-vacuity check has a place in the sequence, not only a presence in the test.** Asserting
  `deaths_in_writes() > 0` after crash-and-restart once counted a death that happened *after* the
  recovery it was meant to justify. Assert the fault occurred before the step that depends on it.
- **A protocol constructed at run time is owed exactly one of `on_init` and `on_recovery` before
  its first event.** The book's "Initialize a new instance" is an event its runtime delivers;
  creating a child with `or_insert_with` and handing it a message skips it, and what goes missing
  is whatever `on_init` would have started — detector timers, the first durable write. Nothing
  fails until a fault needs the detector, which is the fault-knob rule's corollary: every composed
  detector gets at least one test in which it must act. Both total-order members had this bug, and
  the shared crash property is what caught it.
- **A quiet window proves quiescence only if the step budget was not the reason.** `Sim::run_for`
  returns normally once `max_steps` is spent, so a test's later phases can silently do nothing — a
  post-recovery append once vanished this way. Raise the budget where a suite needs the room, and
  treat an unexplained quiet window as suspect before treating it as settled.
- **Assert the property, not a correlate.** "Appends outnumber rewrites" stood in for "the growing
  halves are appended" and broke the moment detectors legitimately began rewriting their bounded
  records. When the direct property is enforced by the types or invisible to the trace, say so in
  the test and assert the minimal implication — a non-vacuity floor — rather than a ratio that
  encodes an assumption about everything else's write cadence.

## The real-world set

Most modules here are the book's algorithms, transcribed faithfully and tested against their stated
guarantees. A **small number** are maintained as useful in the real world, and those carry a second
obligation. Membership, as of 2026-09:

- **In**: the gossip pair, `probabilistic_broadcast` and `lazy_probabilistic_broadcast`. Multi-Paxos,
  when it is written.
- **Out**: single-instance Paxos and everything beneath it that exists to build it — epoch-change,
  epoch consensus, the logged variants. They are the book's stepping stones and are kept faithful
  to the page, not tuned. Stubborn links are academic; nothing in the real-world set runs over one.

What "in" obliges, beyond the usual correctness suite:

- **Over session links**, not stubborn ones. A deployment's transport already retransmits; the
  deployable form of an algorithm is the one that runs over `session_link` and handles the scope
  events it raises.
- **Checked for resource use.** Minimal messages for the work done — a test that counts them
  against what the algorithm needs, not just that it terminates — and no growth of state or of send
  rate with how long the run has been going (`tests/common::assert_send_rate_flat!`).

A module is moved into the set by a change with a proposal, because the second obligation usually
means a departure from the book, and departures belong in a specification.

## Transcriptions vs implementations — read `docs/bounded-space.md`

Two kinds of protocol live here, and which one a module is must be **stated in the module**, not
assumed. A **transcription** renders an algorithm from the book faithfully enough to read against
the page, and inherits the book's omissions — which explicitly include garbage collection. An
**implementation** holds the same guarantees while consuming resources bounded by something other
than how long it has been running.

**The rule: state is bounded by membership, by a window, or by a configured capacity — never by
the number of messages handled.** The same applies to *work*: a periodic task whose cost is
proportional to everything ever sent is unbounded even if each item is small. That is the failure
mode that hides, and this repository currently has it — the stubborn link re-sends everything it
has ever sent on every tick, because nothing calls its `Stop`.

The **broadcast** family above the failure detector is still a transcription and still violates the
rule. The leader-driven family does not: Ω, epoch-change, both epoch consensuses and Paxos are
bounded by membership, and the two logged ones keep one rewritten value each. What they inherit is
the stubborn children's outstanding set, because nothing calls `Stop` — bounded in practice by the
epoch ending, except in `logged_epoch_change`, which has no ending and says so. The audit, the
measurements, and the mechanisms that fix each are in `docs/bounded-space.md`.

Three practices follow:

- **State the space bound in the module documentation**, beside its departures from the book:
  bounded by membership, bounded by a window, or unbounded and therefore a transcription.
- **An implementation carries a test that its state does not grow with messages handled** — run a
  growing count, assert the bound holds — **and one that its send rate does not grow with time**:
  `tests/common::assert_send_rate_flat!` runs successive windows after the work is done and requires
  the last no higher than the first. The second exists because the first was not enough: seven
  modules with bounded state shipped with a send rate growing linearly in time, and nothing noticed
  until it was measured. Same shape as the non-vacuity guards: assert the property that would
  otherwise stop holding in silence.
- **Converting a transcription is a change with a proposal**, not a cleanup commit. Bounding
  usually weakens a guarantee to a scope — "no duplication *within the retention window*" — and
  that belongs in a specification.

## Guarantees are conditional — read `docs/conditional-guarantees.md`

The formal companion is `docs/scope-annotated-modules.md`: the extension to the book's module
notation, proved conservative, with the composition rules and the lower bound on what a layer can
bridge.

The book's abstractions are idealised: stubborn links retransmit forever, and "perfect link"
assumes a link that never ends. Neither survives contact with a real transport — TCP is a perfect
link *within* a session and a liar across one.

The reframing that governs everything above best-effort broadcast: **every guarantee is bounded
by a scope** — a session, a process incarnation, a cancellation, a deadline — and the end of that
scope is a first-class event on the port, never an implementation detail. A layer bridges a scope
ending only if its redundancy outlives it: memory survives a reconnect, stable storage survives a
restart, other processes survive this one dying. **A layer that cannot bridge must propagate.**
Silently absorbing a scope end is the bug the first attempt shipped.

Three things follow for anyone writing a new abstraction:

- Layers above the link name a **port**, not an implementation. `recon_protocols::link::Link` is
  what a layer above may depend on and the whole of what it may: build a send, and classify an
  indication. A link keeps its own `Cmd` and `Ind`. A scope boundary reaches the layer above only
  from a link that can observe one — the session link classifies its endings as boundaries, the
  perfect link has none to classify — and each layer then states in its own indications whether it
  bridges or propagates. Every composing layer takes its child as a type parameter with a default,
  so `BestEffortBroadcast<P>` still means today's stack. This is why there are no longer four
  `session_*` broadcast modules. A `ScopedLink` bound enforcing the same thing was tried and
  deleted for want of a consumer; `link.rs` says why, before you reach for it again.
- `Sim::crash` rebuilds the protocol from its constructor, so a crash genuinely loses volatile
  state and `crash` then `restart` is amnesia, not a pause. `Sim::suspend` is the pause, and
  `Sim::resume` ends it — a *stall*, in which timers, deliveries and scope events are held rather
  than dropped and no startup branch re-runs. The two are not interchangeable and each rejects
  the other's process. What survives a crash is what was written through `Cx::storage` — a
  synchronous `Store` with one rewritten metadata value and an appended entry sequence — and is
  read back in `on_recovery`; `Sim::crash_on_next_write` models dying inside the write, with the
  seed deciding whether it landed.
- **The simulator is subject to the same invariants as the layers.** A new simulator capability
  is checked against `docs/conditional-guarantees.md` before it merges, with one question: can it
  lose something without raising the event that says so? Silently absorbing a scope end is the
  cardinal sin wherever it lives, and the sim is where it most recently appeared — suspension
  dropping in-session deliveries while the session stayed up, with no `SessionEnded` ever
  raised. That one is fixed: a suspension now holds what it cannot deliver. What a stall still
  takes is the clock, which no amount of holding gives back — see the failure detector's note on
  the accusing side.

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
