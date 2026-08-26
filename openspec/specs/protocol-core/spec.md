# protocol-core Specification

## Purpose

Defines the contract every protocol in this project obeys: a synchronous state machine that
consumes events and emits effects, with no access to wall-clock time, ambient randomness, or
input/output. This is what makes protocols reproducible under simulation and testable without a
runtime.

## Requirements

### Requirement: Protocols are deterministic functions of state and event

A protocol SHALL produce identical effects and identical resulting state whenever it is given
identical prior state and an identical event, including the values supplied for time and
randomness. A protocol MUST NOT read wall-clock time, draw randomness from ambient sources, or
perform input/output.

#### Scenario: Identical event sequences produce identical effects

- **WHEN** two instances of the same protocol start from equal initial state and are given the
  same sequence of events, with the same time and randomness values supplied
- **THEN** both instances emit the same effects in the same order and end in equal state

#### Scenario: Handling an event completes without suspension

- **WHEN** a protocol is given any event
- **THEN** handling completes before control returns, with no intermediate state observable by any
  other party, and the resulting state reflects either all or none of that event's transition

### Requirement: Effects are the only means of affecting the world

A protocol SHALL express every outward action as an effect. The available effects SHALL be:
sending a message to a named peer, raising an indication to the layer above, requesting a timer,
and writing its durable state to stable storage. A protocol MUST NOT send, deliver, schedule, or
persist by any other means.

#### Scenario: A protocol transmits to a peer

- **WHEN** a protocol's logic requires transmitting to another process
- **THEN** it emits a send effect naming the destination and the message, and performs no
  transmission itself

#### Scenario: A protocol reports to its caller

- **WHEN** a protocol's logic satisfies a guarantee owed to the layer above
- **THEN** it emits an indication effect, and does not call into that layer directly

#### Scenario: A protocol records something that must survive a crash

- **WHEN** a protocol's logic requires state to outlive the current process incarnation
- **THEN** it emits a store effect carrying that state, and performs no write itself

### Requirement: Time and randomness are supplied to the protocol

Current time and any random values a protocol requires SHALL be supplied through the same
parameter that receives its effects. Time SHALL be monotonic and expressed in a project-defined
type that can be assigned an arbitrary value.

#### Scenario: A run is replayed with a different clock

- **WHEN** the same protocol is driven once with simulated time and once with real time, given
  the same event sequence and time values
- **THEN** it emits the same effects in both cases

#### Scenario: Randomised choice is reproducible

- **WHEN** a protocol that makes a randomised choice is driven twice with the same seeded source
- **THEN** it makes the same choice both times

### Requirement: A parent composes a child by owning it and re-wrapping its effects

A protocol that is built on another SHALL own that child directly, and SHALL translate each effect
the child emits into its own terms before that effect leaves the parent. Composition MUST NOT
depend on names, identifiers, or registries resolved while running.

#### Scenario: A child's outgoing message is re-wrapped

- **WHEN** a child emits a send effect
- **THEN** the parent emits a corresponding send effect carrying the child's message wrapped in
  the parent's own message type

#### Scenario: A child's indication is consumed by the parent

- **WHEN** a child emits an indication
- **THEN** the parent handles it as an input to its own logic, and emits its own indication only
  where its guarantees require one

#### Scenario: A mis-wired composition is rejected before running

- **WHEN** a parent is written to pass a message of the wrong type to a child
- **THEN** the error is detected when the project is built, not by observing an undelivered message

### Requirement: Message payloads are carried as typed values and encoded once

A protocol stack SHALL pass message payloads between layers as typed values. Encoding to bytes
SHALL happen exactly once, at the boundary where messages leave the process, and no intermediate
encoded or type-erased representation SHALL be constructed at any layer boundary.

#### Scenario: A message crosses several layers

- **WHEN** a message passes down through every layer of a composed stack and is transmitted
- **THEN** it is encoded exactly once, and decoded exactly once on receipt

### Requirement: Failures are reported as distinct typed causes

Each layer SHALL report its failures as its own error type, preserving the originating cause.
Errors MUST NOT be flattened into a general-purpose input/output error or reduced to a message
string.

#### Scenario: A decoding failure is surfaced

- **WHEN** a message fails to decode at the wire boundary
- **THEN** the reported error identifies decoding as the cause and retains the underlying detail

### Requirement: A protocol may declare scopes its guarantees depend on

A protocol SHALL declare whether its guarantees are bounded by any scope whose ending it can
observe, and SHALL handle such an ending when one occurs. A protocol with no such scope SHALL be
able to say so in a way that makes an ending impossible to construct, and SHALL NOT be required to
write a handler for one.

#### Scenario: A protocol with no scopes writes no handler

- **WHEN** a protocol declares that it has no scope conditions
- **THEN** it compiles without a scope handler, and no scope ending can be constructed for it

#### Scenario: A protocol with a scope handles its ending

- **WHEN** a protocol declares a scope and that scope ends
- **THEN** the protocol's scope handler is invoked with a description of what ended

#### Scenario: A parent that bridges a child's scope ending absorbs it

- **WHEN** a child's scope ends and the parent restores the guarantee itself
- **THEN** the parent raises nothing to the layer above about that ending

#### Scenario: A parent that cannot bridge propagates

- **WHEN** a child's scope ends and the parent cannot restore the guarantee
- **THEN** the parent reports an ending of its own to the layer above, in its own terms

### Requirement: A protocol declares what it keeps durably

A protocol SHALL declare the type of its durable state, and that type SHALL be distinct from the
protocol's own state. A protocol that keeps nothing durably SHALL be able to say so in a way that
makes a store effect impossible to construct for it.

#### Scenario: A protocol with no durable state cannot emit a store

- **WHEN** a protocol declares that it keeps nothing durably
- **THEN** no store effect can be constructed for it, and this is enforced when the code is built
  rather than when it runs

#### Scenario: A storing child cannot be composed

- **WHEN** a protocol attempts to compose a child that declares durable state of its own
- **THEN** it fails to build, because no mapping from the child's durable state into the parent's
  can be written — a parent's durable state contains its own fields as well as its child's

#### Scenario: What is durable is visible in the interface

- **WHEN** a reader asks what a protocol would still know after a crash
- **THEN** the answer is the declared durable type, not a convention about which fields are
  written

### Requirement: Startup is a branch, and exactly one side runs

A protocol SHALL have two startup entry points — initialisation and recovery — of which **exactly
one** runs. A process with nothing in storage is initialised; a process with something is given it
and recovered. Both SHALL be able to emit effects.

The constructor cannot serve as either. It runs in both cases, so it is the common prefix of the
branch rather than one side of it, and it cannot emit effects, so first-start work that must be
*done* rather than merely set up has nowhere to happen. Writing an initial value down is the
standard case: repeating it on recovery would overwrite exactly what was being recovered.

#### Scenario: A restarted protocol is told what survived

- **WHEN** a process crashes and restarts, having written durable state before the crash
- **THEN** the protocol is given that state on recovery, and its volatile state is empty

#### Scenario: A first start is initialised, not recovered

- **WHEN** a process starts with nothing in storage
- **THEN** its initialisation entry point runs and its recovery entry point does not

#### Scenario: A restart is recovered, not initialised

- **WHEN** a process restarts with something in storage
- **THEN** its recovery entry point runs and its initialisation entry point does not

#### Scenario: A first start can write something down

- **WHEN** a protocol's first act must be durable, so that a later restart recovers rather than
  beginning again
- **THEN** it emits that store during initialisation, and does not repeat it on recovery

#### Scenario: Recovering can produce effects

- **WHEN** a protocol recovers and its algorithm requires it to notify the layer above or to
  re-send what was pending
- **THEN** it emits those effects during recovery, exactly as it would for any other event

### Requirement: A write completes before anything that depends on it is sent

Effects emitted from one event SHALL be performed in an order that makes every store effect
durable before **any effect emitted after it** takes visible effect — sends leaving the process,
and indications reaching the layer above. A protocol MAY therefore emit a store and a send from
the same event and rely on the write having taken effect first.

Without this rule a protocol can be observed by its peers to have made a promise it has no record
of, which is the failure the fail-recovery model exists to prevent. Indications are held for the
same reason at one remove: an indication is how the layer above learns something, and what it
usually does next is send.

#### Scenario: A promise is durable before it is made

- **WHEN** a protocol emits a store effect and then a send effect in response to one event
- **THEN** the write is durable before the message leaves the process

#### Scenario: The layer above is not told before the write lands

- **WHEN** a protocol emits a store effect and then an indication in response to one event
- **THEN** the write is durable before the layer above is notified

#### Scenario: A crash between the two loses the message, not the record

- **WHEN** a process crashes after the store and before the send
- **THEN** on recovery the stored state is present, and the message was never sent
