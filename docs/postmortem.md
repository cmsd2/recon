# Four Links, One Algorithm

A post-mortem of `cmsd2/recon` — January 2018 to May 2019.
42 commits, 3 branches, no test beyond `2 + 2 == 4`.

> Designed version: <https://claude.ai/code/artifact/b8bdaf17-bc85-464f-8b0c-b8aa28efdddf>

You didn't fail to build the DSL. You never got back to it. The repository contains four
independent implementations of the transport layer and exactly one implementation of the thing
the transport was *for* — and that one was written before the repository existed.

---

## 1. The core sample

Read as stratigraphy, the git history is unambiguous. Every layer of deposit is a connection
manager. Each one has a peer table, an outbound queue, an inbound queue, a bounded-buffer
config, and a reconnect policy. Each one was written from scratch on a different framework bet.

| Date | Deposit | Lines |
|---|---|---|
| 2019-05-31 | dependency bump — no code change | — |
| 2018-10-31 → 11-04 | `recon-actix/src/tcp_server.rs` · actix actors | 317 |
| 2018-10-26 | `recon-link/src/conn.rs` — "fix deprecations" | — |
| 2018-01-10 → 01-12 | `recon-link/src/link.rs` + `recon-util/pub_sub.rs` · channels | 565 |
| 2018-01-06 → 01-09 | `recon-link/src/conn.rs` · `state_machine_future` | 400 |
| pre-2018, imported 01-07 | `archive/` · `upb.rs` + `lpb.rs` — **the algorithms** | 574 |

- **Link v3** (actix branch): connection table, lifecycle events, retry-forever. Five days, then stopped.
- **Link v2** (`link` branch): "switch from sink to channel + poll." Never merged. Three days.
- **Link v1** (`master`): the only thing on master. Four days — and the best of the four.
- **The algorithms**: committed once as "import original gossip projects for posterity," never
  edited again. Also present: `link.rs`, `multiplex.rs`, `switchboard.rs` — link v0.

Roughly twelve working days of activity spread over seventeen months. Eleven of them on
transport. The gossip protocols — the reason the project existed — received none.

The v2 rewrite is the tell. It began four days after v1 shipped a working reconnecting link, and
it rebuilt the same abstraction with the same field names — `outbound`, `inbound`, `inbound_max`,
`outbound_max`, a peer map, a config struct — over channels instead of a state machine. Nothing
above it changed. Nothing above it existed.

---

## 2. Diagnosis: two causes, and only one of them was yours

### The part that was 2018

Async/await didn't stabilise until November 2019. You were writing hand-rolled `Poll` state
machines because you had no alternative — `state_machine_future` exists precisely because a
borrow-checked state machine could not be written by hand at the time. `tokio-proto`, which the
original README builds on, was deprecated during the project's life. `tokio-core` was superseded.
The commit log reads "fix non-terminating loop," "fix deprecations," "update deps. fix
compilation," "upgrade deps" — that is the sound of a foundation moving faster than the building.

A large share of the effort went into problems that have since been deleted from the language.
That is not a failure of persistence.

### The part that was design

Three self-inflicted decisions compounded, and all three trace back to one root: the layers were
composed *dynamically*, by string, at runtime.

- **Stringly-typed layering.** Layers found each other through `multiplex_key: String`,
  subscribing by name — `format!("{}/upb", multiplex_key)`. A typo is a silently undelivered
  message, not a compile error.
- **JSON inside JSON inside JSON.** Because the boundary was dynamic, every layer had to erase
  its type: `serde_json::to_value` on the way down, `from_value` on the way up. One gossip
  message crossing lpb → upb → multiplex → link is encoded three times and parsed three times.
- **Everything is `io::Error`.** A serde failure becomes
  `io::Error::new(ErrorKind::Other, "json decoding error")`, discarding the actual error. Seven
  call sites produce that exact string, among nineteen uses of `ErrorKind::Other`. A decode
  failure in a running cluster is indistinguishable from any other decode failure.

And underneath all of it, the decision that made the project unrecoverable: the algorithms were
welded to `tokio_core::reactor::Handle`, `Timer::default()`, `rand::thread_rng()` and real
sockets. There was no way to run a protocol without opening ports, and no way to run it twice
the same way.

> **The actual gap.** `gossip.sh` launches nine processes on ports 9000–9008 and tails one log
> file. That was the entire verification strategy for a probabilistic broadcast protocol with
> loss, timeouts, and randomised fan-out. The missing piece was never the DSL. It was the ability
> to say "run this again, exactly, with seed 42 and 30% loss."

---

## 3. Evidence: what reading the code proves, and what it doesn't

An earlier draft of this document listed four bugs in the gossip code, found by reading it against
remembered pseudocode. Three of the four were wrong. They are recorded here rather than quietly
deleted, because how they failed is the most useful evidence in the document.

Checked against Cachin, Guerraoui & Rodrigues, *Introduction to Reliable and Secure Distributed
Programming*, 2nd ed., §3.8, pages 95–100:

### Not a bug — `lpb.rs:158`, the α probability

Claimed: the field is documented `// probability of storing message` but stores with probability
*1 − α*, so α is inverted.

Algorithm 3.10 (page 98) prints:

```
upon event ⟨ upb, Deliver | p, [DATA, s, m, sn] ⟩ do
    if random([0, 1]) > α then
        stored := stored ∪ {[DATA, s, m, sn]};
```

`lpb.rs` is character-for-character the book. What is genuinely odd belongs to the source, not the
code: the book's *prose* on page 99 says a process "stores a copy of the message with probability
α," which contradicts its own pseudocode. Page 100 breaks the tie — "all of them were to store it
(by setting α = 0)" — and store-always at α = 0 only holds under *1 − α*. So the pseudocode is
right and the prose is loose. `lpb.rs` copied the pseudocode into its code and the prose into its
comment, faithfully reproducing the book's own inconsistency.

### Not a bug — `upb.rs:111–115`, relay before dedup

Claimed: relaying before checking `delivered` re-gossips duplicates and compounds traffic.

Algorithm 3.9 (page 95) places the relay *outside* the dedup guard, at the same level:

```
upon event ⟨ fll, Deliver | p, [GOSSIP, s, m, r] ⟩ do
    if m ∉ delivered then
        delivered := delivered ∪ {m};
        trigger ⟨ pb, Deliver | s, m ⟩;
    if r > 1 then gossip([GOSSIP, s, m, r − 1]);
```

The book names the consequence on the same page: "the algorithm induces a significant amount of
redundancy in the message exchanges: any given process may receive the same message many times."
The redundancy is analysed, not accidental. The TTL encoding differs harmlessly — the book starts
at `R` and stops at `r = 1`, the code starts at `rounds - 1` and stops at `ttl = 0`, giving the
same number of relay hops.

### Not a bug — `lpb.rs:170–179`, repeated retransmit requests

Claimed: nothing records that a request was already sent, so gaps are re-requested on every
out-of-order arrival.

Algorithm 3.10 (page 98) does exactly this, guarded only by `pending`:

```
else if sn > next[s] then
    pending := pending ∪ {[DATA, s, m, sn]};
    forall missing ∈ [next[s], . . . , sn − 1] do
        if no m′ exists such that [DATA, s, m′, missing] ∈ pending then
            gossip([REQUEST, self, s, missing, R − 1]);
    starttimer(∆, s, sn);
```

### Stands — `upb.rs:223`, garbage collection in the hot path

`delivered_gc()` runs on *every* `poll()`, draining and rebuilding the entire delivered-set to
expire entries by wall-clock age. Poll runs on every event, so the cost of receiving one message
is linear in everything ever received.

This one is the implementation's own: page 100 says "garbage collection of the stored message
copies is omitted in the pseudo code for simplicity," so there was no spec to follow and the
chosen strategy is genuinely expensive.

### Stands — the futures-0.1 defects

Unrelated to the algorithms. `examples/gossip.rs:103` calls `self.poll()` recursively from inside
`poll`; `multiplex.rs:164` and `App::poll` both return `Ok(Async::NotReady)` unconditionally at
the end, the classic lost-wakeup shape; `lpb.rs:189` unwraps a map lookup inside a message
handler.

### What this episode actually demonstrates

The original claim was that the project could not find its own bugs. The stronger claim, which
these corrections earn, is this: **nobody can settle these questions by reading — including a
careful reader with the book open.** Three confident false positives came out of exactly the
method the project had available to it, and they were only resolved by going to the source
line by line.

Every one of them would have been settled in seconds by a simulator asserting properties over the
delivery trace: messages per broadcast against the analytical bound, store rate against α,
requests per gap. Not because such a simulator is clever, but because it answers questions that
reading cannot answer at all. That is the argument of §5.3, and it applies to the reader of the
code as much as to its author.

---

## 4. Salvage: what survives, and in what form

Three ideas are worth carrying forward. Zero lines of code are. Everything here depends on
futures 0.1, `tokio-core`, `tokio-proto`, or `state_machine_future` — and a port would preserve
the shapes those libraries forced on you, which is exactly what you want to lose.

### Keep the idea — the session-epoch link contract

`recon-link/src/conn.rs` — `Message::Control` / `Event::Connected`

The genuinely good idea in the repository, and the one piece of real distributed-systems thinking
in it. Every packet is tagged with a session id; the id increments on every reconnect; a control
message announces the new epoch to the layer above. The contract is honest — *within a session,
FIFO; across a session boundary, an unknown suffix was lost* — where almost every
reconnecting-TCP wrapper pretends reconnection is invisible. It is the right primitive to build
perfect links and failure detectors on.

### Keep the idea — stale-write reconnect

`recon-link/src/conn.rs:287–314` — `outbound_max_age`

If the transport refuses a write for longer than a deadline, tear the connection down rather than
wait. That is a half-open-TCP defence learned from operating things, not from a textbook, and it
belongs in the new link.

### Keep the idea — the `NewTransport` factory

`recon-link/src/conn.rs:94–100`

The right shape — abstract transport construction so it can be swapped. It was never used for
that: one implementation, TCP, which is why nothing could be tested. Twelve lines. Rewrite it,
and this time write the in-memory implementation first.

### Read closely — `upb.rs` and `lpb.rs`

`archive/recon-gossip/src/` — 574 lines

Better reference material than the first draft of this document credited. Verified against the
book in §3: these are faithful transcriptions of algorithms 3.9, 3.10 and 3.11, with the awkward
parts — sequence tracking, pending maps, timeout skip-over, TTL encoding — already worked out
correctly. The translation from pseudo-code to concrete data structures has been done once and
it holds up.

What should not come across is the futures-0.1 plumbing they are embedded in: the `Stream` impls,
the manual timer vectors, the `serde_json::Value` boundaries, the `io::Error` returns. Take the
algorithm logic; leave the scaffolding.

### Discard — `multiplex.rs`, `switchboard.rs`, `pub_sub.rs`, `tcp_server.rs`, `recon-service`

`archive/` + `link` and `actix` branches — ~1,100 lines

String-keyed dynamic dispatch, subscription registries, and an unused trait module. The
multiplexer's job is done by QUIC streams. The pub/sub registry's job is done by owning a struct.
`recon-service` is three traits nothing implements, including one `impl Future for` a bare trait
object that would not compile in a modern edition.

---

## 5. The restart: six decisions, in this order

The order is the whole prescription. Last time the network came first and the DSL never arrived;
the fix is not more discipline applied to the same sequence, it is a different sequence.

### 1. Don't open a socket until five protocols work

No `TcpStream`, no reconnect logic, no `gossip.sh`. The first milestone is several protocols
running against an in-memory network inside a single test process. Every hour spent on transport
before that point is the failure mode repeating — and the history shows it repeating four times.

### 2. Make the core sans-IO: no clock, no sockets, no executor

A protocol is a synchronous state machine that consumes events and emits effects. It never
awaits, never reads a clock, never calls `thread_rng`.

```rust
pub trait Protocol {
    type Cmd;   // requests from the layer above
    type Ind;   // indications to the layer above
    type Msg;   // what crosses the wire

    fn on_cmd(&mut self, cmd: Self::Cmd, cx: &mut Cx<Self>);
    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut Cx<Self>);
    fn on_timer(&mut self, t: TimerId, cx: &mut Cx<Self>);
}

// the only way to affect the world
impl<P: Protocol> Cx<P> {
    fn send(&mut self, to: NodeId, msg: P::Msg);
    fn indicate(&mut self, ind: P::Ind);
    fn timer(&mut self, after: Duration) -> TimerId;
    fn now(&self) -> Instant;          // virtual under simulation
    fn rng(&mut self) -> &mut impl Rng; // seeded under simulation
}
```

This one change fixes most of section 2 at a stroke. Time and randomness become injectable, so
runs become reproducible. Handlers become plain functions, so they are unit-testable without a
runtime. And the code starts to read as the algorithm:

```rust
// lpb.rs:158, restated — deterministic, and right way round
if cx.rng().gen_bool(self.alpha) {
    self.stored.insert(id, msg.clone());
}
```

This is the sans-IO pattern, and it is well-established in Rust now — `quinn-proto`, `rustls` and
`raft-rs` all separate protocol state from I/O this way. There is prior art to copy from.

### 3. The simulator is the product

Not a test harness — the deliverable. A seeded RNG, a virtual clock, and a priority queue of
`(deliver_at, from, to, msg)`, with knobs for latency distribution, drop rate, duplication,
reordering, partition, and crash/restart. A whole cluster runs in one thread with no sockets,
faster than real time.

Then assert properties over the delivery trace rather than reading logs. For lazy probabilistic
broadcast: per-sender FIFO on delivered messages; no message delivered twice; with zero loss,
everything is eventually delivered; a gap is skipped only after δ has actually elapsed; and total
messages per broadcast stays under a bound. Every question §3 had to settle by hand — does the
relay compound, is the store rate α or 1−α, how many requests does one gap produce — is one of
these assertions, answered on the first run instead of by argument.

Because the schedule is a seed, a failing run is a number you can replay and hand to a shrinker.
Worth surveying before you build: `turmoil`, `madsim` and `stateright` all occupy this space —
the last is a model checker with an actor model, which is the closest thing in Rust to what a
teaching framework gives you. *Check their current state; this survey has a knowledge cutoff.*

### 4. Compose statically — and extract the DSL, don't design it

Kompics — the framework from the KTH course, and the reason this project started — can wire an
arbitrary runtime graph of components because the JVM erases types and boxes everything. Reproducing that dynamism in Rust means trait objects
plus `Any` downcasting, which is `multiplex_key: String` wearing a better hat. That instinct is
what produced the JSON-in-JSON.

Rust rewards a static tree. A parent owns its children as concrete fields, runs them into an
effect buffer, and decides what each effect means:

```rust
struct Lpb<T> { upb: Upb<LpbMsg>, stored: .., pending: .. }

// child emits into its own buffer; parent re-wraps, never re-encodes
self.upb.on_msg(from, m, &mut child);
for eff in child.drain() {
    match eff {
        Effect::Send(to, m) => cx.send(to, LpbWire::Upb(m)),
        Effect::Indicate(i) => self.on_upb_deliver(i, cx),
        Effect::Timer(d, t) => cx.forward_timer(d, t),
    }
}
```

Typed, checked at compile time, and one serialisation at the wire boundary instead of three.
Write two or three protocols this way by hand first. Only once the dispatch boilerplate is
visibly repetitive should you write a macro to remove it — the DSL is the residue of algorithms
that already work, not scaffolding erected in advance. Building the framework first is the
mistake this repository already made.

### 5. Add the network last, and let QUIC do the work

When protocols work under simulation, the transport becomes a thin adapter with two
responsibilities: `send(NodeId, Bytes)` best-effort, and a `SessionChanged(NodeId, epoch)` event
— the one contract worth carrying over from `conn.rs`. Keep the stale-write deadline.

Reach for `quinn` rather than TCP plus reconnection logic. QUIC gives you connection identity
across address changes, stream multiplexing, and framing, which deletes `multiplex.rs`,
`switchboard.rs` and the string keys outright. The layer that consumed eleven of your twelve
working days mostly stops being a layer you write.

### 6. Climb the ladder, in order, each rung with properties

Use the book the archive was already following. Each rung is a small `Protocol` impl, tested in
the simulator against its stated guarantees before the next is started.

1. Fair-loss / stubborn link
2. Perfect link
3. Failure detector
4. Best-effort broadcast
5. Reliable broadcast
6. Uniform reliable broadcast
7. **Paxos or Raft** — the real test

Reliable broadcast is the honest milestone: five composed protocols is the minimum that proves
the composition model holds. Consensus is what proves it was worth building. Errors get
`thiserror` types per layer — the string `"json decoding error"` should not appear once.

---

## Closing

The instinct that started this was correct, and the fragments confirm it: the session-epoch
contract is a better link abstraction than most production code has, and the fact that the gossip
algorithms were transcribed at all means the hard translation from pseudo-code to data structures
is already done once.

What went wrong is that the project spent its entire life on the layer where 2018 Rust was most
hostile, and never reached the layer it was actually about. **The difficulty that stopped you has
largely been removed** — async/await landed, the framework churn settled, sans-IO became a
recognised pattern, and deterministic simulation became something you can pull off the shelf. A
restart today is not the same attempt with more stamina. It is a substantially easier problem,
approached in the opposite order.

---

## Colophon

Drawn from the working tree at `373a7b1` and the unmerged `origin/link` and `origin/actix`
branches. The archived sources were read, never executed — §3 is about what that limitation
costs.

Section 3 was revised after checking every claim against Cachin, Guerraoui & Rodrigues,
*Introduction to Reliable and Secure Distributed Programming*, 2nd ed. (Springer, 2011),
§3.8 "Probabilistic Broadcast", algorithms 3.9–3.11 on pages 95–99. Three of its four original
findings did not survive that check and are marked as withdrawn rather than removed.

The Scala framework was originally inferred from the module names `upb` / `lpb`; the author has
since confirmed it was Kompics, from the KTH distributed systems course that uses this book as
its text. Section 5 step 4 rests on that confirmation rather than on inference.
