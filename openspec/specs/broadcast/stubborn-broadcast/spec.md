# broadcast/stubborn-broadcast Specification

## Purpose

Best-effort broadcast that delivers infinitely often rather than once. It exists because a process
which was down when a message was sent must still receive it after recovering, and nothing else in
this stack keeps trying for ever at the broadcast level.

## Requirements

### Requirement: A broadcast is delivered infinitely often

If a correct process broadcasts a message, every correct process SHALL deliver it infinitely
often.

#### Scenario: Delivery repeats over time

- **WHEN** a correct process broadcasts and the run continues
- **THEN** every correct process delivers that message repeatedly, without bound

#### Scenario: A process that was down receives it after recovering

- **WHEN** a process is crashed at the moment of a broadcast and restarts afterwards
- **THEN** it eventually delivers that message, the sender still retransmitting

### Requirement: No creation

If a process delivers a message with a named sender, that message SHALL have been previously
broadcast by that process.

#### Scenario: Deliveries match broadcasts

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast by the named sender

### Requirement: Duplication is the interface, not a defect

This layer SHALL NOT deduplicate, and the layer above SHALL be responsible for tolerating repeats.
Deduplicating here would defeat the purpose: the repeats are what reach a process that was absent.

#### Scenario: Repeats are delivered, not suppressed

- **WHEN** the same message is transmitted repeatedly
- **THEN** each arrival is delivered to the layer above

### Requirement: State is bounded by membership and by what is outstanding

This layer SHALL hold the process set and the messages it is still transmitting, and nothing per
delivery.

#### Scenario: Receiving does not grow this layer's state

- **WHEN** a growing number of messages is received
- **THEN** this layer's own state does not grow with them
