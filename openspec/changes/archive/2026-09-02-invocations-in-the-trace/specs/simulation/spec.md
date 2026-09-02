## ADDED Requirements

### Requirement: An operation given to a process is recorded when it is handled

The simulator SHALL record in the trace every command it hands to a process: which process, which
command, and the instant the process handled it.

The instant recorded SHALL be the one at which the process handled the command, not the one at which
the command was scheduled. A handler's effects cannot precede the handler, so this is a valid
left-hand end for the interval containing the operation's effect, and a tighter one than the moment
the caller asked. A looser end is not merely wasteful: several operations scheduled at one instant
would otherwise appear to overlap when they did not.

The caller SHALL be given an identity for the operation when it issues one, so that a test can name
the operation it has just asked for and find it in the trace.

#### Scenario: An operation appears in the trace

- **WHEN** a command is given to a running process
- **THEN** the trace records it against that process, with the command, and with the instant the
  process handled it

#### Scenario: The instant recorded is when it was handled

- **WHEN** a command is scheduled to be given to a process later than now
- **THEN** the instant recorded is when the process handled it, not when it was scheduled

#### Scenario: Operations scheduled together do not appear to overlap

- **WHEN** several commands are scheduled at one instant and handled at different instants
- **THEN** the trace distinguishes when each was handled

#### Scenario: The caller can name what it asked for

- **WHEN** a caller issues a command
- **THEN** it receives an identity that names that operation in the trace, distinct from every other
  operation in the run

### Requirement: An operation that never reaches its process is recorded as such

A command that the simulator discards without handing it to a process SHALL be recorded, with the
reason, rather than dropped silently.

An operation asked for and never begun is not the same as one never asked for, and a record that
cannot tell them apart is a record a checker would reason from falsely. This is the same obligation
every layer is under: something lost without an event saying so is the failure this project treats as
cardinal, and the simulator is subject to it as strictly as the protocols are.

Discarding is the correct behaviour and SHALL be kept; what was missing was the record. A command is
not network traffic held in a buffer — it is a request from the layer above, which on a process that
is not running is not running either. A recorded discard is also the more useful history: an
operation that certainly did not begin is a stronger fact than one whose beginning is unexplained.

#### Scenario: A command to a crashed process is recorded

- **WHEN** a command is given to a process that has crashed and not restarted
- **THEN** the trace records that the operation was asked for and did not reach the process, and why

#### Scenario: A command to a stalled process is recorded as never begun

- **WHEN** a command is given to a suspended process
- **THEN** the trace records that the operation did not reach the process, and that the process was
  stalled — and the operation is not handled when the process resumes

#### Scenario: Why an operation did not begin is distinguishable

- **WHEN** operations fail to begin for different reasons across a run
- **THEN** the trace says which reason applied to which operation

#### Scenario: Asked for and never begun is distinguishable from never asked for

- **WHEN** a run contains an operation that never reached its process
- **THEN** the trace distinguishes it from an operation that was never issued at all
