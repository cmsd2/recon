# simulation Specification

## Purpose

Provides the deterministic execution environment in which protocols are run and judged: a virtual
clock, seeded randomness, a scheduled delivery queue with configurable faults, and a recorded
trace of everything that happened. This is the project's standard of evidence, replacing the
reading of log files.

## Requirements

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

### Requirement: A synchronous mode with a bounded delivery delay

The simulator SHALL offer a mode in which every message between connected, uncrashed processes is
delivered within a known upper bound and none is lost. The bound SHALL be readable by a test, so
that a protocol depending on it can be configured consistently with the network it runs on.

This mode is additional. The fair-loss behaviour remains the default, and the existing
configuration of loss, duplication, reordering and latency is unchanged.

#### Scenario: Delivery within the bound

- **WHEN** a run is configured synchronous with bound Δ and a message is sent between two
  connected, uncrashed processes
- **THEN** it is delivered, and the delay between sending and delivery does not exceed Δ

#### Scenario: No loss in synchronous mode

- **WHEN** a run is configured synchronous
- **THEN** no message between connected, uncrashed processes is dropped

#### Scenario: Crashes and partitions still apply

- **WHEN** a run is configured synchronous and a process is crashed, or two processes are
  partitioned
- **THEN** messages to the crashed process and across the partition are still not delivered, so
  the mode constrains timing without removing failures

#### Scenario: The default remains asynchronous

- **WHEN** a run is configured without requesting synchronous mode
- **THEN** loss, duplication and latency behave exactly as before

### Requirement: A session network model

The simulator SHALL offer a mode in which communication between each pair of processes takes place
within a session. While a session holds, messages between connected, uncrashed processes SHALL be
delivered reliably, in the order sent, and without duplication. This mode is additional; the
fair-loss behaviour remains the default and is unchanged.

#### Scenario: Delivery within a session is reliable and ordered

- **WHEN** a run is session-based and one process sends several messages to another while their
  session holds
- **THEN** all of them are delivered, in the order they were sent, each exactly once

#### Scenario: The fair-loss default is unaffected

- **WHEN** a run is configured without requesting sessions
- **THEN** loss, duplication and reordering behave exactly as before

### Requirement: A session ends on disruption, losing an unknown suffix

A session SHALL end when the processes are partitioned, when either crashes, or when a break is
requested explicitly. On ending, an unknown suffix of the messages in flight SHALL be discarded,
and a new session SHALL begin at a higher epoch once communication is possible again.

#### Scenario: A break discards messages in flight

- **WHEN** messages are in flight between two processes and their session is broken
- **THEN** some suffix of those messages is never delivered

#### Scenario: A partition ends the session

- **WHEN** two processes with an established session are partitioned
- **THEN** their session ends

#### Scenario: A new session begins at a higher epoch

- **WHEN** a session between two processes ends and communication becomes possible again
- **THEN** a new session is established, and its epoch is greater than the previous one

#### Scenario: Ordering restarts with the new session

- **WHEN** a session ends and a new one is established
- **THEN** messages sent in the new session are delivered in their own order, independently of
  anything lost from the old one

### Requirement: Session events are visible in the trace

The simulator SHALL record session establishment, session ends and suffix losses in the trace, so
that a property can be asserted over them without inspecting protocol state.

#### Scenario: A session end is recorded

- **WHEN** a session ends for any reason
- **THEN** the trace records it, with the processes involved and the epoch that ended

#### Scenario: Discarded messages are distinguishable from delivered ones

- **WHEN** a session ends with messages in flight
- **THEN** the trace distinguishes those discarded by the session ending from those delivered

### Requirement: A session is re-established without being prompted

Once communication with a peer becomes possible again, the simulator SHALL establish a session with
it without waiting for either process to send. It MAY delay before doing so, modelling a link that
retries with backoff.

This models the link a deployment would have: one that keeps trying to reconnect on its own,
reporting its epoch and connected status upward. It matters because the alternative — establishing
lazily, when something happens to transmit — makes reconnection depend on the state of the layers
above, which neither end controls and which may be silent indefinitely.

#### Scenario: A healed partition reconnects on its own

- **WHEN** a session ends because of a partition, the partition heals, and no process sends
  anything
- **THEN** a session is established anyway, and both processes are told

#### Scenario: A restarted process reconnects on its own

- **WHEN** a session ends because a process crashed, and that process restarts
- **THEN** a session is established without either process being prompted

#### Scenario: An unreachable peer is retried, not abandoned

- **WHEN** a peer remains unreachable
- **THEN** the simulator continues to attempt establishment, and establishes one as soon as it
  becomes possible

### Requirement: A session establishment is reported to the processes

When a session is established with a peer — whether prompted by this process sending, by the peer
sending, or by any other traffic — the simulator SHALL report it to both processes, naming the
epoch now in force.

This is a distinct event from the ending, and the two are not interchangeable. An ending is known
at the moment of failure but cannot be acted on, the peer being unreachable. An establishment is
what a protocol can act on, and it happens at a moment neither end fully controls: it may be
provoked by a heartbeat, by an application send, or by the peer connecting inward.

#### Scenario: Establishment is reported when a session opens

- **WHEN** a session is established with a peer
- **THEN** both processes are told, naming the epoch now in force

#### Scenario: A message sent in response is delivered

- **WHEN** a process sends to a peer on being told a session with it was established
- **THEN** that message is delivered, the session being in force

#### Scenario: A peer that never returns is never reported established

- **WHEN** a session ends and no later session is established with that peer
- **THEN** no establishment is reported for it, and a process must learn of its absence by other
  means

#### Scenario: Nothing above need provoke it

- **WHEN** a session becomes possible again and neither process sends
- **THEN** it is established and both processes are told, because reconnection is the link's own
  business and not the business of the layers above

### Requirement: Stable storage survives a crash

The simulator SHALL provide each process with storage that survives a crash and a restart, while
volatile state does not. Storage SHALL be part of the run's deterministic state, so that a seed
reproduces a run including everything recovered from it.

#### Scenario: What was written is retrieved after a restart

- **WHEN** a process writes durable state, crashes, and restarts
- **THEN** it is given back what it wrote, and nothing it held only in memory

#### Scenario: A process that never wrote is initialised again rather than recovered

- **WHEN** a process crashes having written nothing
- **THEN** its initialisation entry point runs, not its recovery one

#### Scenario: Every process is initialised at the start of a run

- **WHEN** a run begins
- **THEN** each process's initialisation entry point runs once, and any effects it emits are
  interpreted like those of any other event

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
