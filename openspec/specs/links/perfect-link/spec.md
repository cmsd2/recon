# links/perfect-link Specification

## Purpose

Provides the reliable point-to-point delivery that the broadcast abstractions are built on: a
message sent between correct processes arrives exactly once, and nothing arrives that was never
sent. Built by suppressing the duplicates that the stubborn link produces.

## Requirements

### Requirement: Reliable delivery


If a correct process sends a message to a correct process, the recipient SHALL eventually deliver
that message.

#### Scenario: A message arrives despite loss

- **WHEN** a correct process sends a message to a correct process over a lossy network
- **THEN** the recipient eventually delivers it

#### Scenario: Many messages all arrive

- **WHEN** a correct process sends many messages to a correct process over a lossy network
- **THEN** the recipient eventually delivers every one of them

### Requirement: No duplication


The link SHALL deliver each message to the layer above at most once, regardless of how many times
it is received from the network.

#### Scenario: Network duplication is suppressed

- **WHEN** the network is configured to duplicate messages and a message is sent once
- **THEN** the recipient delivers it exactly once to the layer above

#### Scenario: Retransmissions are suppressed

- **WHEN** the underlying link retransmits a message many times
- **THEN** the recipient delivers it exactly once to the layer above

### Requirement: No creation


A message SHALL be delivered only if it was previously sent by the named sender.

#### Scenario: Deliveries match sends

- **WHEN** a run completes
- **THEN** every delivery in the trace corresponds to an earlier send by the named sender, and the
  set of messages delivered is a subset of those sent

### Requirement: Distinct messages are distinguished


Two messages that are equal in content but sent separately SHALL each be delivered. Suppression of
duplicates MUST NOT suppress a genuine resend by the layer above.

#### Scenario: The same content is sent twice

- **WHEN** the layer above sends two messages with identical content as separate sends
- **THEN** the recipient delivers two messages to its layer above

### Requirement: The perfect link satisfies the link port


The perfect link SHALL satisfy the link port, so that any layer written against the port composes
over it without naming it.

It reports no scope boundary: its guarantees hold for as long as the process does, so there is no
ending for it to raise and it SHALL NOT declare one.

#### Scenario: A layer written against the port composes over it

- **WHEN** a layer that names only the port is composed over the perfect link
- **THEN** the project builds, and the layer's guarantees hold as this link's own allow

#### Scenario: It declares no scope it cannot observe

- **WHEN** the perfect link's declaration is read
- **THEN** it names no scope boundary, and a caller cannot construct one for it

