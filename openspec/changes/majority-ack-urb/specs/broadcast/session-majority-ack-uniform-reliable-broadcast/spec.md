## Purpose

Uniform reliable broadcast over session links, resting on a correct majority rather than on a
failure detector. A suffix lost to a session ending is repaired by resending when the session
returns; a peer that never returns needs no accusation, because it was never waited for.

## ADDED Requirements

### Requirement: Validity across session endings

If a correct process broadcasts a message, it SHALL eventually deliver it, provided a majority of
processes are correct and reachable, including when sessions end and are re-established in the
meantime.

#### Scenario: A correct sender delivers despite a session ending

- **WHEN** a correct process broadcasts, a session with some peer ends and is later re-established
- **THEN** the sender eventually delivers its own message

#### Scenario: A minority becoming unreachable does not prevent delivery

- **WHEN** fewer than half the processes become unreachable and stay unreachable
- **THEN** the remaining majority still delivers every message broadcast among them

### Requirement: Uniform agreement across session endings

If a message is delivered by any process, whether correct or subsequently crashed, it SHALL
eventually be delivered by every correct process, provided a majority of processes are correct.

#### Scenario: A process delivers and then crashes

- **WHEN** a process delivers a message and crashes immediately afterwards
- **THEN** every correct process eventually delivers that message

#### Scenario: Uniform agreement survives repeated session endings

- **WHEN** sessions end and are re-established repeatedly during a run
- **THEN** every process delivers the same set of messages

### Requirement: No duplication and no creation

Each process SHALL deliver each broadcast at most once, and only if it was previously broadcast by
the process named as its sender.

#### Scenario: Resending does not cause double delivery

- **WHEN** a message is sent again after a session is re-established
- **THEN** no process delivers it twice

### Requirement: An established session prompts a resend

When a session with a peer is established, this layer SHALL send that peer every message it still
holds pending, and SHALL send only to that peer. It SHALL NOT attempt to resend on being told a
session ended, because the peer is then unreachable and anything sent would be discarded.

The resend is unconditional, for the reason recorded in the all-ack version over sessions: the
acknowledgement record says who relayed a message **to this process**, not whether this process's
own relay reached them, and that relay is what they are waiting for.

#### Scenario: What the peer missed is sent again

- **WHEN** a session with a peer is established and pending messages exist
- **THEN** each of those messages is sent to that peer

#### Scenario: Nothing is attempted on the ending

- **WHEN** this layer is told a session with a peer ended
- **THEN** it does not resend to that peer, there being no session over which to do so

#### Scenario: Only the peer whose session returned is sent to

- **WHEN** a session with one peer is established while sessions with the others held throughout
- **THEN** nothing is sent to those others on that account

### Requirement: Resending is the only liveness mechanism, and no peer is ever accused

Progress SHALL depend on reaching a majority and on resending what a peer missed, and on nothing
else. This layer SHALL NOT consult a failure detector, SHALL NOT maintain a set of processes
believed correct, and SHALL NOT require a peer to be judged crashed before anything can be
delivered.

#### Scenario: A peer that never returns is simply not waited for

- **WHEN** a session with a peer ends and is never re-established, and a majority remains
- **THEN** the remaining processes deliver the pending messages without any judgement being made
  about that peer

#### Scenario: No failure-detection traffic appears

- **WHEN** a run completes
- **THEN** no heartbeat or other failure-detection message appears on the wire, and every message
  sent is a broadcast payload

#### Scenario: A returning peer is not treated as a stranger

- **WHEN** a peer becomes unreachable for longer than any timeout the all-ack version would have
  used, and then returns
- **THEN** it receives what it missed and delivers it, no exclusion having taken place that would
  need undoing

### Requirement: Session changes are reported to the layer above

When the link reports that a session with a peer ended, or that one was established, this layer
SHALL report it upward rather than absorbing it.

#### Scenario: Both reports reach the layer above

- **WHEN** a session with a peer ends, and when a later one is established
- **THEN** the layer above is told of each, distinguishably

### Requirement: The assumption is a correct majority, and its failure blocks rather than diverges

This layer's guarantees SHALL hold whenever more than half the processes are correct and mutually
reachable. When that assumption fails, the layer SHALL cease to deliver rather than deliver
inconsistently.

#### Scenario: A minority partition delivers nothing

- **WHEN** the processes are partitioned so that one side holds fewer than half of them
- **THEN** that side delivers nothing new, rather than delivering something the majority will
  never deliver

#### Scenario: The majority side continues

- **WHEN** the processes are partitioned so that one side holds more than half of them
- **THEN** that side continues to deliver
