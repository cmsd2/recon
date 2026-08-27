## MODIFIED Requirements

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

### MODIFIED Requirement: A parent composes a child by owning it and re-wrapping its effects

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

## ADDED Requirements

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
