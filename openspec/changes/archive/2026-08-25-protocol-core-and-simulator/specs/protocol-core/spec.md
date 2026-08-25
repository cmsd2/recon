## Purpose

Defines the contract every protocol in this project obeys: a synchronous state machine that
consumes events and emits effects, with no access to wall-clock time, ambient randomness, or
input/output. This is what makes protocols reproducible under simulation and testable without a
runtime.

## ADDED Requirements

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
sending a message to a named peer, raising an indication to the layer above, and requesting a
timer. A protocol MUST NOT send, deliver, or schedule by any other means.

#### Scenario: A protocol transmits to a peer

- **WHEN** a protocol's logic requires transmitting to another process
- **THEN** it emits a send effect naming the destination and the message, and performs no
  transmission itself

#### Scenario: A protocol reports to its caller

- **WHEN** a protocol's logic satisfies a guarantee owed to the layer above
- **THEN** it emits an indication effect, and does not call into that layer directly

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
