# protocol-core Specification

## Purpose

Defines the contract every protocol in this project obeys: a synchronous state machine that
consumes events and emits effects, with no access to wall-clock time, ambient randomness, or
input/output. This is what makes protocols reproducible under simulation and testable without a
runtime.

## Requirements

### Requirement: Protocols are deterministic functions of state and event


A protocol SHALL produce identical effects and identical resulting state whenever it is given
identical prior state and an identical event, including the values supplied for time and
randomness. A protocol MUST NOT read wall-clock time, draw randomness from ambient sources, or
perform input/output.

#### Scenario: Identical event sequences produce identical effects

- **WHEN** two instances of the same protocol start from equal initial state and are given the
  same sequence of events, with the same time and randomness values supplied
- **THEN** both instances emit the same effects in the same order and end in equal state

#### Scenario: Handling an event completes without suspension

- **WHEN** a protocol is given any event
- **THEN** handling completes before control returns, with no intermediate state observable by any
  other party, and the resulting state reflects either all or none of that event's transition

### Requirement: Effects are the only means of affecting the world


A protocol SHALL express every outward action as an effect. The available effects SHALL be:
sending a message to a named peer, raising an indication to the layer above, and requesting a
timer. A protocol MUST NOT send, deliver, or schedule by any other means.

Storage is deliberately not among them, and the reason is that an effect is one-way. A protocol
must be able to *ask* storage a question and use the answer in the same breath, and nothing
one-way can do that.

Requesting a timer SHALL yield a handle naming the timer registered, so that a protocol has
something to compare a later expiry against. The handle is opaque and carries no information about
which protocol registered it or where that protocol sits in a composition.

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

#### Scenario: Requesting a timer names it

- **WHEN** a protocol requests a timer
- **THEN** it receives a handle for that timer, and the same handle identifies the expiry when it
  arrives

### Requirement: Time and randomness are supplied to the protocol


Current time and any random values a protocol requires SHALL be supplied through the same
parameter that receives its effects. Time SHALL be monotonic and expressed in a project-defined
type that can be assigned an arbitrary value.

#### Scenario: A run is replayed with a different clock

- **WHEN** the same protocol is driven once with simulated time and once with real time, given
  the same event sequence and time values
- **THEN** it emits the same effects in both cases

#### Scenario: Randomised choice is reproducible

- **WHEN** a protocol that makes a randomised choice is driven twice with the same seeded source
- **THEN** it makes the same choice both times

### Requirement: A parent composes a child by owning it and re-wrapping its effects


A protocol that is built on another SHALL own that child directly, and SHALL translate each effect
the child emits **that carries the child's own vocabulary** — its messages and its indications —
into its own terms before that effect leaves the parent. Composition MUST NOT depend on names,
identifiers, or registries resolved while running.

A timer is not among them. It is named by an opaque handle the driver issued, which says nothing
about which layer registered it, so there is nothing for a parent to translate and no mapping for
it to supply. Requiring a parent to re-wrap *every* effect is what made a timer's type encode its
position in the composition.

#### Scenario: A child's outgoing message is re-wrapped

- **WHEN** a child emits a send effect
- **THEN** the parent emits a corresponding send effect carrying the child's message wrapped in
  the parent's own message type

#### Scenario: A child's indication is consumed by the parent

- **WHEN** a child emits an indication
- **THEN** the parent handles it as an input to its own logic, and emits its own indication only
  where its guarantees require one

#### Scenario: A child's timer request is not re-wrapped

- **WHEN** a child emits a timer request
- **THEN** the parent passes it outward unchanged, supplying no mapping for it

#### Scenario: A mis-wired composition is rejected before running

- **WHEN** a parent is written to pass a message of the wrong type to a child
- **THEN** the error is detected when the project is built, not by observing an undelivered message

### Requirement: Message payloads are carried as typed values and encoded once


A protocol stack SHALL pass message payloads between layers as typed values. Encoding to bytes
SHALL happen exactly once, at the boundary where messages leave the process, and no intermediate
encoded or type-erased representation SHALL be constructed at any layer boundary.

#### Scenario: A message crosses several layers

- **WHEN** a message passes down through every layer of a composed stack and is transmitted
- **THEN** it is encoded exactly once, and decoded exactly once on receipt

### Requirement: Failures are reported as distinct typed causes


Each layer SHALL report its failures as its own error type, preserving the originating cause.
Errors MUST NOT be flattened into a general-purpose input/output error or reduced to a message
string.

#### Scenario: A decoding failure is surfaced

- **WHEN** a message fails to decode at the wire boundary
- **THEN** the reported error identifies decoding as the cause and retains the underlying detail

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

This follows from the write being durable when it returns rather than being a separate obligation
on the driver: an effect emitted after a write is emitted after the write has landed, because the
handler could not have reached it otherwise. A driver has nothing to hold and nothing to order.

#### Scenario: A promise is durable before it is made

- **WHEN** a protocol writes and then sends in response to one event
- **THEN** the write is durable before the message leaves the process

#### Scenario: The layer above is not told before the write lands

- **WHEN** a protocol writes and then raises an indication in response to one event
- **THEN** the write is durable before the layer above is notified

#### Scenario: A send with no write before it costs nothing

- **WHEN** a protocol sends without writing in response to an event
- **THEN** the message leaves the process with no write involved

#### Scenario: A crash between the two loses the message, not the record

- **WHEN** a process is killed inside a write whose handler would have gone on to send
- **THEN** no peer receives that message, whether or not the write itself landed

### Requirement: Storage is a synchronous interface, and reading is possible


A protocol SHALL be supplied with a storage handle through the same parameter that supplies time,
randomness and the effect sink. The handle SHALL offer, synchronously: reading and replacing a
metadata value, appending an entry, reading the entries from a given position onward, and asking
for the current end position.

A write SHALL be durable when it returns. A protocol is a synchronous state machine, so the return
of the write call is the only point at which a driver can synchronise with it — after the handler
returns, the sends are already in the driver's hands. A write therefore blocks, as `fsync` does.

A read SHALL be synchronous too. That is honest while the record is mirrored in memory; for a
record larger than memory a read is a real disk read, and that is a stated bound of this interface
rather than a property of it.

Storage supplied this way is the same kind of thing as the current time and the random source:
state the driver hands the protocol so that it can be made virtual and seeded. It is not IO
performed by the protocol.

#### Scenario: What was written can be read back immediately

- **WHEN** a protocol appends an entry and then reads from a position before it, within one event
- **THEN** the entry is among what it reads, without waiting for anything

#### Scenario: A write that returned cannot be lost

- **WHEN** a protocol writes and the process crashes at any point after that call returned
- **THEN** the recovering process reads what was written

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

### Requirement: A timer is named by an opaque handle, not by a type


A timer SHALL be identified by a handle whose type is the same for every protocol. A protocol MUST
NOT declare a type of its own for timers, and a parent MUST NOT translate a child's timer into
terms of its own.

Identifying a timer by type makes the type encode the composition: every parent must re-wrap its
children's, inserting a layer rewraps every timer beneath it, and a layer's timer vocabulary becomes
visible to everything above it. A handle carries no such information, so composition costs nothing
and a layer that registers no timer needs no timer vocabulary at all.

Handles SHALL be distinct across everything composed within one process for as long as the timers
they name could be confused — that is, an identity is unique to a run, not to a layer. Two layers
each being handed the same identity is the failure this requirement exists to prevent.

#### Scenario: A protocol that registers no timer declares nothing about timers

- **WHEN** a protocol neither registers a timer nor is composed over anything that does
- **THEN** it declares no timer type, no timer vocabulary, and no translation

#### Scenario: Composition does not translate a timer

- **WHEN** a parent runs a child that registers a timer
- **THEN** the request reaches the driver unchanged, and the parent supplies no timer mapping

#### Scenario: Inserting a layer does not change the timers beneath it

- **WHEN** a layer is inserted between two others
- **THEN** the types of the timers registered below it are unchanged

#### Scenario: Two layers in one composition never share an identity

- **WHEN** two layers within one composition each register a timer
- **THEN** the handles differ

### Requirement: A protocol acts only on an expiry it registered


An expiry SHALL be delivered to the protocol at the top of a composition, and a protocol that
composes children SHALL pass it to each of them. A protocol that registered one or more timers
SHALL act on an expiry only if it registered that expiry, and SHALL ignore any other. A protocol
that registered none SHALL do nothing but pass it on.

This is the cost of a handle carrying no routing information: most of what reaches a layer belongs
to somebody else. A layer that acted on an expiry it did not register would run its timeout logic
at a moment chosen by an unrelated layer.

#### Scenario: A layer ignores another layer's expiry

- **WHEN** a layer that has a timer outstanding is given the expiry of a timer registered by a
  different layer
- **THEN** it does nothing, and its own timer remains outstanding

#### Scenario: A layer ignores an expiry it has superseded

- **WHEN** a layer registers a timer, later registers a replacement, and is then given the expiry
  of the first
- **THEN** it does nothing, because that is no longer the timer it is waiting on

#### Scenario: An expiry reaches the layer that registered it

- **WHEN** a timer registered by the lowest layer of a composition fires
- **THEN** that layer acts on it, having been passed it by each layer above

### Requirement: A protocol driven directly is given an identity source


A helper that delivers one event to a protocol SHALL allow the caller to supply the source of timer
identities, so that identities continue across successive calls as they do under a driver.

A helper that starts identities afresh on each call SHALL be documented as suitable only for a
protocol driven alone: two layers within one composition would each be handed the same identity, and
each would accept the other's expiry as its own.

#### Scenario: A composed protocol driven by hand keeps its identities distinct

- **WHEN** a composition is driven event by event with a caller-owned identity source
- **THEN** no two layers are handed the same identity, and each acts only on its own expiry

#### Scenario: A test fires the timer the protocol is waiting on

- **WHEN** a test drives a protocol and then fires a timer
- **THEN** it names the identity the protocol registered, which it learns from what the protocol
  emitted rather than by assuming a value

