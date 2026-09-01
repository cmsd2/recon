## ADDED Requirements

### Requirement: What a process says is recorded in the same account as what happened to it

A narrated decision SHALL appear in the run's trace, in the order it was narrated relative to every
other recorded event, and on the same clock.

One account, not two. A separate record of what protocols claimed would agree with the record of what
happened only because something merged them, and the whole value of narration is in reading the two
against each other: a claim to have reached a quorum beside the deliveries that were supposed to
constitute it.

This is also what makes narration **checkable**. A record a test can read is a record a test can
require to agree with the run; a record only a human reads is worth exactly as much as a comment.

#### Scenario: A narrated decision appears in the trace

- **WHEN** a protocol narrates a decision during a run
- **THEN** the trace contains it, attributed to that process and that instant

#### Scenario: Narration is ordered with the rest of the run

- **WHEN** a protocol narrates a decision and then sends a message
- **THEN** the trace holds them in that order

#### Scenario: A claim can be checked against what happened

- **WHEN** a test reads a narrated decision from the trace
- **THEN** it can require the effects that decision implies to be present in the same trace, and
  require their absence for a decision to take no action

### Requirement: A trace can be rendered to a tracing subscriber as it is recorded

The simulator SHALL be able to emit each recorded event to a `tracing` subscriber at the moment it
records it, carrying the process and the run's virtual time.

At the moment it is recorded, not at the end: a run that fails to terminate is one of the things
worth reading, and a renderer that walks a finished trace has nothing to show for it.

Virtual time, not wall time: a subscriber's own timestamps describe how long the *simulation* took,
which is unrelated to the run being reproduced and actively misleading when read as if it were.

Rendering SHALL be off unless asked for, like the codec check and session-event delivery, so that a
run pays nothing for an audience it does not have.

#### Scenario: A hanging run still reports

- **WHEN** a run does not terminate and rendering is enabled
- **THEN** the events recorded before it stopped progressing have already been emitted

#### Scenario: Events carry virtual time

- **WHEN** an event is rendered
- **THEN** the time it carries is the run's, not the wall clock's

#### Scenario: A run without an audience is unchanged

- **WHEN** rendering is not enabled
- **THEN** the run behaves exactly as it does today
