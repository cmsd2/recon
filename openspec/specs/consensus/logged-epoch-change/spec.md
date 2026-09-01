# consensus/logged-epoch-change Specification

## Purpose

Epoch-change for processes that crash and restart: the epoch a process had reached survives the
crash, so a restarted process does not begin again at a timestamp it has already used.

## Requirements

### Requirement: The epoch reached survives a crash

The timestamp of the last epoch a process started SHALL be recorded durably, and SHALL be recovered
when the process restarts. A restarted process SHALL NOT start an epoch with a timestamp it or an
earlier incarnation has already used.

Reusing a timestamp would let two different epochs share one, which the epoch-consensus above uses
to order writes — so two writes would be indistinguishable in age and the safety argument would
fail.

#### Scenario: A restart does not reuse a timestamp

- **WHEN** a process crashes after starting an epoch and then restarts
- **THEN** every epoch it starts afterwards has a timestamp greater than the one it had reached

#### Scenario: The record is durable before the epoch is announced

- **WHEN** a process starts an epoch
- **THEN** the timestamp is durable before any message or indication reveals it

### Requirement: The guarantees of epoch-change hold across incarnations

Timestamps SHALL increase and eventually settle, as they must without crashes, with those properties
holding across a crash and recovery rather than only within one incarnation.

#### Scenario: Settling survives a restart

- **WHEN** a process crashes and recovers while the leader detector is settled
- **THEN** it rejoins the same final epoch rather than starting a new sequence

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
