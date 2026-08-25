## ADDED Requirements

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
