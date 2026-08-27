## MODIFIED Requirements

### Requirement: A parent composes a child by owning it and re-wrapping its effects

A protocol that is built on another SHALL own that child directly, and SHALL translate each effect
the child emits **that carries the child's own vocabulary** — its messages and its indications —
into its own terms before that effect leaves the parent. Composition MUST NOT depend on names,
identifiers, or registries resolved while running.

A timer is not among them. It is named by an opaque handle the driver issued, which says nothing
about which layer registered it, so there is nothing for a parent to translate and no mapping for
it to supply. Requiring a parent to re-wrap *every* effect is what made a timer's type encode its
position in the composition.

A parent SHALL state what it requires of its child as a declared port — the child's request and
indication types — rather than by naming a particular implementation. The child is a parameter of
the parent, so a parent is written once and composed over every implementation satisfying the port.
A parent MUST NOT depend on anything about its child beyond that port.

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

#### Scenario: The requirement on a child is visible in the parent's interface

- **WHEN** a reader asks what a parent needs of the layer beneath it
- **THEN** the answer is the port named in the parent's own declaration, and nothing else is
  required to know it

#### Scenario: Substituting an implementation does not change the parent

- **WHEN** a parent is composed over a different implementation of the same port
- **THEN** the parent's source is unchanged, and its guarantees continue to hold as far as the
  substituted implementation's own guarantees allow
