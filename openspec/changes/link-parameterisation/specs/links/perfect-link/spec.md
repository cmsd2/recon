## ADDED Requirements

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
