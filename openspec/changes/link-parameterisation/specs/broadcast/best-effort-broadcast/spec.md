## ADDED Requirements

### Requirement: The link beneath is a parameter

This broadcast SHALL be written against the link port and SHALL NOT name a link implementation. It
composes over any link satisfying the port, and its own guarantees hold as far as that link's do.

The guarantees already specified for this capability are stated over a link that does not lose
messages between correct processes. Over a link whose guarantees lapse at a scope boundary, they
hold within a scope; that is a property of the link supplied, not a second broadcast.

#### Scenario: The ordinary stack is unchanged

- **WHEN** this broadcast is used without naming a link
- **THEN** it composes over the perfect link, exactly as before this change

#### Scenario: The same implementation runs over a session link

- **WHEN** this broadcast is composed over a link that reports scope boundaries
- **THEN** validity holds within a session, and there is one implementation rather than two

### Requirement: Scope boundaries reported by the link reach the layer above

When composed over a link that reports the ending and re-establishment of the scopes carrying its
messages, this broadcast SHALL pass those reports upward rather than absorbing them. A layer that
can repair what an ending lost cannot do so if it is not told.

When composed over a link that reports no such boundary, no such report is emitted and none can be
constructed.

#### Scenario: An ending and an establishment both reach the layer above

- **WHEN** the link beneath reports that a scope ended, and later that one was established
- **THEN** both reach the layer above, distinguishable from one another

#### Scenario: Nothing is invented over a link without scopes

- **WHEN** this broadcast is composed over a link that reports no boundary
- **THEN** it reports none either

### Requirement: A directed send reaches one member

This broadcast SHALL offer a request that reaches exactly one named member, alongside the fan-out to
every member. Repair after a scope ending is directed at the peer whose scope returned, so a layer
above needs a way to address one process without broadcasting to all.

#### Scenario: Only the addressed member receives it

- **WHEN** a directed send names one member
- **THEN** that member receives the message and no other member does
