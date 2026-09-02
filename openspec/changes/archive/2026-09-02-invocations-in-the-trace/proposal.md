## Why

The trace holds completions without the instants that began them. `Sim::command` schedules an
operation and records nothing; `TraceEvent` has `Indicated` and no counterpart for the invocation. So
a run can say what a process *concluded* and never what it was *asked*.

That is the foundational gap for the evidence track. Linearizability is defined over the interval
`[invoke, complete]` — an operation may take effect anywhere inside it — so no checker is possible
without the left-hand end, whatever else is built. It pays for itself before any of that: no test
here can currently ask how long an operation took, or whether two of them overlapped.

**And an operation can vanish entirely.** A command whose process is not running is discarded
without a word — `crates/recon-sim/src/sim.rs`, in the `Scheduled::Command` arm, returns before the
handler runs. Nothing in the trace says the operation was ever asked for. That is absorption without
an event, which `docs/conditional-guarantees.md` names the cardinal sin and holds the simulator to as
strictly as any layer.

**A stalled process is the interesting case, and the answer is not what it first looks like.** The
simulator holds timers, deliveries and scope events for a suspended process and does *not* hold
commands. That looks like an oversight against its own rule that "nothing addressed to a suspended
process is dropped" — but that rule is about network traffic inside a live session, where a message
lost with no `SessionEnded` to announce it is loss nobody is told of. A command is not network
traffic. It is a call from the layer above, on that node, and if the node is stalled that layer is
stalled with it: there is a receive buffer between a socket and its reader, and nothing at all
between an application and its protocol. A held delivery models a real queue; a held command would
model nothing.

So a command to a stalled process is **not** delivered late — it is recorded as never begun, with
the stall as the reason. What was wrong was the silence, not the dropping.

## What Changes

- **An operation has an identity.** `Sim::command` and `Sim::command_at` mint an `OpId` and return
  it, so a test can name the operation it just issued.
- **An invocation is a trace event.** `TraceEvent::Invoked { at, node, op, cmd }`, recorded **when
  the handler runs**, not when the command was scheduled. A handler's effects cannot precede it, so
  `[dispatch, completion]` is a valid interval and a strictly tighter one than
  `[scheduled, completion]`; suites here routinely schedule a batch at time zero, which would
  otherwise make every operation overlap every other and tell a checker nothing.
- **A command that never reaches its process is recorded too**, rather than dropped in silence, with
  why. That closes the absorption above, and it is the seed of the next item: a `Propose` whose
  process died before the handler ran is exactly Jepsen's `:info` — *may or may not have happened*.
- **A stalled process's commands stay dropped, and are now recorded.** Distinguishing why an
  operation never began — crashed, stalled, not a member — is what makes a dropped command a clean
  *definitely did not happen* rather than a hole. That is a stronger history entry than an operation
  whose interval begins mysteriously at resume, and it leaves *may or may not have happened* to mean
  what the next item needs it to mean: a process that died inside the handler.
- **The trace can be asked about operations**: what was invoked, by whom, when.

**Deliberately not in scope: pairing a completion to its invocation.** The pairing does not exist in
the algorithms this repository has. Every correct process raises `Ind::Decide`, including ones that
proposed nothing, and the value need not be the proposer's; a broadcast's `Deliver` is an event
arriving at a process rather than a reply to anything it asked. Marking indications with the
operation they complete would be fiction for twenty-five of twenty-six modules, and load-bearing
fiction, because a checker would trust it. It would also oblige every protocol to hold a
driver-assigned identity in its own state and keep it across a crash, which is the defect the
2026-08 audit found three times. Pairing belongs to the replicated-log port, where an operation has
a caller waiting for a result. This change is the half that every design of that pairing needs
first.

## Capabilities

### Modified Capabilities

- `simulation`: an operation given to a process is recorded in the trace when it is handled, with an
  identity the caller was given; and one that never reaches its process is recorded as such rather
  than discarded silently

## Impact

`recon-sim`: `OpId`; `Sim::command` and `Sim::command_at` gain a return value; `TraceEvent` gains two
variants and `Trace` two accessors; `Sim` acquires a `P::Cmd: Clone` bound, which every command in
this repository already satisfies. `Scheduled::Command` carries its `OpId`.

No behaviour changes: what the simulator does with a command is unchanged, and only what it records
is new. `a_suspended_process_handles_nothing_while_stopped` and every other existing suite hold as
they are.

No protocol changes. No existing suite changes behaviour, though callers wanting the identity opt in
by using the return value.
