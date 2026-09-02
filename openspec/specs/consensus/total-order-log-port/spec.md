# consensus/total-order-log-port Specification

## Purpose

What a layer above a totally ordered log may depend on, and the whole of what it may. One suite is
written against this port and every implementation behind it is held to the same properties, so that
where two implementations differ is visible rather than asserted.

## Requirements

### Requirement: A layer above names the port, not an implementation

An implementation SHALL keep its own request and indication types, and the port SHALL supply the
translations a layer above needs: building an append, building a read, and classifying an
indication. Nothing else about an implementation SHALL be visible through the port.

Pinning the port to one pair of types would admit exactly one implementation, which is the failure
the link port already records: a layer written against one link's vocabulary could not compose over
another's, and four duplicated modules existed because of it.

Satisfying the port SHALL be a decision rather than an accident of shape — a protocol that has not
declared itself a log SHALL NOT be usable as one.

#### Scenario: One suite serves every implementation

- **WHEN** a property is asserted against the port
- **THEN** it holds of every implementation behind it, without the property naming one

#### Scenario: A protocol that has not declared itself a log is rejected

- **WHEN** a protocol with the right shape but no declaration is used where the port is required
- **THEN** the project fails to build

#### Scenario: Classification is total

- **WHEN** an implementation raises any indication
- **THEN** the port classifies it, so that a layer above has no case it can only discard

### Requirement: The port offers a read, which the book's interface does not

The port SHALL offer a read of the ordered sequence from a given position, returning the entries
that position and later.

This is a departure and is recorded as one. The book's abstraction is total-order broadcast — a
broadcast and an ordered delivery, with no read — but both algorithms behind this port already
maintain the totally ordered sequence, and a log's clients read it. The port exposes what the page
keeps and does not offer.

A read SHALL be served from the reading process's own copy of the sequence. The claim is a total
order, **not** that a read observes the most recent append: a process whose consensus round has not
yet decided has not yet extended its sequence, and its read SHALL reflect that rather than wait.

#### Scenario: A read returns the sequence from a position

- **WHEN** a process reads from a position
- **THEN** it receives the entries at that position and later, in order

#### Scenario: A read may lag an append that has completed elsewhere

- **WHEN** an append completes at one process and another process reads before its own round decides
- **THEN** the read may omit that entry, and this capability is satisfied

#### Scenario: What a read returns is a prefix-consistent view

- **WHEN** two processes read at any two moments
- **THEN** one result is a prefix of the other, since both are prefixes of one agreed sequence
