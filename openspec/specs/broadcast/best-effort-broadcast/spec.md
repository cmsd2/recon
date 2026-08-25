# broadcast/best-effort-broadcast Specification

## Purpose

Delivers a message to every process in the system by sending it individually over perfect links.
It guarantees delivery to all correct processes only while the sender itself remains correct,
which is what makes it the starting point for the stronger broadcast abstractions.

## Requirements

### Requirement: Best-effort validity

If a correct process broadcasts a message, every correct process SHALL eventually deliver it.

#### Scenario: A correct sender reaches everyone

- **WHEN** a correct process broadcasts a message and all processes remain correct
- **THEN** every process eventually delivers that message

#### Scenario: A correct sender reaches survivors

- **WHEN** a correct process broadcasts a message and some other processes have crashed
- **THEN** every process that has not crashed eventually delivers that message

#### Scenario: A crashed sender gives no guarantee

- **WHEN** a process crashes partway through broadcasting
- **THEN** some processes may deliver the message and others may not, and no violation is reported

### Requirement: No duplication

Each broadcast message SHALL be delivered at most once by each process.

#### Scenario: A broadcast under network duplication

- **WHEN** a message is broadcast over a network configured to duplicate messages
- **THEN** each recipient delivers it exactly once

### Requirement: No creation

A process SHALL deliver a message only if that message was previously broadcast by the process
named as its sender.

#### Scenario: Deliveries match broadcasts

- **WHEN** a run completes
- **THEN** every delivery in the trace corresponds to an earlier broadcast by the named sender

### Requirement: The sender delivers to itself

A correct process that broadcasts a message SHALL also deliver that message to its own layer above.

#### Scenario: Self-delivery

- **WHEN** a correct process broadcasts a message
- **THEN** that process delivers the message to its own layer above, as the other processes do
