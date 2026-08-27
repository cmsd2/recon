# broadcast/majority-ack-uniform-reliable-broadcast Specification

## Purpose

Uniform reliable broadcast that rests on a correct majority rather than on a failure detector.
Delivery waits for more than half the processes to have relayed a message, so no process is ever
excluded and no wrong judgement about who has crashed can be made — the guarantee holds without
any assumption about network timing.

## Requirements

### Requirement: Validity


If a correct process broadcasts a message, it SHALL eventually deliver it, provided a majority of
processes are correct.

#### Scenario: A correct sender delivers its own broadcast

- **WHEN** a correct process broadcasts and a majority of processes are correct
- **THEN** it eventually delivers that message

#### Scenario: A minority crashing does not prevent delivery

- **WHEN** fewer than half the processes crash
- **THEN** every correct process still eventually delivers every message broadcast by a correct
  process

### Requirement: Uniform agreement


If a message is delivered by any process, whether correct or subsequently crashed, it SHALL
eventually be delivered by every correct process, provided a majority of processes are correct.

#### Scenario: A process delivers and then crashes

- **WHEN** a process delivers a message and crashes immediately afterwards
- **THEN** every correct process eventually delivers that message

#### Scenario: Uniform agreement does not depend on network timing

- **WHEN** the network delivers with no known bound, so that no timing assumption holds
- **THEN** no two processes deliver different sets of messages that would violate uniform
  agreement

### Requirement: No duplication and no creation


Each process SHALL deliver each broadcast at most once, and only if it was previously broadcast by
the process named as its sender, which SHALL be the originator and not a relayer.

#### Scenario: Deliveries match broadcasts and name the originator

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast, names its originator, and no
  process delivered the same broadcast twice

### Requirement: Delivery waits for a majority, and for nothing else


A message SHALL be delivered once more than half of the processes have been seen to relay it. This
layer SHALL NOT consult a failure detector, SHALL NOT maintain a set of processes believed
correct, and SHALL NOT exclude any process from consideration for any reason.

#### Scenario: A bare majority is enough

- **WHEN** more than half the processes have relayed a message
- **THEN** it is delivered, without waiting for the remainder

#### Scenario: Half is not enough

- **WHEN** exactly half the processes have relayed a message
- **THEN** it is not yet delivered

#### Scenario: No process is ever excluded

- **WHEN** a process crashes, or becomes unreachable for any length of time
- **THEN** no other process removes it from consideration, because there is no set from which to
  remove it

### Requirement: No failure-detection traffic


This layer SHALL send no message other than the broadcast payloads it exists to carry, and SHALL
require no command to begin operating.

#### Scenario: The wire carries payloads only

- **WHEN** a run completes
- **THEN** no heartbeat or other failure-detection message appears on the wire

#### Scenario: Broadcasting is the only request

- **WHEN** a process broadcasts without any prior request having been made of this layer
- **THEN** the message is delivered normally

### Requirement: The assumption is a correct majority, and its failure blocks rather than diverges


This layer's guarantees SHALL hold whenever more than half the processes are correct. When that
assumption fails, the layer SHALL cease to deliver rather than deliver inconsistently.

#### Scenario: Without a majority, nothing is delivered

- **WHEN** half or more of the processes have crashed
- **THEN** messages not already delivered are not delivered, and no process delivers a message
  that another correct process will never deliver

#### Scenario: Progress resumes when a majority is available again

- **WHEN** a majority is unreachable for a time and then becomes reachable again
- **THEN** the messages that were waiting are delivered

### Requirement: The broadcast beneath is a parameter


This protocol SHALL be written against the port of the broadcast beneath it and SHALL NOT name an
implementation.

Where the layer beneath reports that a scope was re-established, this protocol SHALL resend what is
pending to the peer whose scope returned. That resend SHALL be unconditional rather than filtered by
which peers have been seen to acknowledge, and directed rather than broadcast: acknowledgements
record who relayed to this process, not whether this process's own relay arrived, so filtering by
them deadlocks.

Where no such report is possible, the quorum alone carries liveness, and nothing is resent.

#### Scenario: The ordinary stack is unchanged

- **WHEN** this protocol is used without naming the layer beneath
- **THEN** it composes as it did before this change

#### Scenario: A re-established scope prompts an unconditional directed resend

- **WHEN** the layer beneath reports a scope established with one peer while messages are pending
- **THEN** every pending message is sent to that peer, whether or not that peer has been seen to
  acknowledge it, and to no other peer

#### Scenario: No peer is ever accused

- **WHEN** a peer never returns
- **THEN** delivery proceeds on the quorum alone, and no judgement about that peer is made or
  required

