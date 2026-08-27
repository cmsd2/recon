## Purpose

The weakest link there is, and the bottom of the book's stack: it carries a message to a peer and
reports what arrives, with no retransmission, no deduplication and no state. Every other link in
this project is defined by what it adds on top of it.

## ADDED Requirements

### Requirement: Delivery is attempted and not assured

A send SHALL be attempted once and SHALL NOT be retried. A message SHALL therefore be lost when the
network loses it, and the layer above SHALL NOT be told that it was.

This is the point rather than a limitation. Every guarantee above rests on redundancy the layer
above supplies — retransmission, gossip, a quorum — and a link that hid the loss would hide the
thing those layers exist to overcome.

#### Scenario: A lost message is not retried

- **WHEN** a message is sent and the network loses it
- **THEN** the link makes no further attempt to deliver it

#### Scenario: Delivery is reported when it happens

- **WHEN** a message arrives
- **THEN** the link reports it to the layer above, naming the sender

#### Scenario: Nothing is reported that was not sent

- **WHEN** a run completes
- **THEN** every delivery corresponds to a send by the process named as its sender

### Requirement: The link keeps no state

The link SHALL keep no state between events. Its resource use SHALL NOT grow with the number of
messages it carries, in any respect.

Deduplication, ordering and retransmission all require state, and a link that had any of them would
be one of the stronger abstractions rather than this one. A message arriving twice SHALL be reported
twice.

#### Scenario: A duplicate is reported twice

- **WHEN** the network delivers the same message to a process twice
- **THEN** the link reports it to the layer above twice

#### Scenario: State does not grow with messages carried

- **WHEN** a process carries a large number of messages
- **THEN** the link's state is unchanged, because it holds none

### Requirement: It declares no scope boundary

The link SHALL NOT report a scope boundary, because it can observe none: it holds no session, no
epoch and no incarnation it could detect the end of.

`docs/scope-annotated-modules.md` forbids a module declaring a scope it cannot observe. A layer whose
liveness depends on being told that a scope was re-established gets nothing from this link, which is
the honest outcome rather than a boundary invented to satisfy a bound.

#### Scenario: No indication is a boundary

- **WHEN** every indication this link can raise is classified through the link port
- **THEN** none of them classifies as a scope boundary

### Requirement: It satisfies the link port

The link SHALL satisfy the link port, so that a layer above composes over it by naming the port
alone.

#### Scenario: A layer above composes over it without naming it

- **WHEN** a layer bounded on the link port is composed over this link
- **THEN** it compiles unchanged, having named the port and not this implementation
