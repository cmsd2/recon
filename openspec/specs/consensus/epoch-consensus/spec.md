# consensus/epoch-consensus Specification

## Purpose

Single-shot consensus within one epoch, which can be abandoned. It is the quorum core of Paxos: a
leader reads what a majority holds, writes a value to a majority, and decides — and when it is
abandoned it hands back its state so the next epoch does not contradict it.

## Requirements

### Requirement: A decision is a value some process proposed

A process SHALL decide only a value that was proposed, in this epoch or in an earlier one whose
state reached this one.

Deciding a value nobody proposed would make agreement worthless: every process could decide the
same invented value and satisfy it.

#### Scenario: Only proposals are decided

- **WHEN** a process decides
- **THEN** the value decided was proposed by some process in this epoch or an earlier one

#### Scenario: A process decides at most once per epoch

- **WHEN** an epoch runs to completion
- **THEN** each process decides at most one value in it

### Requirement: A decision requires a majority to have written the value

A process SHALL NOT decide until a majority of processes have accepted the value being decided.

The majority is the whole mechanism: two majorities intersect, so a value decided in one epoch is
seen by any later epoch that reads a majority. Deciding on fewer would let two epochs decide
differently.

#### Scenario: A minority is not enough

- **WHEN** fewer than a majority have accepted a value
- **THEN** no process decides it

#### Scenario: A value decided is visible to a later epoch

- **WHEN** an epoch decides a value and a later epoch reads the state of a majority
- **THEN** that value is among what it reads

### Requirement: An abandoned epoch returns its state and then does nothing

An instance that is abandoned SHALL report the state it held — the value it accepted and the
timestamp at which it accepted it — and SHALL take no further action afterwards.

That state is how the next epoch avoids contradicting this one. An instance that kept acting after
being abandoned would be a second leader by another name.

#### Scenario: Abandoning yields the state

- **WHEN** an instance is abandoned
- **THEN** it reports the value it accepted and the timestamp at which it accepted it

#### Scenario: An abandoned instance is silent

- **WHEN** an instance has been abandoned and a message for it arrives
- **THEN** it sends nothing and decides nothing

### Requirement: Only the epoch's leader drives it

A process SHALL initiate the read and the write only if it is the leader of the epoch. Every other
process SHALL respond and no more.

#### Scenario: A follower does not read or write

- **WHEN** a process that is not this epoch's leader is asked to propose
- **THEN** it initiates nothing
