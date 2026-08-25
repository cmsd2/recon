## MODIFIED Requirements

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

## ADDED Requirements

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
