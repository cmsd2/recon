# consensus/logged-epoch-consensus Specification

## Purpose

The quorum core for processes that crash and restart. What a process accepted, and the timestamp at
which it accepted it, outlive the crash — so a restarted process cannot contradict a promise its
earlier incarnation made and a quorum may have relied on.

## Requirements

### Requirement: What was accepted is durable before it is revealed

The accepted value and its timestamp SHALL be written durably **before** any message or indication
reveals that they were accepted.

A process that told a quorum it had accepted a value, then crashed and came back having forgotten,
would be free to accept a different value at the same timestamp. Two majorities would then report
different things about one epoch, and the intersection argument that makes the algorithm safe would
no longer hold. The write must precede the reply in the handler's own text, not by relying on a
driver to buffer effects.

#### Scenario: The write precedes the reply

- **WHEN** a process accepts a value
- **THEN** the value and its timestamp are durable before the acceptance is sent

#### Scenario: A process that died inside the write reveals nothing it did not record

- **WHEN** a process crashes during the write and recovers
- **THEN** it has either recorded the acceptance or never announced it, and never the reverse

### Requirement: A recovered process resumes from what it accepted

On recovery a process SHALL read back the accepted value and timestamp, and SHALL respond to reads
with them rather than with an empty state.

A recovered process answering as though it had accepted nothing is exactly the forgetting the
durability exists to prevent — it would allow a later epoch to overwrite a value an earlier one had
already decided.

#### Scenario: Recovery restores the accepted state

- **WHEN** a process restarts after accepting a value
- **THEN** a read of its state returns that value and its timestamp

#### Scenario: A process that accepted nothing recovers nothing

- **WHEN** a process restarts having accepted nothing
- **THEN** it reports an empty state, and is not treated as having accepted

### Requirement: Safety holds across crashes and recoveries

No two processes SHALL decide different values in a run containing crashes and recoveries, including
a leader crashing partway through a write.

#### Scenario: A leader crashing mid-write does not split the decision

- **WHEN** a leader crashes after some but not all processes have accepted its value
- **THEN** no two processes decide differently, whether or not that value is the one decided

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
