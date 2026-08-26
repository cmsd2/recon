## MODIFIED Requirements

### Requirement: Stable storage survives a crash

The simulator SHALL provide each process with storage that survives a crash and a restart, while
volatile state does not. That storage SHALL hold a metadata value and an append-only sequence of
entries, and SHALL be readable synchronously by the process that owns it. Storage SHALL be part of
the run's deterministic state, so that a seed reproduces a run including everything read from it.

#### Scenario: What was written is retrieved after a restart

- **WHEN** a process writes durable state, crashes, and restarts
- **THEN** it can read back what it wrote, and nothing it held only in memory

#### Scenario: Appended entries survive in order

- **WHEN** a process appends entries, crashes, and restarts
- **THEN** reading from the beginning yields those entries in the order they were appended

#### Scenario: A process that never wrote is initialised again rather than recovered

- **WHEN** a process crashes having written nothing
- **THEN** its initialisation entry point runs, not its recovery one

#### Scenario: Every process is initialised at the start of a run

- **WHEN** a run begins
- **THEN** each process's initialisation entry point runs once, and any effects it emits are
  interpreted like those of any other event

#### Scenario: A run with storage is reproducible from its seed

- **WHEN** a run involving writes, appends, crashes and recoveries is repeated with the same seed
  and configuration
- **THEN** it produces an identical trace

### Requirement: A write takes time and can be interrupted by a crash

A write SHALL NOT become durable instantaneously, and the simulator SHALL be able to crash a
process while one is outstanding. When that happens the write SHALL have taken effect or not,
chosen by the seeded source, and the recovering process SHALL have no way to tell which case it is
in beyond reading what survived.

A write being *visible* to the writing process is immediate and separate: what a protocol writes it
can read back at once, within the same incarnation. What a crash decides is whether it was ever
durable.

This is the fault that finds the bugs. An algorithm that is correct only when writes are atomic and
instantaneous is not correct.

#### Scenario: An outstanding write may or may not survive

- **WHEN** a process is crashed while a write is outstanding, across many seeds
- **THEN** some runs recover the new state and some recover the old, and both are legitimate

#### Scenario: An outstanding append may or may not survive

- **WHEN** a process is crashed while an append is outstanding, across many seeds
- **THEN** some runs read the entry back and some do not, and both are legitimate

#### Scenario: A completed write always survives

- **WHEN** a write has completed before the crash
- **THEN** the recovering process reads it in every run

#### Scenario: A partially written value is never retrieved

- **WHEN** a crash interrupts a write
- **THEN** what is read is either the whole new value or the whole previous one, never a mixture

#### Scenario: A crash never leaves a gap in the entries

- **WHEN** a crash interrupts a sequence of appends
- **THEN** what survives is a prefix of what was appended, never a sequence with a hole in it

### Requirement: Storage activity is visible in the trace

Writes, appends, their becoming durable, and whether a recovering process found anything SHALL
appear in the trace, so that properties about durability can be asserted over the trace rather than
over protocol internals.

#### Scenario: A durability property is assertable without touching internals

- **WHEN** a test needs to establish that something was durable before a message was sent
- **THEN** it can determine that from the trace alone

#### Scenario: Rewriting and appending are distinguishable

- **WHEN** a run both replaces metadata and appends entries
- **THEN** the trace shows which happened, so that a claim about a protocol's write cost can be
  checked rather than asserted

## ADDED Requirements

### Requirement: Nothing is dispatched to a process while it is starting or recovering

The simulator SHALL NOT deliver a message, fire a timer, or dispatch a command to a process
between entering its initialisation or recovery handler and that handler returning.

This holds today by accident — both handlers run synchronously, outside the event loop — and it is
load-bearing: a protocol part-way through loading its durable state would otherwise act on a
message while believing it has recorded nothing. Stating it makes it a property that can be
checked rather than an arrangement that can be disturbed.

#### Scenario: A message in flight waits for recovery to finish

- **WHEN** a process restarts with messages already in flight to it
- **THEN** none is delivered until its recovery handler has returned

#### Scenario: A protocol that reads during recovery is not interrupted

- **WHEN** a recovery handler reads its durable state and acts on what it finds
- **THEN** nothing else has been dispatched to it in the meantime
