## ADDED Requirements

### Requirement: Storage reports the number of writes it has acknowledged

The storage interface SHALL expose the number of writes that have returned through it, counting
replacements and appends alike. The number SHALL be monotone within an incarnation, SHALL survive a
restart along with what was written, and SHALL be readable synchronously like everything else in the
interface.

A child composed through a slot SHALL see its parent's count, because the child's writes are
read-modify-writes of the parent's record and there is one store beneath both. A protocol that
keeps nothing durably SHALL read zero.

The count exists so that a protocol can say to a peer "this is what my storage has kept", and a peer
that has seen more can say so. It is a property of the store, not of any protocol, which is why it
is in the interface rather than reconstructed by each protocol that needs it.

#### Scenario: Every write that returns is counted once

- **WHEN** a protocol replaces its metadata and appends entries
- **THEN** the count read afterwards is the number of those calls that returned

#### Scenario: A child sees the parent's count

- **WHEN** a durable child composed through a slot writes, and then reads the count
- **THEN** it reads the same value its parent reads

#### Scenario: A protocol that keeps nothing reads zero

- **WHEN** a protocol whose durable types are uninhabited reads the count
- **THEN** it is zero
