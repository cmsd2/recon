## Why

Nothing in this repository survives a crash. `Sim::crash` rebuilds a process from its constructor,
so a restart is genuine amnesia — but there is nowhere to write anything down, so the whole
fail-recovery half of the book is unreachable. Ω writes its epoch down; epoch consensus writes its
state down; neither can be attempted until a process can remember something across an incarnation.

`docs/conditional-guarantees.md` already names the scope this closes. Its lattice is
`session ⊑ incarnation ⊑ always`, and stable storage is listed as the redundancy that bridges a
restart — the entry has been sitting there unimplemented since the scope work was written.

The interface consequence is the reason this is a change and not a utility. A crash-stop protocol
notifies the layer above by triggering `⟨ Deliver | m ⟩` once. A crash-recovery protocol cannot:
the process may crash immediately afterwards, and neither it nor anyone else will ever know the
indication happened. The book's answer, which this change adopts, is that the indication carries
**the durable log itself**:

> **Indication:** ⟨ lpl, Deliver | *delivered* ⟩: Notifies the upper layer of potential updates to
> variable *delivered* in stable storage (which log-delivers messages according to the text).

That is a different shape of promise, and it belongs at the bottom of the stack where it can be
seen rather than three abstractions up where it would be discovered.

## What Changes

- **A durable-storage effect in `recon-core`.** A protocol declares what it keeps durably and
  emits an effect to write it; the driver performs the write. This is an addition to the effect
  vocabulary, which the specification currently states exhaustively.
- **A recovery path.** Algorithm 2.3 has an explicit `⟨ Recovery ⟩` event distinct from `⟨ Init ⟩`,
  and the change follows the book rather than reconstructing state in the constructor.
- **Storage in `recon-sim`**: surviving a crash, taking time to complete, and — the case that finds
  bugs — **a crash landing mid-write, where the write may or may not have taken effect and the
  process cannot tell which**.
- **Logged perfect links** (Module 2.4, Algorithm 2.3 "Log Delivered"), over the existing stubborn
  link. The first consumer.
- **Stubborn best-effort broadcast** (§3.5), which the second consumer requires — see Impact.
- **Logged uniform reliable broadcast** (Module 3.6, Algorithm 3.8 "Logged Majority-Ack Uniform
  Reliable Broadcast"), the second consumer, which is the majority-ack algorithm just built with
  `pending` and `delivered` written down and `ack` deliberately not.

The indication carries the durable set, as the book has it, and the modules say so and say that it
grows without bound. A bounded delivered-*cursor* is the obvious follow-up and is explicitly not
attempted here: bounding changes the guarantee to a scope, and that is a change with a proposal.

## Capabilities

### New Capabilities

- `links/logged-link`: perfect-link guarantees stated over *log-delivery* rather than delivery, so
  that a restarted process can retrieve what it already delivered instead of delivering it again.
- `broadcast/stubborn-broadcast`: best-effort broadcast that delivers infinitely often, so that a
  process which was down when a message was sent receives it after recovering.
- `broadcast/logged-uniform-reliable-broadcast`: uniform agreement over log-delivery in the
  fail-recovery model, resting on a correct majority and on state that survives a restart.

### Modified Capabilities

- `protocol-core`: the effect vocabulary is stated exhaustively as send, indicate and set-timer;
  durable storage becomes a fourth. Adds the durable-state declaration, the recovery event, and the
  ordering rule between writing and sending.
- `simulation`: storage that survives a crash, takes time, and can be interrupted by one.

## Impact

- `recon-core`: a new effect variant, a `Durable` associated type on `Protocol`, and a recovery
  entry point. Every existing protocol declares no durable state and is unaffected, but the effect
  enum gains a variant, so every exhaustive match over it must be revisited.
- `recon-sim`: per-process storage in the deterministic state, a write-latency knob, and a
  crash-during-write fault.
- Three new modules in `recon-protocols`, three new test suites.
- **A scope consequence worth stating plainly.** The book's logged abstractions do **not** stack on one
  another: Algorithm 2.3 builds on stubborn links, Algorithm 3.7 builds on stubborn links, and
  Algorithm 3.8 builds on stubborn *broadcast*. Each keeps its own log. So the second consumer does
  not prove that the new indication shape composes upward — it proves that one storage primitive
  serves two independent consumers, which is a different and lesser claim. Adding stubborn
  broadcast is a consequence of choosing that second consumer, not an expansion beyond it.
- `README.md`: three rows, and a note on what the fail-recovery model changes.
- The bounded-space position gets worse before it gets better: these are the first abstractions whose
  unbounded state is on disk.
