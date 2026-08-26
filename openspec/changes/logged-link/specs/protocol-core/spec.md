## MODIFIED Requirements

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

## ADDED Requirements

### Requirement: A protocol declares what it keeps durably

A protocol SHALL declare the type of its durable state, and that type SHALL be distinct from the
protocol's own state. A protocol that keeps nothing durably SHALL be able to say so in a way that
makes a store effect impossible to construct for it.

#### Scenario: A protocol with no durable state cannot emit a store

- **WHEN** a protocol declares that it keeps nothing durably
- **THEN** no store effect can be constructed for it, and this is enforced when the code is built
  rather than when it runs

#### Scenario: What is durable is visible in the interface

- **WHEN** a reader asks what a protocol would still know after a crash
- **THEN** the answer is the declared durable type, not a convention about which fields are
  written

### Requirement: Recovery is an event, distinct from initialisation

A process that restarts SHALL be given its retrieved durable state through a recovery entry point
distinct from construction, so that a protocol can act on recovering — re-indicating what it
already log-delivered, or re-sending what is still pending — rather than merely existing again.

#### Scenario: A restarted protocol is told what survived

- **WHEN** a process crashes and restarts, having written durable state before the crash
- **THEN** the protocol is given that state on recovery, and its volatile state is empty

#### Scenario: A first start is distinguishable from a recovery

- **WHEN** a process starts for the first time, with nothing in storage
- **THEN** it is initialised rather than recovered, and can tell the difference

#### Scenario: Recovering can produce effects

- **WHEN** a protocol recovers and its algorithm requires it to notify the layer above or to
  re-send what was pending
- **THEN** it emits those effects during recovery, exactly as it would for any other event

### Requirement: A write completes before anything that depends on it is sent

Effects emitted from one event SHALL be performed in an order that makes every store effect
durable before any send effect emitted after it leaves the process. A protocol MAY therefore emit
a store and a send from the same event and rely on the write having taken effect first.

Without this rule a protocol can be observed by its peers to have made a promise it has no record
of, which is the failure the fail-recovery model exists to prevent.

#### Scenario: A promise is durable before it is made

- **WHEN** a protocol emits a store effect and then a send effect in response to one event
- **THEN** the write is durable before the message leaves the process

#### Scenario: A crash between the two loses the message, not the record

- **WHEN** a process crashes after the store and before the send
- **THEN** on recovery the stored state is present, and the message was never sent
