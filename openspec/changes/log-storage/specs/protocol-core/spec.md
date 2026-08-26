## MODIFIED Requirements

### Requirement: Effects are the only means of affecting the world

A protocol SHALL express every outward action as an effect. The available effects SHALL be:
sending a message to a named peer, raising an indication to the layer above, and requesting a
timer. A protocol MUST NOT send, deliver, or schedule by any other means.

Storage is deliberately not among them, and the reason is that an effect is one-way. A protocol
must be able to *ask* storage a question and use the answer in the same breath, and nothing
one-way can do that.

#### Scenario: A protocol transmits to a peer

- **WHEN** a protocol's logic requires transmitting to another process
- **THEN** it emits a send effect naming the destination and the message, and performs no
  transmission itself

#### Scenario: A protocol reports to its caller

- **WHEN** a protocol's logic satisfies a guarantee owed to the layer above
- **THEN** it emits an indication effect, and does not call into that layer directly

#### Scenario: A protocol records something that must survive a crash

- **WHEN** a protocol's logic requires state to outlive the current process incarnation
- **THEN** it writes through the storage interface supplied to it, and performs no write itself

### Requirement: A protocol declares what it keeps durably

A protocol SHALL declare the type of the metadata it keeps and the type of the entries it appends,
and both SHALL be distinct from the protocol's own state. A protocol that keeps nothing durably
SHALL be able to say so in a way that makes a write impossible to construct for it.

#### Scenario: A protocol with no durable state cannot emit a store

- **WHEN** a protocol declares that it keeps nothing durably
- **THEN** no value can be constructed to write or to append, so a write is rejected when the code
  is built rather than when it runs

#### Scenario: Such a protocol may still read, vacuously

- **WHEN** a protocol that keeps nothing durably reads from its store
- **THEN** it finds nothing, and this is a normal answer rather than an error

#### Scenario: A storing child cannot be composed

- **WHEN** a protocol attempts to compose a child that declares durable state of its own
- **THEN** it fails to build, because there is no way to give a child a store of its own —
  a parent and child sharing one store would collide, and scoping one store into two is a design
  nothing yet needs

#### Scenario: What is durable is visible in the interface

- **WHEN** a reader asks what a protocol would still know after a crash
- **THEN** the answer is the declared metadata and entry types, not a convention about which
  fields are written

### Requirement: Startup is a branch, and exactly one side runs

A protocol SHALL have two startup entry points — initialisation and recovery — of which **exactly
one** runs. A process with nothing in storage is initialised; a process with something in storage
is recovered, and reads what survived through its storage handle. Both SHALL be able to emit
effects.

The constructor cannot serve as either. It runs in both cases, so it is the common prefix of the
branch rather than one side of it, and it cannot emit effects, so first-start work that must be
*done* rather than merely set up has nowhere to happen. Writing an initial value down is the
standard case: repeating it on recovery would overwrite exactly what was being recovered.

#### Scenario: A restarted protocol is told what survived

- **WHEN** a process crashes and restarts, having written durable state before the crash
- **THEN** its recovery handler can read that state, and its volatile state is empty

#### Scenario: A first start is initialised, not recovered

- **WHEN** a process starts with nothing in storage
- **THEN** its initialisation entry point runs and its recovery entry point does not

#### Scenario: A restart is recovered, not initialised

- **WHEN** a process restarts with something in storage
- **THEN** its recovery entry point runs and its initialisation entry point does not

#### Scenario: A first start can write something down

- **WHEN** a protocol's first act must be durable, so that a later restart recovers rather than
  beginning again
- **THEN** it writes during initialisation, and does not repeat that write on recovery

#### Scenario: Recovering can produce effects

- **WHEN** a protocol recovers and its algorithm requires it to notify the layer above or to
  re-send what was pending
- **THEN** it emits those effects during recovery, exactly as it would for any other event

### Requirement: A write completes before anything that depends on it is sent

A write SHALL become durable before **any effect emitted after it** takes visible effect — sends
leaving the process, and indications reaching the layer above. A protocol MAY therefore write and
send in response to the same event and rely on the write having taken effect first.

Without this rule a protocol can be observed by its peers to have made a promise it has no record
of, which is the failure the fail-recovery model exists to prevent. Indications are held for the
same reason at one remove: an indication is how the layer above learns something, and what it
usually does next is send.

The rule is *positional*: an effect emitted before a write is not held. A synchronous write
therefore SHALL mark its position among the effects, so that the driver can tell which effects
preceded it and which followed.

#### Scenario: A promise is durable before it is made

- **WHEN** a protocol writes and then sends in response to one event
- **THEN** the write is durable before the message leaves the process

#### Scenario: The layer above is not told before the write lands

- **WHEN** a protocol writes and then raises an indication in response to one event
- **THEN** the write is durable before the layer above is notified

#### Scenario: What preceded the write is not held

- **WHEN** a protocol sends and then writes in response to one event
- **THEN** the message leaves the process without waiting for the write

#### Scenario: A crash between the two loses the message, not the record

- **WHEN** a process crashes after the write and before the send
- **THEN** on recovery the stored state is present, and the message was never sent

## ADDED Requirements

### Requirement: Storage is a synchronous interface, and reading is possible

A protocol SHALL be supplied with a storage handle through the same parameter that supplies time,
randomness and the effect sink. The handle SHALL offer, synchronously: reading and replacing a
metadata value, appending an entry, reading the entries from a given position onward, and asking
for the current end position.

Synchronous means the call returns before the operation is durable, and that what was written is
visible to any later read in the same process incarnation. It does **not** mean durable. Durability
remains deferred and remains the driver's business, in the same way that a write to a page cache
returns long before the data is on a disk.

Storage supplied this way is the same kind of thing as the current time and the random source:
state the driver hands the protocol so that it can be made virtual and seeded. It is not IO
performed by the protocol.

#### Scenario: What was written can be read back immediately

- **WHEN** a protocol appends an entry and then reads from a position before it, within one event
- **THEN** the entry is among what it reads, without waiting for anything

#### Scenario: Reading does not require the answer to arrive later

- **WHEN** a protocol needs its durable state in order to decide what to do
- **THEN** it obtains that state within the handler, and does not have to resume in a later event

#### Scenario: Appending returns a position that reads can start from

- **WHEN** a protocol appends entries and records where it had reached
- **THEN** a later read from that position yields exactly the entries appended after it

#### Scenario: A protocol still performs no IO

- **WHEN** a protocol is driven twice with the same events, the same clock, the same seed and the
  same stored state
- **THEN** it behaves identically, because storage is supplied rather than reached for

### Requirement: Recovery reads rather than being handed a value

On recovering, a protocol SHALL be told that it is recovering and SHALL read what survived through
the storage handle, rather than being given a value as a parameter.

Recovery SHALL still complete within the handler. Nothing else — a message, a timer, a command —
SHALL be dispatched to a protocol between its being told to recover and that handler returning.
This is what makes it safe for a protocol to hold state it has not yet loaded, and it is the reason
reading must be synchronous.

#### Scenario: A recovering protocol reads what it needs

- **WHEN** a process restarts having written durable state
- **THEN** its recovery handler reads that state from the store and may act on it

#### Scenario: Nothing arrives mid-recovery

- **WHEN** a process restarts and messages are in flight to it
- **THEN** none is delivered until its recovery handler has returned

#### Scenario: A protocol may read only what it needs

- **WHEN** a protocol's durable state is larger than what it needs to resume
- **THEN** it may read a suffix rather than the whole of it
