## ADDED Requirements

### Requirement: Work is bounded by membership, not by time

The messages a process sends per unit time SHALL not grow with how long it has been running. A
redelivered announcement that was already refused SHALL not be refused again: one refusal per
distinct announcement per peer, which is bounded by membership.

#### Scenario: The send rate is flat once leadership has settled

- **WHEN** leadership has settled and the run continues for several more timeouts
- **THEN** the number of messages sent per window of time does not increase from window to window

#### Scenario: A stale announcement is refused once

- **WHEN** an announcement a process has already refused is delivered to it again
- **THEN** no second refusal is sent
