# links/session-link Specification

## Purpose

A link whose reliability comes from an underlying session rather than from retransmitting until
acknowledged. It is what would be deployed over TCP or QUIC, and unlike the perfect link it does
not pretend that reconnection is invisible.

## Requirements

### Requirement: Reliable ordered delivery within a session


While a session with a peer holds, the link SHALL deliver every message sent to that peer, in the
order sent, exactly once.

#### Scenario: Messages arrive in order

- **WHEN** several messages are sent to a peer while a session holds
- **THEN** all are delivered to the layer above, in the order they were sent

#### Scenario: No duplication within a session

- **WHEN** a session holds
- **THEN** no message is delivered to the layer above more than once

### Requirement: A session ending is reported, not concealed


When a session with a peer ends, the link SHALL report it to the layer above, identifying the peer
and **the epoch that ended**. It SHALL NOT present the resulting gap as ordinary delivery, and
SHALL NOT claim that messages sent before the ending were delivered.

The epoch reported is the one that ended, not a prediction of the next. At the moment of failure
the next epoch is not yet a fact, and may never become one.

#### Scenario: The layer above is told

- **WHEN** a session with a peer ends
- **THEN** the layer above receives a report naming that peer and the epoch that ended

#### Scenario: A lost suffix is not concealed

- **WHEN** messages are in flight and the session ends
- **THEN** the layer above is not told those messages were delivered

#### Scenario: Delivery resumes in the new session

- **WHEN** a session has ended and a new one is established
- **THEN** messages sent afterwards are delivered normally

### Requirement: A session establishment is reported


When a session with a peer is established, the link SHALL report it to the layer above, naming the
peer and the epoch now in force.

An ending tells the layer above that something may have been lost; an establishment tells it that
the peer can be reached again. Only the second is actionable, and a layer that must resend
anything can only do so on the second.

#### Scenario: The layer above is told

- **WHEN** a session with a peer is established
- **THEN** the layer above receives a report naming that peer and the epoch now in force

#### Scenario: A response reaches the peer

- **WHEN** the layer above sends to a peer on being told a session with it was established
- **THEN** that message is delivered

#### Scenario: The two reports are distinguishable

- **WHEN** a session ends and a later one is established
- **THEN** the layer above receives two distinct reports, and can tell which is which

### Requirement: The guarantee is scoped to the session


The link's reliable-ordered-delivery guarantee SHALL be stated as holding within a session, not
across one. Across a session boundary the link SHALL make no claim about messages that had not yet
been delivered.

#### Scenario: No claim spans a boundary

- **WHEN** a run contains a session ending
- **THEN** every message delivered was sent within the session in which it was delivered

### Requirement: No creation


The link SHALL deliver a message only if that message was previously sent to it by the process
named as its sender.

#### Scenario: Deliveries match sends

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier send by the named sender

### Requirement: State is bounded by membership


The link SHALL hold state proportional to the number of peers, not to the number of messages
handled. It SHALL NOT retain a record of every message sent or delivered.

#### Scenario: State does not grow with messages

- **WHEN** a growing number of messages is sent over the link
- **THEN** the link's state does not grow with them

### Requirement: The session link satisfies the link port in its scope-reporting form


The session link SHALL satisfy the same link port as the perfect link, and SHALL additionally
report the boundaries of the sessions carrying its messages, so that a layer above can repair what
a session ending lost.

Satisfying the same port is what allows one implementation of a broadcast to run over either link.
The extra reporting SHALL be visible in the type, so that a layer requiring it cannot be composed
over a link that cannot provide it.

#### Scenario: One broadcast implementation runs over either link

- **WHEN** a broadcast written against the port is composed over the session link, and separately
  over the perfect link
- **THEN** both build, and the broadcast is implemented once

#### Scenario: The extra reporting is part of the type

- **WHEN** a layer that repairs a session ending is composed over the session link
- **THEN** the project builds
- **AND** composing the same layer over a link that reports no boundary is rejected when the
  project is built

