# links/stubborn-link Specification

## Purpose

Turns a lossy network into one where a message sent between correct processes eventually arrives,
by retransmitting it indefinitely. This is the first abstraction in the sequence and the foundation the
perfect link is built on.

## Requirements

### Requirement: Stubborn delivery

If a correct process sends a message to a correct process, the recipient SHALL deliver that message
infinitely often, for as long as both remain correct. Delivery is not required while the recipient
has crashed or is partitioned away.

#### Scenario: A message survives heavy loss

- **WHEN** a correct process sends a message to a correct process over a network configured to
  drop most messages
- **THEN** the recipient delivers that message at least once, and continues to deliver it

#### Scenario: Delivery resumes after a partition heals

- **WHEN** a message is sent while the two processes are partitioned, and the partition is later
  removed
- **THEN** the recipient delivers the message after the partition is removed

#### Scenario: No delivery is required to a crashed process

- **WHEN** the recipient has crashed
- **THEN** no delivery is required, and the sender continues retransmitting without failing

### Requirement: No creation

A message SHALL be delivered by a process only if it was previously sent by the named sender. The
link MUST NOT deliver a message that was never sent, nor attribute a message to a process that did
not send it.

#### Scenario: Only sent messages are delivered

- **WHEN** a run completes
- **THEN** every delivery in the trace corresponds to an earlier send by the process named as its
  sender

### Requirement: Retransmission continues until stopped

The link SHALL continue retransmitting a message at its configured interval until instructed to
stop retransmitting it. Retransmission SHALL NOT depend on any acknowledgement from the recipient.

#### Scenario: Retransmission repeats over time

- **WHEN** a message is sent and no instruction to stop is given
- **THEN** the trace shows repeated transmissions of that message separated by the configured
  interval

#### Scenario: Retransmission ceases when stopped

- **WHEN** the layer above instructs the link to stop retransmitting a message
- **THEN** no further transmissions of that message appear in the trace
