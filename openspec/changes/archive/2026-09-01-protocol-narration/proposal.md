## Why

The simulator's trace records what happened *to* a protocol — what was sent, delivered, lost,
written. It cannot record what the protocol **decided**, and in particular it cannot record a
decision whose outcome was to do nothing. The three diagnoses this repository keeps citing were all
of that kind: the epoch that climbed to 647,309 needed somebody to see `ets`; the leader trusted by
everyone that announced nothing was a *silence*, and a silence leaves no trace event at all. Each was
reached by hand-writing a throwaway probe, printing state, reading it, and deleting it.

The archived `scenario-shrinking` change settled the narrow version of the argument by measurement:
reducing a real defect turned nine faults and five processes into one command and one process, which
was worth having, and said nothing whatever about *why*. This is the item that answers why.

**The governing constraint is that narration must be checkable, not merely present.** A
`tracing::info!` beside a state change is a second statement about that change, and second statements
go stale — which is the failure this repository already documents for quoted pseudocode, and already
suffered once. So a note has to land somewhere a test can read it and compare it against what the
run actually did. That is the trace.

## What Changes

- **A protocol can narrate a decision.** `Protocol::Note` names the vocabulary a protocol narrates
  in. A protocol that narrates nothing declares `Infallible`, so it cannot construct one and
  `cx.note(..)` is uncallable for it — exactly as `Scope` already works for scope events. (By
  convention rather than by a language default: associated type defaults are not stable, so `Scope`
  is written out at every implementation too.)
- **`Cx` carries the note channel**, alongside time, randomness and storage. Not because narration
  is randomness, but because the two facts most needed when reading a seeded run — *which process*
  and *when in virtual time* — are supplied by the driver and can therefore never be forgotten or
  wrong at a call site. A global `tracing` subscriber gets both wrong: five processes share one
  thread, and `tracing-subscriber` timestamps with the wall clock.
- **A note is a trace event.** `TraceEvent::Said` puts what a process claims into the same ordered
  account, with the same clock, as what happened to it. This is what makes narration *checkable*:
  a test can require that a process claiming a quorum is a process to which a quorum of
  acknowledgements was delivered, and that an epoch started is an epoch narrated.
- **Narrating does not change the run.** The same seed with narration on and off produces the same
  trace but for the `Said` events, and this is asserted rather than assumed.
- **`recon-sim` gains a `tracing` dependency and renders the trace to it** — live as events are
  recorded, so a run that hangs still logs, with `node` and virtual `at` as span fields. The
  dependency lands only there: `recon-core` and `recon-protocols` stay as they are.
- **One module is narrated as proof**: `epoch_change`, chosen because its interesting decisions are
  the ones that produce no effect — a `NewEpoch` refused, a `NACK` ignored because the timestamp was
  already passed, a leader told where a follower has reached. Its suite gains the agreement tests.

Deliberately not in scope: narrating the other twenty-five modules. The vocabulary should be
designed against decision points somebody is actually trying to read, and a second pass costs
nothing that this one saves.

## Capabilities

### Modified Capabilities

- `protocol-core`: a protocol may narrate a decision through its context, in a vocabulary it
  declares; a protocol that declares none cannot; and narrating does not change the run
- `simulation`: what a process says is recorded in the same trace, and in the same order, as what
  happened to it, and the trace can be rendered to a `tracing` subscriber as it is recorded

## Impact

`recon-core`: `Protocol` gains an associated `Note` type defaulting to `Infallible`; `Cx` gains a
note channel and a type parameter. The parameter is absorbed by `ProtoCx<'a, P>` — **290 of the 298
mentions of a context in this repository are `ProtoCx<'_, Self>` and do not change**, and the eight
that do are all in `cx.rs`, `child.rs` and one core test. No algorithm changes shape.

`recon-sim`: `Trace` and `TraceEvent` gain the note parameter (ten and five mentions across the
suites), a `Said` variant, and a `tracing` dependency with a renderer.

`recon-protocols`: one shared `Note` enum, and `epoch_change` narrating into it.
