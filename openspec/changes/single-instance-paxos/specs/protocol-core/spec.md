## Purpose

Scoping one durable record into two, so that a protocol which keeps durable state can compose
children that keep durable state of their own. Algorithm 5.10 is the first thing here that needs it.

## MODIFIED Requirements

### Requirement: A protocol declares what it keeps durably

A protocol SHALL declare the type of the metadata it keeps and the type of the entries it appends,
and both SHALL be distinct from the protocol's own state. A protocol that keeps nothing durably
SHALL be able to say so in a way that makes a write impossible to construct for it.

A protocol that keeps durable state SHALL be able to compose a child that keeps durable state, by
naming the part of its own record that belongs to that child. The child's write SHALL be **one**
write of the whole record rather than two, so that a crash cannot land between a parent's record and
its child's.

#### Scenario: A protocol with no durable state cannot emit a store

- **WHEN** a protocol declares that it keeps nothing durably
- **THEN** no value can be constructed to write or to append, so a write is rejected when the code
  is built rather than when it runs

#### Scenario: Such a protocol may still read, vacuously

- **WHEN** a protocol that keeps nothing durably reads from its store
- **THEN** it finds nothing, and this is a normal answer rather than an error

#### Scenario: A storing child is composed through a named part of the parent's record

- **WHEN** a protocol composes a child that declares durable metadata of its own, naming where the
  child's record lives inside its own
- **THEN** the child reads and writes that part, the parent keeps its own, and neither erases the
  other

#### Scenario: A storing child cannot be composed

- **WHEN** a protocol attempts to compose a child that declares durable state of its own through
  the ordinary composition, which supplies no store
- **THEN** it fails to build, because a parent and child sharing one store unscoped would collide —
  the child must be composed through a named part of the parent's record instead

#### Scenario: An appending child cannot be composed at all

- **WHEN** a protocol attempts to compose a child that appends to a sequence
- **THEN** it fails to build, because only the metadata is scoped — the sequence half is not built,
  and the signature says so rather than a comment

#### Scenario: What is durable is visible in the interface

- **WHEN** a reader asks what a protocol would still know after a crash
- **THEN** the answer is the declared metadata and entry types, not a convention about which
  fields are written

## ADDED Requirements

### Requirement: A child's durable record is a named part of its parent's

Where a child keeps durable metadata, the composition SHALL name that part of the parent's record
by a pair of pure projections — one reading the child's record out of the parent's, one putting a
child's record into a parent's. Neither SHALL capture state: a slot names a fixed place in a type,
and one that could close over state would name a different place on different calls.

The write projection SHALL accept the absence of a parent record, because a child may write before
its parent has.

#### Scenario: One write, not two

- **WHEN** a composed child makes its record durable
- **THEN** the parent's whole record is written once, with the child's part replaced, and there is
  no interval in which one is durable and the other is not

#### Scenario: A child that has written nothing reads nothing

- **WHEN** a child reads its part of a parent record it has not yet written into
- **THEN** it finds nothing, rather than finding the parent's record or another child's

#### Scenario: A child writing first creates the parent's record

- **WHEN** a child writes before anything else in the run has
- **THEN** a whole parent record comes into being with the child's part in it and the rest at its
  default

#### Scenario: Only the store changes

- **WHEN** a durable child sends a message or raises an indication
- **THEN** both are re-wrapped and collected exactly as they are for a child that keeps nothing
