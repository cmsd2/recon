# failure-detection/eventual-leader-detector Specification

## Purpose

Ω: eventually every correct process trusts the same correct process. It is allowed to be wrong
before that, and everything above it must stay safe while it is — which is the difference between
this and the perfect failure detector, and the reason Paxos can be deployed where flooding
consensus cannot.

## Requirements

### Requirement: Eventually one correct process is trusted by all

There SHALL be a time after which every correct process trusts the same correct process, and
continues to.

Before that time the detector MAY trust different processes at different correct processes, and MAY
trust a process that has crashed. Nothing above it may assume otherwise.

#### Scenario: A single leader emerges

- **WHEN** a run continues long enough after the last crash, under a detector whose assumption holds
- **THEN** every correct process trusts the same correct process, and does not change afterwards

#### Scenario: A crashed leader is replaced

- **WHEN** the trusted process crashes
- **THEN** every correct process eventually trusts a different process, which is correct

#### Scenario: Disagreement is permitted while it lasts

- **WHEN** two correct processes trust different processes at the same moment
- **THEN** this capability is satisfied, and it is the layer above's obligation to remain safe

### Requirement: The trusted process is chosen deterministically from what is suspected

The trusted process SHALL be a fixed function of the set of processes not currently suspected, so
that two processes with the same suspicions trust the same leader.

Without this the detector could change its mind while nothing changed, and epochs above it would
advance without cause.

#### Scenario: The same suspicions give the same leader

- **WHEN** two processes suspect exactly the same set
- **THEN** they trust the same process

#### Scenario: Trust changes only when the suspected set does

- **WHEN** the suspected set is unchanged
- **THEN** the trusted process is unchanged, and no new indication is raised
