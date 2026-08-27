## ADDED Requirements

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
