## ADDED Requirements

### Requirement: Work is bounded by membership, not by time

The messages an instance sends per unit time SHALL not grow with how long the instance has been
running. A redelivered announcement from the leader SHALL not be answered again: the answer travels
by a link that retransmits it until the instance ends, so one answer suffices.

#### Scenario: The send rate is flat in steady state

- **WHEN** an instance runs for several times longer than it takes to decide, with nothing faulty
- **THEN** the number of messages sent per window of time is the same in the last window as in the
  first

#### Scenario: A redelivered read or write is not answered again

- **WHEN** the leader's `READ` or `WRITE` is delivered to a follower a second time
- **THEN** the follower sends no second reply, and its first reply is still being retransmitted
