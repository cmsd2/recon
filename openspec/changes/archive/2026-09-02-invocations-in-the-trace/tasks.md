`design.md`'s open question does not change what gets built: it asks whether a command still queued
when a run ends should be recorded as not-invoked, and the answer taken is no — a run's end is
already visible, and conflating an unfinished run with a fault is what item `D` exists to get right.

## 1. An operation has an identity

- [x] 1.1 `OpId`, minted from one source per run as `TimerId` is, so identities do not collide and no
      protocol ever sees one. Verify two operations in a run never share an identity
- [x] 1.2 `Sim::command` and `Sim::command_at` return the `OpId` they minted, and `Scheduled::Command`
      carries it. Verify a caller can name the operation it just issued and find it in the trace
- [x] 1.3 Verify existing callers are unaffected — the identity is a return value, not a parameter.
      True of behaviour and of statement position; **not** quite true of syntax, since a closure
      written `|s| s.command(..)` in a `()` context stops compiling once the tail expression is an
      `OpId`. Four sites needed braces, and `design.md` records that the original claim was broader
      than what it cost

## 2. An invocation in the trace

- [x] 2.1 `TraceEvent::Invoked { at, node, op, cmd }`, and the accessors to read invocations back.
      `Sim` acquires `P::Cmd: Clone`, which every command here already satisfies
- [x] 2.2 Record it **when the handler runs**, not when the command was scheduled. Verify a command
      scheduled for later is recorded at the later instant
- [x] 2.3 Verify several commands scheduled at one instant and handled at different instants are
      distinguishable — the reason dispatch is the instant recorded. Recording the scheduling instant
      would make every one of them appear to overlap every other, and a checker fed that could rule
      out almost nothing
- [x] 2.4 Verify a test can now ask what this change exists to make askable: when an operation began,
      how long until a given indication, and whether two operations' windows overlapped

## 3. An operation that never began

- [x] 3.1 `TraceEvent::NotInvoked { at, node, op, why }`, replacing the silent `return` in the
      `Scheduled::Command` arm. The reason is carried because "the process had crashed" and "the
      process is not in the membership" are different facts, and the next roadmap item is built on
      telling them apart
- [x] 3.2 Verify a command to a crashed process is recorded rather than discarded
- [x] 3.3 Verify asked-for-and-never-begun is distinguishable from never-asked-for. Absorption
      without an event is what `docs/conditional-guarantees.md` names the cardinal sin, and the
      simulator is subject to it as strictly as any layer

## 4. A stall and a crash, told apart

- [x] 4.1 Verify a command to a **suspended** process is recorded as never begun, with the stall as
      the reason, and is **not** handled when the process resumes. Discarding is correct and stays:
      a command is a call from the layer above, on that process, and a stalled process's layer above
      is stalled with it. There is a buffer between a socket and its reader and nothing between an
      application and its protocol, so a held delivery models a real queue and a held command would
      model none
- [x] 4.2 Verify a command to a **crashed** process is recorded with that reason instead, so the two
      are distinguishable. This is the distinction the next roadmap item is built on
- [x] 4.3 Verify no existing behaviour changed: what the simulator does with a command is untouched
      and only what it records is new. `a_suspended_process_handles_nothing_while_stopped` asserts a
      count that an earlier draft of this design would have changed, and must still pass unedited

## 5. What this dates

- [x] 5.1 `README.md`'s roadmap item `C`: mark it built, and say what it does and does not buy —
      intervals need a pairing this change deliberately does not invent, and the reason belongs in
      the roadmap where the next reader will look for it
- [x] 5.2 `README.md`'s `recon-sim` section, the suite table and the counts
- [x] 5.3 `./scripts/check.sh` passes in full
