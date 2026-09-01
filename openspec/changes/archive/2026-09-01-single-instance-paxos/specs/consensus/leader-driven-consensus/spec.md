## Purpose

Paxos: uniform consensus built from an epoch-change and a sequence of abortable epoch consensuses.
Its distinguishing property is not that it decides, but that it stays safe while the leader detector
is wrong — which is the case flooding consensus cannot survive.

## ADDED Requirements

### Requirement: No two processes decide differently, even while leadership is disputed

No two processes SHALL decide different values. This SHALL hold **including** in runs where the
leader detector is inaccurate, where two processes each believe they lead, and where their epochs
overlap.

This is the requirement the abstraction exists for. `consensus/flooding-consensus` fails exactly
here: one false suspicion splits it permanently, which its own suite demonstrates. A suite that
tests this one only where the detector behaves has tested nothing that flooding consensus does not
already give.

#### Scenario: Agreement holds under an inaccurate detector

- **WHEN** the timing assumption is withdrawn so that correct processes are suspected, and two
  processes each act as leader in overlapping epochs
- **THEN** no two processes decide different values

#### Scenario: The disputed leadership is real

- **WHEN** that run is examined
- **THEN** more than one process is observed to have acted as a leader, so the agreement above is
  not a statement about a run in which nothing happened

#### Scenario: Agreement holds across crashes

- **WHEN** processes crash during a run, including a leader mid-decision
- **THEN** no two surviving processes decide different values

### Requirement: A decision is final

A process SHALL decide at most once, and SHALL NOT retract or replace a decision when a later epoch
begins.

#### Scenario: A later epoch does not overturn a decision

- **WHEN** a process has decided and a new epoch starts
- **THEN** it does not decide again, and its decision is unchanged

### Requirement: A decision is a proposal, and every correct process eventually reaches one

A decided value SHALL be one that some process proposed. Every correct process SHALL eventually
decide, provided a majority is correct and the leader detector eventually settles.

Termination is conditional on both, and stating it that way is the point: a majority that never
forms, or a detector that never settles, leaves this abstraction waiting — which is the honest
outcome and what the FLP result requires of it.

#### Scenario: Every correct process decides once the assumptions hold

- **WHEN** a majority is correct and the leader detector settles on a correct process
- **THEN** every correct process eventually decides

#### Scenario: Nothing is decided that was not proposed

- **WHEN** a process decides
- **THEN** the value was proposed by some process

#### Scenario: Without a majority it waits rather than diverges

- **WHEN** no majority is available
- **THEN** no process decides, and none decides differently from any other later
