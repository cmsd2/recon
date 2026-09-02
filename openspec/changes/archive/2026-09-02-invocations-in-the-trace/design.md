## Context

The trace is this project's standard of evidence, and it records what happened *to* a process and
what a process concluded. It has never recorded what a process was *asked*. Everything on the
evidence track past this point — indeterminate outcomes, a concurrent workload, a checker — reads
intervals, and an interval needs a left-hand end.

What was found while writing the requirement is one defect, not two: a command to a process that is
not running is discarded silently. The first draft of this design called the suspended case a second
defect and proposed to hold commands as the simulator already holds timers, deliveries and scope
events. That was wrong, and the section below says why.

## Goals / Non-Goals

Goals: an identity for an operation, the instant it was handled, and a record when it was not.

Non-goals: pairing a completion to its invocation; any change to `Protocol`; any change to what a
protocol can observe.

## Decisions

### The invocation instant is when the handler ran, not when the command was scheduled

Both are valid left-hand ends of an interval containing the effect, because a handler's effects
cannot precede the handler. Dispatch is the tighter one, and tightness is the whole value: suites
here routinely schedule several commands at time zero, and recording the scheduling instant would
make every one of them appear to overlap every other. A checker fed that history could rule out
almost nothing.

The cost is that an operation which never runs has no dispatch instant — which is exactly why the
second requirement exists rather than being an afterthought.

### Pairing is out of scope because it is not real yet

The pairing a checker wants does not exist in twenty-five of the twenty-six modules here. Every
correct process raises `Ind::Decide`, including processes that proposed nothing, and the value need
not be the proposer's; `Ind::Deliver` is an event arriving at a process, not a reply to something it
asked. Marking an indication with the operation it completes would be fiction, and fiction a checker
would trust.

There is a second cost that decided it. A protocol echoing back a driver-assigned `OpId` has to
*hold* one, and this repository's rule is that identity is as durable as the state it keys — so
every module would acquire an obligation to persist or re-derive it in `on_recovery`. That is the
defect the 2026-08 audit found three times, distributed across the whole stack on purpose.

A pairing port was considered and rejected for the same reason `link.rs` records `ScopedLink` being
deleted: an abstraction built ahead of its only consumer is one whose shape was guessed. The
replicated-log port is where an operation has a caller waiting for a result, and it will say what
the pairing needs.

### The trace grows a fourth vocabulary, and an alias to name it

Not foreseen, and worth recording. `Trace` and `TraceEvent` now carry the command type as well as the
message, indication and note types — four parameters, which clippy rightly called a very complex type
where the simulator holds one. So `ProtoTrace<P>` and `ProtoTraceEvent<P>` join `recon_core`'s
`ProtoCx<'a, P>` and `ProtoEffect<P>`, for the same reason those exist: a protocol's own trace type
should be nameable without spelling out four associated types. Every suite that named the type by
hand now names the alias instead, and reads better for it.

### `OpId` is minted by the driver, like `TimerId`

One source per run, so identities do not collide, and opaque to protocols — no protocol sees one, and
none can, because commands are not changed. That is what keeps this out of `Protocol` entirely.

### Not handled, and why, rather than a bare absence

A command discarded records the reason. "The process had crashed" and "the process was gone from the
membership" are different facts, and the next item on the roadmap — indeterminate outcomes — is
built on telling them apart. An operation asked for and never begun is also not the same as one never
asked for, and a record that cannot distinguish those is one a checker reasons from falsely.

### A stalled process does not hold its commands, and should not

Reversed after review, and the reasoning is worth keeping because the wrong version was plausible.

The simulator holds timers, deliveries and scope events for a suspended process and not commands,
which reads as an oversight against its rule that "nothing addressed to a suspended process is
dropped". It is not, because that rule is about **network traffic inside a live session**: a message
discarded while its session stays up is loss with no `SessionEnded` to announce it, which is the
thing no layer may do. The justification does not reach a command, which has no session and no sender
awaiting delivery.

And the model argues the other way, once it is clear what a suspension *is*. It is not a network
delay and not unavailability in the crash sense: the process is up, its memory intact and its
sessions open, and it simply is not scheduled — a GC pause, a VM pause, `SIGSTOP`. From the outside
that looks exactly like a delay, which is why everything addressed to it arrives late rather than
being lost. From the inside it is a gap, which is why a resumed process is not told that time passed
and comes back with stale evidence and a timer due immediately.

**Commands are on the wrong side of that line.** A `Deliver` crosses into the stalled process from
outside, so delaying it is faithful — it waits in a receive buffer, which is a real thing. A `Cmd`
originates *inside* the same layer stack, above the protocol, so it is on the stalled side: there is
nothing to delay it, because the code that would have issued it is not running either. There is a
buffer between a socket and its reader; there is nothing between an application and its protocol. A
held delivery models a queue that exists. A held command would model one that does not.

Two things follow that the first draft had backwards:

- **The absorption is cured by recording, not by holding.** The defect was the silent `return`, and
  `NotInvoked` fixes it exactly.
- **A discarded command is the better history entry.** It *certainly did not happen*, which a checker
  can use, where an operation whose interval begins at resume is a puzzle. It also leaves *may or may
  not have happened* to mean what the next roadmap item needs — a process that died inside the
  handler.

So a crash and a stall agree about commands and differ about everything else, which is where the
distinction between them actually lives.

## Risks / Trade-offs

- **`Sim` acquires `P::Cmd: Clone`**, since the trace keeps the command and the handler consumes it.
  → Every command in the repository already derives `Clone`; the scenario work established that. Two
  generic test helpers needed the bound restating, which the compiler found.
- **A return value is not quite free.** The claim that existing callers are unaffected holds for
  behaviour and for statement position, but a closure written `|s| s.command(..)` in a `()` context
  stops compiling, because the tail expression is now an `OpId`. Four such sites needed braces. Small
  and mechanical, but the claim was stated too broadly and this is what it actually cost.
- **Two more trace variants**, on a type that several suites match exhaustively. → The compiler finds
  them; most matches here are `matches!` and unaffected.
- **No behaviour changes at all.** What the simulator *does* with a command is untouched; only what
  it records is new. `a_suspended_process_handles_nothing_while_stopped` asserts a count that the
  held-commands draft would have changed, and under this design it stands as written. That the risky
  half of the change disappeared is a point in the design's favour rather than an accident.
- **Recording the command means the trace grows** by one entry per operation. → Negligible beside
  deliveries, and it is the entry the trace exists to gain.

## Migration Plan

1. `OpId`, minted by the driver; `command` and `command_at` return one.
2. `TraceEvent::Invoked`, recorded at dispatch, and the accessors.
3. `TraceEvent::NotInvoked` with its reason, replacing the silent discard.
4. The tests that a stall and a crash are both recorded, and are told apart by reason.
5. Docs: the roadmap's item `C`, the `recon-sim` section, the suite table and counts.

## Open Questions

- **Whether `NotInvoked` should also cover a run that ends with commands still queued.** A command
  scheduled beyond the horizon is not discarded — it is simply never reached — and calling that
  "not invoked" would conflate a fault with an unfinished run. Left out for now; the run's end is
  visible without it, and item `D` may want the distinction drawn differently.
