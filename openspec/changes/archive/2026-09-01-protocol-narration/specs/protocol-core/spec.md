## ADDED Requirements

### Requirement: A protocol may narrate a decision through its context

A protocol SHALL be able to record a decision it has taken, in a vocabulary it declares, through the
same context that supplies it with time, randomness and storage.

The vocabulary SHALL be typed, so that a recorded decision can be matched and asserted against rather
than read. A decision flattened into a message string is not checkable, and an unchecked narration is
a comment with a longer reach.

A narrated decision SHALL be attributed to the process that took it and to the time at which it was
taken, by the driver rather than by the protocol. These are the two facts a reader of a reproducible
run needs first, and a protocol that had to supply them could omit or mistake them silently.

Narration SHALL be available at decisions whose outcome is to take **no** action. That is the case a
record of effects cannot cover, and it is the case that has cost this project the most: a process
that should have announced something and did not leaves nothing behind to read.

#### Scenario: A decision is recorded with the process and the time

- **WHEN** a protocol narrates a decision
- **THEN** the record carries which process narrated it and the time at which it did

#### Scenario: A decision to do nothing can be narrated

- **WHEN** a protocol reaches a decision point and takes no action
- **THEN** it can still record what it decided and why

### Requirement: A protocol that declares no vocabulary cannot narrate

A protocol SHALL declare the vocabulary it narrates in, and one that declares none SHALL be unable to
narrate at all — not by convention, but because there is no value it could pass.

This is how scope events already work, and for the same reason: what a layer does not participate in
should be impossible for it rather than merely unusual, so that inserting a layer changes nothing
about the layers around it.

#### Scenario: Narration is unavailable to a protocol that declares none

- **WHEN** a protocol declares no narration vocabulary
- **THEN** no call narrating a decision can be written for it

#### Scenario: Composition does not translate a note

- **WHEN** a protocol composes a child that narrates
- **THEN** the child's records reach the run unchanged, without the parent restating them

### Requirement: Narrating does not change the run

A run with narration observed SHALL be identical to the same run with it unobserved, but for the
records themselves. No protocol's state, no message, no timer and no draw from the run's generator
SHALL depend on whether anything is listening.

Without this, narration would be a fault injector: the runs that are read would not be the runs that
fail, and every diagnosis reached by reading one would be a diagnosis of a different run.

#### Scenario: The same seed gives the same run either way

- **WHEN** one seed and configuration are run twice, once with narration observed and once without
- **THEN** the two runs agree on everything except the records of narration

#### Scenario: A protocol cannot read its own narration back

- **WHEN** a protocol narrates a decision
- **THEN** nothing it can subsequently observe reveals whether anything received it
