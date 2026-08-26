## ADDED Requirements

### Requirement: Stable storage survives a crash

The simulator SHALL provide each process with storage that survives a crash and a restart, while
volatile state does not. Storage SHALL be part of the run's deterministic state, so that a seed
reproduces a run including everything recovered from it.

#### Scenario: What was written is retrieved after a restart

- **WHEN** a process writes durable state, crashes, and restarts
- **THEN** it is given back what it wrote, and nothing it held only in memory

#### Scenario: A process that never wrote recovers nothing

- **WHEN** a process crashes having written nothing
- **THEN** it starts as if for the first time

#### Scenario: A run with storage is reproducible from its seed

- **WHEN** a run involving writes, crashes and recoveries is repeated with the same seed and
  configuration
- **THEN** it produces an identical trace

### Requirement: A write takes time and can be interrupted by a crash

A write SHALL NOT complete instantaneously, and the simulator SHALL be able to crash a process
while one is outstanding. When that happens the write SHALL have taken effect or not, chosen by
the seeded source, and the recovering process SHALL have no way to tell which case it is in beyond
reading what it retrieved.

This is the fault that finds the bugs. An algorithm that is correct only when writes are atomic
and instantaneous is not correct.

#### Scenario: An outstanding write may or may not survive

- **WHEN** a process is crashed while a write is outstanding, across many seeds
- **THEN** some runs recover the new state and some recover the old, and both are legitimate

#### Scenario: A completed write always survives

- **WHEN** a write has completed before the crash
- **THEN** the recovering process retrieves it in every run

#### Scenario: A partially written value is never retrieved

- **WHEN** a crash interrupts a write
- **THEN** what is retrieved is either the whole new value or the whole previous one, never a
  mixture

### Requirement: Storage activity is visible in the trace

Writes, their completion, and what a process retrieved on recovery SHALL appear in the trace, so
that properties about durability can be asserted over the trace rather than over protocol
internals.

#### Scenario: A durability property is assertable without touching internals

- **WHEN** a test needs to establish that something was durable before a message was sent
- **THEN** it can determine that from the trace alone
