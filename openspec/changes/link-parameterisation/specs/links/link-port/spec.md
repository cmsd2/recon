## Purpose

Names the request and indication vocabulary a layer above the link depends on, so that swapping one
link implementation for another is a change to a type argument rather than a fork of everything
above it.

## ADDED Requirements

### Requirement: The link port is a contract belonging to neither side

There SHALL be a named vocabulary of requests a layer may make of a link and indications a link may
raise. It SHALL belong to neither the layer above nor any implementation below: a layer above states
its requirement as the port, and a link states its capability as the port, and neither names the
other.

A layer above the link SHALL depend on the port and on nothing else about the link. Depending on a
particular implementation is what forces a fork when a second implementation appears.

#### Scenario: A layer states its requirement without naming an implementation

- **WHEN** a layer that composes over a link is written
- **THEN** it names the port, and no implementation of it
- **AND** the project builds without that layer mentioning any particular link

#### Scenario: A second implementation needs no change above it

- **WHEN** a second link satisfying the port is introduced
- **THEN** every layer above it composes over it unchanged
- **AND** no layer above it is duplicated to accommodate it

#### Scenario: A link that does not satisfy the port is rejected before running

- **WHEN** a type that does not satisfy the port is supplied as a link
- **THEN** the error is reported when the project is built

### Requirement: The port admits links that report scope boundaries and links that cannot

A link running over a transport whose sessions end SHALL be able to report those boundaries, and a
link with no such boundary to report SHALL NOT be obliged to invent one. Both SHALL satisfy the same
port, so that a layer above is written once rather than once per kind of link.

A layer whose guarantees do not depend on scope boundaries SHALL compose over either kind without
mentioning them. A layer that repairs a scope ending SHALL state that it requires a link reporting
them, and that requirement SHALL be checked when the project is built.

#### Scenario: A layer indifferent to scopes composes over both kinds

- **WHEN** a layer whose guarantees do not depend on scope boundaries is composed over a link that
  reports them, and over one that does not
- **THEN** both compose, and the layer is written once

#### Scenario: A layer that repairs a scope ending requires a link that reports one

- **WHEN** a layer whose liveness depends on being told that a session was re-established is
  composed over a link that cannot report it
- **THEN** the error is reported when the project is built, rather than the layer waiting forever

#### Scenario: A link with no sessions is not made to pretend

- **WHEN** a link whose guarantees never lapse satisfies the port
- **THEN** it raises no scope boundary, and is not required to declare a scope it cannot observe

### Requirement: An application may supply its own link

A link written outside this project SHALL be usable beneath this project's protocols, up to and
including consensus, without either the link or the protocols being edited.

#### Scenario: A foreign link carries a broadcast

- **WHEN** a link written outside this project, satisfying the port, is supplied to a broadcast
- **THEN** the broadcast's guarantees hold over it, as far as that link's own guarantees allow

#### Scenario: A foreign link carries consensus

- **WHEN** the same link is supplied beneath the consensus stack and every process proposes
- **THEN** every correct process decides, and no two decide differently
- **AND** neither the link nor the consensus implementation was modified to achieve it
