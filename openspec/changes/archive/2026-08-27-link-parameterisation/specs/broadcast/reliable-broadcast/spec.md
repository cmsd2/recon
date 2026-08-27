## ADDED Requirements

### Requirement: The broadcast beneath is a parameter

This protocol SHALL be written against the port of the broadcast beneath it and SHALL NOT name an
implementation. It composes over any broadcast satisfying that port.

Its agreement is bounded by whatever the layer beneath can carry: over a link that never loses a
relay between correct processes it holds outright, and over one whose guarantees lapse at a scope
boundary it holds within a scope. That difference is a property of what is supplied, not of two
different reliable broadcasts.

#### Scenario: The ordinary stack is unchanged

- **WHEN** this protocol is used without naming the layer beneath
- **THEN** it composes as it did before this change

#### Scenario: The same implementation runs over a session-carrying stack

- **WHEN** it is composed over a broadcast whose link reports scope boundaries
- **THEN** agreement holds within the scopes carrying the relay, and there is one implementation
  rather than two
