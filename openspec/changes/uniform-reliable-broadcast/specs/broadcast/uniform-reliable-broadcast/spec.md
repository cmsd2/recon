## Purpose

Broadcast in which a message delivered by *any* process — including one that crashes immediately
afterwards — is eventually delivered by every correct process. It closes the divergence reliable
broadcast permits, where a process may act on a delivery that the survivors never see.

## ADDED Requirements

### Requirement: Validity

If a correct process broadcasts a message, that process SHALL eventually deliver it.

#### Scenario: A correct sender delivers its own broadcast

- **WHEN** a correct process broadcasts a message and every process remains correct
- **THEN** that process eventually delivers it to the layer above

### Requirement: Uniform agreement

If a message is delivered by any process, whether that process is correct or subsequently crashes,
it SHALL eventually be delivered by every correct process.

#### Scenario: A process delivers and then crashes

- **WHEN** a process delivers a message and crashes immediately afterwards
- **THEN** every process that has not crashed eventually delivers that message

#### Scenario: The sender crashes partway through broadcasting

- **WHEN** a process crashes while broadcasting, and at least one process delivers the message
- **THEN** every correct process eventually delivers it

#### Scenario: Nobody delivers a message nobody received

- **WHEN** a process crashes while broadcasting and no process delivers the message
- **THEN** no process delivers it, and no requirement is violated

#### Scenario: Agreement holds through partition and healing

- **WHEN** a message is delivered by some process while the network is partitioned, and the
  partition later heals
- **THEN** every correct process eventually delivers it

### Requirement: Delivery waits for acknowledgement by every correct process

A process SHALL NOT deliver a message until every process it believes correct has acknowledged
having seen it. A process that is detected as crashed SHALL cease to be waited for.

#### Scenario: Delivery is withheld until all correct processes have seen the message

- **WHEN** a message has been received by some but not all correct processes
- **THEN** no process delivers it

#### Scenario: A crash unblocks delivery

- **WHEN** delivery is waiting on a process that then crashes and is detected as crashed
- **THEN** the remaining correct processes deliver the message

### Requirement: No duplication

Each process SHALL deliver each broadcast message at most once, however many copies it receives.

#### Scenario: Relayed and duplicated copies are suppressed

- **WHEN** several processes relay the same message and the network duplicates it
- **THEN** each process delivers it exactly once

#### Scenario: Separately broadcast messages with identical content are both delivered

- **WHEN** a process broadcasts the same content twice as two separate requests
- **THEN** every correct process delivers two messages

### Requirement: No creation

A process SHALL deliver a message only if it was previously broadcast by the process named as its
sender.

#### Scenario: Deliveries match broadcasts

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast by the named sender

#### Scenario: A message reaching a process by relay is attributed to its originator

- **WHEN** a process delivers a message that reached it via a relaying process
- **THEN** the delivery names the original sender, not the relayer

### Requirement: The guarantees depend on accurate failure detection

The specification SHALL state that these properties hold only while the failure detection this
abstraction relies on is accurate, which in turn requires the network to deliver within a known
bound. Where that assumption does not hold, uniform agreement SHALL NOT be claimed.

This dependency is stated rather than expressed as a scope: a process cannot observe the
assumption failing, because it would reach this layer as a detector mistake indistinguishable
from correct detection.

#### Scenario: A wrongly accused process breaks the guarantee

- **WHEN** the failure detector wrongly reports a correct process as crashed, because the timing
  assumption it depends on was violated
- **THEN** a message may be delivered by some processes and not others, and this is a violation of
  the assumption rather than of the implementation
