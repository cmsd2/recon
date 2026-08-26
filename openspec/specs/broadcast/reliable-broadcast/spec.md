# broadcast/reliable-broadcast Specification

## Purpose

Broadcast in which the correct processes agree on what was delivered, even when the sender crashed
partway through sending. This is the first abstraction whose guarantee survives the failure of another
process rather than of the network.

## Requirements

### Requirement: Validity

If a correct process broadcasts a message, that process SHALL eventually deliver it.

This is deliberately weaker than best-effort broadcast's validity, which promises delivery to
every correct process. Delivery to the others follows from agreement, not from this requirement.

#### Scenario: A correct sender delivers its own broadcast

- **WHEN** a correct process broadcasts a message
- **THEN** that process eventually delivers it to the layer above

### Requirement: Agreement

If a message is delivered by any correct process, it SHALL eventually be delivered by every
correct process — including when the process that originally broadcast it has crashed.

#### Scenario: The sender crashes after reaching only some processes

- **WHEN** a process broadcasts a message, some but not all processes deliver it, and the sender
  then crashes
- **THEN** every process that has not crashed eventually delivers that message

#### Scenario: The sender crashes before reaching anyone

- **WHEN** a process crashes while broadcasting and no process delivers the message
- **THEN** no process delivers it, and no requirement is violated

#### Scenario: Agreement holds under loss and partition

- **WHEN** a message is delivered by at least one correct process while the network is losing
  messages and later partitioned and healed
- **THEN** every correct process eventually delivers it

#### Scenario: A message never broadcast is never agreed upon

- **WHEN** no process broadcasts anything
- **THEN** no process delivers anything

### Requirement: No duplication

Each process SHALL deliver each broadcast message at most once, however many times it receives it
from the layer below or from a relaying process.

#### Scenario: Relayed copies are suppressed

- **WHEN** several processes relay the same message and it arrives repeatedly
- **THEN** each process delivers it exactly once

#### Scenario: Separately broadcast messages with identical content are both delivered

- **WHEN** a process broadcasts the same content twice as two separate requests
- **THEN** every correct process delivers two messages

### Requirement: No creation

A process SHALL deliver a message only if that message was previously broadcast by the process
named as its sender.

#### Scenario: Deliveries match broadcasts

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast by the named sender

#### Scenario: A relayed message is attributed to its originator

- **WHEN** a process delivers a message that reached it via a relaying process rather than from
  the original sender
- **THEN** the delivery names the original sender, not the relayer
