# broadcast/session-reliable-broadcast Specification

## Purpose

Eager reliable broadcast over session links. Its agreement is scoped: a relay lost to a session
ending is never retried, and this layer has no failure detector to tell it that a peer is gone. It
exists to make that limitation explicit, and to stand against the uniform version, which does not
share it.

## Requirements

### Requirement: Validity

If a correct process broadcasts a message and no session ending intervenes, that process SHALL
eventually deliver it.

#### Scenario: A correct sender delivers its own broadcast

- **WHEN** a correct process broadcasts and no session ends
- **THEN** that process eventually delivers it

### Requirement: Agreement is scoped to the sessions carrying the relay

If a message is delivered by some correct process, every correct process whose sessions hold
throughout SHALL eventually deliver it. Across a session ending this layer makes no such claim.

#### Scenario: Agreement holds while sessions hold

- **WHEN** a message is delivered by a correct process and no session ends
- **THEN** every correct process eventually delivers it, including when the original sender crashed

#### Scenario: A lost relay is not retried

- **WHEN** a relay of a delivered message is lost to a session ending
- **THEN** a correct process may never deliver that message, and this is the stated limit of the
  guarantee rather than a defect

#### Scenario: The limitation is reported, not hidden

- **WHEN** a session with a peer ends, or a later one is established
- **THEN** the layer above is told, so that it may act where this layer cannot

### Requirement: No duplication and no creation

Each process SHALL deliver each broadcast at most once, and only if it was previously broadcast by
the process named as its sender, which SHALL be the originator and not a relayer.

#### Scenario: Deliveries match broadcasts and name the originator

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast, names its originator, and no process
  delivered the same broadcast twice
