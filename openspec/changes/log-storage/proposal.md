## Why

Storage arrived as a single effect, `Store(D)`, carrying a protocol's entire durable state. Ten
call sites later, every one of them is the same line — `cx.store(self.<whole_state>.clone())` —
and the shape has three problems that are really one.

**It is the wrong cost.** The durable value is written in full on every change, so a protocol that
records `n` things writes `O(n²)` bytes over its life. `docs/bounded-space.md` records the
measurement and names the fix.

**It cannot read.** Effects are one-way, so nothing can ask storage a question. Recovery works only
because the driver *pushes* the whole value, which is tenable exactly as long as the whole value
fits in one hand-over.

**Making it read would break something load-bearing and undocumented.** If a read returned its
answer as a later event, recovery would no longer complete inside the recovery handler — and the
simulator relies on it completing there, in two places, by accident rather than by design. A logged
link that had not finished loading its `delivered` set when a retransmission arrived would
log-deliver a duplicate, which is the one property that protocol exists to provide.

The resolution is to stop treating storage as an effect and give the protocol a **synchronous
interface** instead: `get`/`set` for metadata, `append` and `read_from` for a log. That makes
storage precisely analogous to `cx.now()` and `cx.rng()` — state the driver supplies so it can be
made virtual and seeded — and recovery stays synchronous, so the startup invariant survives
untouched and needs no recovering state, no held messages, and no reply events.

**A write is durable when it returns.** A protocol is a synchronous state machine, so the return of
a write call is the *only* point at which a driver can synchronise with it: after the handler
returns, the sends are already in the driver's hands. Anything weaker would let a process be seen
by its peers to have made a promise it has no record of, and there would be no place left to stop
it. The cost is that a write blocks, which is what `fsync` does and what the guarantee requires.

## What Changes

- **A synchronous `Store` handle on the context**, with `get`/`set` for a metadata value,
  `append` for a log entry, `read_from(position)` for a suffix, and `end()` for the current
  position. **BREAKING**: `Effect::Store` is removed; the six protocol call sites and four test
  ones move to `storage().set(..)`.
- **The compile-time check is kept.** A protocol declares its metadata and entry types, and one
  that keeps nothing declares them uninhabited — so `set` and `append` take an argument nobody can
  construct and a write stays a compile error, exactly as `Durable = Infallible` does today. Reads
  become vacuous rather than forbidden, which is harmless.
- **The ordering rule becomes a property of the call.** It is currently enforced because
  `Effect::Store` sits in the effect stream and the driver holds everything emitted after it. A
  synchronous write that is durable on return needs no holding: nothing emitted after it can leave
  before it, because it has already happened. The rule stops being a driver obligation.
- **The crash-during-write fault is armed rather than incidental.** With no window in which a write
  is outstanding, the only way to observe one that did not land is to kill the process inside it.
  The simulator gains a way to say so, and the write that killed it may or may not have taken
  effect, decided by the seed and invisible to the recovering process.
- **Both logged protocols convert from rewriting to appending**, which is what makes this a change
  rather than a refactor: their write cost goes from `O(n²)` to `O(n)`, and `read_from` is used for
  real rather than for demonstration.
- The simulator's storage gains a log beside the metadata blob, keeping the crash-during-write
  fault, the determinism, and the trace.

Explicitly **not** in scope: truncation and compaction, which nothing needs yet; bounding the
protocols' *memory*, which is a separate problem needing per-sender ordering from the link; and any
new protocol.

## Capabilities

### New Capabilities

None. This changes how an existing capability is expressed, not what the system does that it did
not do before.

### Modified Capabilities

- `protocol-core`: the effect vocabulary loses its store variant; the durable-state declaration
  becomes a metadata type and an entry type; a new requirement covers the synchronous interface and
  the durable-on-return promise; the write-ordering requirement becomes a consequence of that
  promise rather than a driver obligation.
- `simulation`: storage gains an append-only log beside the metadata value, and the
  crash-during-write fault becomes something a test arms explicitly.
- `links/logged-link`: the durable record becomes an appended log with metadata, and the module's
  space statement changes from "unbounded, and rewritten in full" to "unbounded, appended once".
- `broadcast/logged-uniform-reliable-broadcast`: the same.

## Impact

- `recon-core`: a `Store` trait, a handle on `Cx`, two associated types replacing `Durable`, and
  one fewer effect variant. Every protocol declares the two types; all but two declare them
  uninhabited and are otherwise untouched.
- `recon-sim`: a per-process store with a metadata slot and an append-only log, both part of the
  seeded state, plus `crash_on_next_write` to arm the interrupted-write fault. The configurable
  write latency goes: with writes durable on return there is nothing for it to delay.
- Two protocol modules rewritten around append; their suites gain assertions about what is written
  rather than only what is remembered.
- **This supersedes part of the change archived immediately before it.** `Effect::Store` and
  `Durable` were the right first cut and lasted one consumer each; saying so plainly is better than
  presenting the replacement as an extension.
- `docs/bounded-space.md`: the `O(n²)` measurement becomes historical for these two protocols, and
  the remaining growth — in memory, and in the log itself — is restated.
