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
interface** instead: `get`/`set` for metadata, `append` and `read_from` for a log. Synchronous is
not the same as durable — the call returns immediately and is visible to later reads in the same
process, while durability stays deferred and stays the driver's business, exactly as a write to a
page cache returns before `fsync`. That makes storage precisely analogous to `cx.now()` and
`cx.rng()`: state the driver supplies so it can be made virtual and seeded. Recovery stays
synchronous, so the invariant survives untouched and needs no recovering state, no held messages,
and no reply events.

## What Changes

- **A synchronous `Store` handle on the context**, with `get`/`set` for a metadata value,
  `append` for a log entry, `read_from(position)` for a suffix, and `end()` for the current
  position. **BREAKING**: `Effect::Store` is removed; the six protocol call sites and four test
  ones move to `storage().set(..)`.
- **The compile-time check is kept.** A protocol declares its metadata and entry types, and one
  that keeps nothing declares them uninhabited — so `set` and `append` take an argument nobody can
  construct and a write stays a compile error, exactly as `Durable = Infallible` does today. Reads
  become vacuous rather than forbidden, which is harmless.
- **The ordering rule survives positionally.** It is currently enforced because `Effect::Store`
  sits in the effect stream and the driver holds everything emitted after it. A synchronous write
  pushes a *marker* into the same stream — the value travels by the handle, the position by the
  marker — so nothing observable leaves a process before the write it depends on is durable.
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
  what it does and does not promise; the write-ordering requirement is restated in terms of a
  synchronous write rather than an effect.
- `simulation`: storage gains an append-only log beside the metadata value, with the
  crash-during-write behaviour extended to cover both.
- `links/logged-link`: the durable record becomes an appended log with metadata, and the module's
  space statement changes from "unbounded, and rewritten in full" to "unbounded, appended once".
- `broadcast/logged-uniform-reliable-broadcast`: the same.

## Impact

- `recon-core`: a `Store` trait, a handle on `Cx`, two associated types replacing `Durable`, and
  one fewer effect variant. Every protocol declares the two types; all but two declare them
  uninhabited and are otherwise untouched.
- `recon-sim`: a per-process store with a metadata slot and an append-only log, both part of the
  seeded state and both subject to the interrupted-write fault.
- Two protocol modules rewritten around append; their suites gain assertions about what is written
  rather than only what is remembered.
- **This supersedes part of the change archived immediately before it.** `Effect::Store` and
  `Durable` were the right first cut and lasted one consumer each; saying so plainly is better than
  presenting the replacement as an extension.
- `docs/bounded-space.md`: the `O(n²)` measurement becomes historical for these two protocols, and
  the remaining growth — in memory, and in the log itself — is restated.
