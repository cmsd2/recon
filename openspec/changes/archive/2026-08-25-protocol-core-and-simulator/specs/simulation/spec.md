## Purpose

Provides the deterministic execution environment in which protocols are run and judged: a virtual
clock, seeded randomness, a scheduled delivery queue with configurable faults, and a recorded
trace of everything that happened. This is the project's standard of evidence, replacing the
reading of log files.

## ADDED Requirements

### Requirement: A run is fully determined by its seed and configuration

The simulator SHALL produce an identical delivery trace for identical seed and configuration. All
scheduling decisions, fault decisions, and randomness supplied to protocols SHALL derive from that
seed. Iteration over internal collections MUST NOT introduce ordering that varies between runs.

#### Scenario: A run is repeated

- **WHEN** the same scenario is run twice with the same seed and configuration
- **THEN** both runs produce identical traces

#### Scenario: A failure is reproduced from its seed

- **WHEN** a run violates an asserted property and reports its seed
- **THEN** re-running with that seed reproduces the same violation

#### Scenario: Differing seeds explore differing schedules

- **WHEN** the same scenario is run with two different seeds
- **THEN** the traces are permitted to differ, and each remains reproducible from its own seed

### Requirement: Time is virtual and advances only through scheduled events

The simulator SHALL maintain a monotonic virtual clock that advances to the timestamp of the next
scheduled event. A run's duration in wall-clock terms SHALL be independent of the simulated
durations within it.

#### Scenario: A long delay costs no real time

- **WHEN** a protocol requests a timer far in the simulated future and no other event is pending
- **THEN** the clock advances directly to that timer's due time without any real waiting

#### Scenario: Simultaneous events are ordered deterministically

- **WHEN** two events are scheduled at the same virtual time
- **THEN** they are processed in an order that is the same on every run of that seed

### Requirement: The network provides fair-loss semantics with configurable faults

The simulator SHALL act as the fair-loss link layer. It SHALL support configuring message loss,
duplication, reordering, delivery delay, and network partitions between named groups of processes.
Under any configuration that does not permanently drop all messages between two correct processes,
a message retransmitted infinitely often SHALL eventually be delivered.

#### Scenario: Messages are dropped at the configured rate

- **WHEN** a run is configured with a non-zero loss rate and many messages are sent
- **THEN** the trace records losses, and the observed rate is consistent with the configuration

#### Scenario: A partition prevents delivery

- **WHEN** two processes are placed in disjoint partitions
- **THEN** no message sent between them is delivered while the partition holds

#### Scenario: A healed partition permits delivery again

- **WHEN** a partition is removed and a message is retransmitted afterward
- **THEN** delivery becomes possible again

#### Scenario: Retransmission overcomes loss

- **WHEN** a correct process retransmits a message indefinitely to a correct process over a lossy
  but not partitioned network
- **THEN** the message is eventually delivered

### Requirement: Every run produces an inspectable trace

The simulator SHALL record a trace containing, in order, each message sent, each delivery
outcome including drops and duplicates, each timer fired, and each indication raised, with the
virtual time and originating process for each entry.

#### Scenario: Properties are asserted over the trace

- **WHEN** a run completes
- **THEN** the trace can be examined to decide whether a stated property held, without inspecting
  protocol internals

#### Scenario: Fault injection is visible in the trace

- **WHEN** a run is configured to inject faults
- **THEN** the trace distinguishes messages that were dropped or duplicated from those delivered
  normally

### Requirement: Multiple processes run within a single test process

The simulator SHALL run a configured set of named processes within one operating-system process
and one thread, with no sockets opened and no network interfaces used.

#### Scenario: A cluster runs in a test

- **WHEN** a scenario configures several processes and runs to completion
- **THEN** it executes inside the test process, opening no sockets

### Requirement: A crash loses volatile state

The simulator SHALL model a crash as the loss of a process's volatile state. A process that
crashes and later restarts SHALL resume with freshly initialised state and no pending timers,
having forgotten everything it held in memory.

The simulator SHALL additionally offer suspension, which stops a process from handling events
while preserving its state, for scenarios that require a pause rather than a crash.

#### Scenario: A restarted process has forgotten what it delivered

- **WHEN** a process crashes after delivering a message, is restarted, and the same message is
  delivered to it again
- **THEN** it delivers that message to the layer above a second time, because the record of the
  first delivery did not survive

#### Scenario: A crashed process loses its pending timers

- **WHEN** a process sets a timer, crashes before it fires, and is restarted
- **THEN** that timer does not fire

#### Scenario: A suspended process resumes with its state intact

- **WHEN** a process is suspended and later resumed
- **THEN** it continues with the state it had, and its pending timers still fire

### Requirement: Encoding can be exercised on demand

The simulator SHALL move message payloads between processes as typed values by default. It SHALL
offer a mode in which every delivered message is round-tripped through the wire encoding, so that
encoding defects can be detected without being incurred on every run.

#### Scenario: Codec checking is enabled

- **WHEN** a run is configured to check encoding and a message type fails to round-trip
- **THEN** the run reports the failure and identifies the offending message

#### Scenario: Codec checking is disabled

- **WHEN** a run is configured normally
- **THEN** no encoding or decoding is performed for deliveries within the simulation
