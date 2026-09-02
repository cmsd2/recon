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
duplication, reordering, delivery delay, and severed connectivity between processes.
Under any configuration that does not permanently drop all messages between two correct processes,
a message retransmitted infinitely often SHALL eventually be delivered.

Connectivity SHALL be a property of a **pair** of processes rather than of a grouping, so that
reachability need not be transitive: a network in which `A` reaches `B` and `B` reaches `C` while `A`
does not reach `C` SHALL be expressible. Partitioning into groups SHALL remain available and SHALL
mean severing every pair that spans two groups.

A test SHALL be able to ask whether two processes can currently reach each other, so that the
topology it built can be asserted rather than assumed.

#### Scenario: Messages are dropped at the configured rate

- **WHEN** a run is configured with a non-zero loss rate and many messages are sent
- **THEN** the trace records losses, and the observed rate is consistent with the configuration

#### Scenario: A partition prevents delivery

- **WHEN** two processes are placed in disjoint partitions
- **THEN** no message sent between them is delivered while the partition holds

#### Scenario: A healed partition permits delivery again

- **WHEN** a partition is removed and a message is retransmitted afterward
- **THEN** delivery becomes possible again

#### Scenario: A severed pair prevents delivery in both directions

- **WHEN** connectivity between two processes is severed
- **THEN** no message between them is delivered in either direction while it stays severed, and
  messages to and from every other process are unaffected

#### Scenario: Reachability need not be transitive

- **WHEN** connectivity is severed between two processes but each still reaches a third
- **THEN** messages between each of them and the third are delivered, and messages between the two
  are not

#### Scenario: A test can ask what is reachable

- **WHEN** a test builds a topology
- **THEN** it can ask whether any two processes reach each other, and the answer reflects every
  severing and healing applied so far

#### Scenario: Retransmission overcomes loss

- **WHEN** a correct process retransmits a message indefinitely to a correct process over a lossy
  but not partitioned network
- **THEN** the message is eventually delivered

### Requirement: Every run produces an inspectable trace

The simulator SHALL record a trace containing, in order, each message sent, each delivery
outcome including drops and duplicates, each timer fired, and each indication raised, with the
virtual time and originating process for each entry.

A timer entry SHALL carry the handle of the timer that fired, so that a claim about *which* timer
fired can be settled from the trace rather than from protocol internals. The trace SHALL NOT be
parameterised by a timer type, having none to be parameterised by.

#### Scenario: Properties are asserted over the trace

- **WHEN** a run completes
- **THEN** the trace can be examined to decide whether a stated property held, without inspecting
  protocol internals

#### Scenario: Fault injection is visible in the trace

- **WHEN** a run is configured to inject faults
- **THEN** the trace distinguishes messages that were dropped or duplicated from those delivered
  normally

#### Scenario: Which timer fired is visible in the trace

- **WHEN** two layers of one process each have a timer outstanding and one of them fires
- **THEN** the trace names which, by the handle the registering layer was given

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

A write SHALL be durable when it returns: once a write call has returned, no subsequent crash can
take it. A synchronous state machine offers no later point at which a driver could wait on a
protocol's behalf, so a process must not be able to be seen to have made a promise it has no record
of.

The simulator SHALL therefore be able to kill a process *inside* a write, on request. When that
happens the write SHALL have taken effect or not, chosen by the seeded source; any further write in
the same handler SHALL NOT take effect; everything the handler went on to emit SHALL be discarded;
and the recovering process SHALL have no way to tell which case it is in beyond reading what
survived.

This is the fault that finds the bugs. An algorithm that is correct only when writes are atomic and
never interrupted is not correct.

#### Scenario: An outstanding write may or may not survive

- **WHEN** a process is armed to die in its next write and then writes, across many seeds
- **THEN** some runs recover the new state and some recover the old, and both are legitimate

#### Scenario: An outstanding append may or may not survive

- **WHEN** a process is armed to die in its next write and that write is an append, across many
  seeds
- **THEN** some runs read the entry back and some do not, and both are legitimate

#### Scenario: A completed write always survives

- **WHEN** a write has returned before the crash
- **THEN** the recovering process reads it in every run, whatever the seed

#### Scenario: A partially written value is never retrieved

- **WHEN** a crash interrupts a write
- **THEN** what is read is either the whole new value or the whole previous one, never a mixture

#### Scenario: A crash never leaves a gap in the entries

- **WHEN** a crash interrupts a sequence of appends
- **THEN** what survives is a prefix of what was appended, never a sequence with a hole in it

#### Scenario: Nothing decided on an interrupted write escapes the process

- **WHEN** a process is killed inside a write and its handler went on to send a message
- **THEN** no peer receives that message

### Requirement: Storage activity is visible in the trace

Writes, appends, deaths inside a write, and whether a recovering process found anything SHALL
appear in the trace, so that properties about durability can be asserted over the trace rather than
over protocol internals.

#### Scenario: A durability property is assertable without touching internals

- **WHEN** a test needs to establish that something was durable before a message was sent
- **THEN** it can determine that from the trace alone

#### Scenario: Dying inside a write is visible, and its outcome is not

- **WHEN** a process is killed inside a write
- **THEN** the trace records that it died writing, separately from the writes that completed, and
  does NOT record whether that write landed — the recovering process reading what survived is the
  only evidence either way

#### Scenario: Rewriting and appending are distinguishable

- **WHEN** a run both replaces metadata and appends entries
- **THEN** the trace shows which happened, so that a claim about a protocol's write cost can be
  checked rather than asserted

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

### Requirement: The run owns the source of timer identities

The simulator SHALL supply one source of timer identities per run and SHALL pass it to every
protocol it drives, so that identities are distinct across every layer of every process in that run.

A source owned per protocol, or begun afresh for each event, would hand two layers the same identity
and let each accept the other's expiry. Owning it at the run is what makes the guarantee that
identities do not collide something the driver provides rather than something each protocol must
arrange.

#### Scenario: Identities do not collide across a composition

- **WHEN** several layers of one process each register a timer during a run
- **THEN** every handle is distinct

#### Scenario: A run remains reproducible from its seed

- **WHEN** the same seed and configuration are run twice, with timers registered and fired
- **THEN** the two traces are identical, including which handle each timer entry names

#### Scenario: An expiry is delivered to the process that registered it

- **WHEN** a timer registered by one process fires
- **THEN** it is delivered to that process, and to no other


### Requirement: A run can be described as a value and executed from it

A run SHALL be expressible as data — a configuration and its seed, a membership, a sequence of steps
each with the time it occurs, and a horizon to run to — and the simulator SHALL execute such a
description. Every fault and command the simulator accepts imperatively SHALL be expressible as a
step.

Executing the same description SHALL produce the same run, by the determinism this capability
already requires.

#### Scenario: A described run and an imperative one agree

- **WHEN** the same commands and faults are applied at the same times, once by calling the
  simulator's methods and once by executing a description
- **THEN** the two runs produce the same trace

#### Scenario: A description executes identically every time

- **WHEN** one description is executed twice
- **THEN** the two runs produce the same trace

### Requirement: A failing scenario can be reduced to a smaller one that still fails

Given a scenario and a predicate over the run it produces, the simulator SHALL search for a smaller
scenario whose run still satisfies that predicate, and SHALL return one from which no further
reduction it attempts can be made.

Smaller SHALL mean fewer steps, a shorter horizon, a smaller membership, or a simpler fault — the
reductions that make a counterexample readable.

The result SHALL satisfy the predicate. A reduction that is not re-run and re-checked is a guess,
and returning a scenario that does not fail would be worse than returning the original.

The search SHALL itself be deterministic, so that a reduction can be reproduced and reported.

#### Scenario: A scenario with irrelevant steps loses them

- **WHEN** a scenario contains faults and commands that the predicate does not depend on
- **THEN** the returned scenario no longer contains them

#### Scenario: The horizon shrinks to when the predicate first holds

- **WHEN** a scenario runs far beyond the point at which its predicate becomes true
- **THEN** the returned scenario's horizon is near that point rather than the original

#### Scenario: What is returned still fails

- **WHEN** a scenario is reduced
- **THEN** running the returned scenario satisfies the predicate

#### Scenario: Reducing twice gives the same answer

- **WHEN** the same scenario and predicate are reduced twice
- **THEN** the two results are identical

#### Scenario: An already-minimal scenario is returned unchanged

- **WHEN** no reduction the search attempts leaves the predicate holding
- **THEN** the original scenario is returned, rather than something that no longer fails

### Requirement: A reduced scenario is reported as something that can be run again

The reduced scenario SHALL be renderable as source that reconstructs it, so that the end of a
reduction is a test rather than a description to transcribe by hand.

#### Scenario: The rendering reconstructs the scenario

- **WHEN** a scenario is rendered and the rendering is compiled and executed
- **THEN** it produces the same run as the scenario it was rendered from

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
