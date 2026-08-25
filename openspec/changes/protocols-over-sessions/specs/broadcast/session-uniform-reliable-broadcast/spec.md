## Purpose

Uniform reliable broadcast over session links and a perfect failure detector. Unlike the reliable
broadcast beside it, this layer keeps both of its guarantees across a session ending — because
between resending on re-establishment and being told when a peer is gone, there is no outcome in
which it waits for ever.

## ADDED Requirements

### Requirement: Validity

If a correct process broadcasts a message, that process SHALL eventually deliver it, including when
sessions end and are re-established in the meantime.

#### Scenario: A correct sender delivers despite a session ending

- **WHEN** a correct process broadcasts, a session with some peer ends and is later re-established
- **THEN** the sender eventually delivers its own message

### Requirement: Uniform agreement

If a message is delivered by any process, whether correct or subsequently crashed, it SHALL
eventually be delivered by every correct process.

#### Scenario: A process delivers and then crashes

- **WHEN** a process delivers a message and crashes immediately afterwards
- **THEN** every correct process eventually delivers that message

#### Scenario: Uniform agreement survives a session ending

- **WHEN** a message is delivered by some process and a session with another has ended and been
  re-established
- **THEN** every correct process eventually delivers it

### Requirement: An established session prompts a resend

When a session with a peer is **established**, this layer SHALL re-broadcast every pending message
that peer has not been seen to acknowledge. It SHALL NOT attempt to resend on being told a session
ended, because the peer is then unreachable and anything sent would be discarded.

This uses only the record the algorithm already keeps and the broadcast it already performs: no new
message, no acknowledgement protocol, and no state beyond what is required to decide delivery.

#### Scenario: What the peer missed is sent again

- **WHEN** a session with a peer is established and pending messages exist that it has not
  acknowledged
- **THEN** those messages are broadcast again

#### Scenario: Nothing is attempted on the ending

- **WHEN** this layer is told a session with a peer ended
- **THEN** it does not resend to that peer, there being no session over which to do so

#### Scenario: What the peer has acknowledged is not sent again

- **WHEN** a session is established and the peer has acknowledged every pending message
- **THEN** nothing is re-broadcast on its account

### Requirement: Progress does not depend on the peer returning

If a peer never returns, this layer SHALL still make progress once the failure detector reports it
crashed, because it is then no longer waited for.

#### Scenario: A peer that never returns is eventually excluded

- **WHEN** a session with a peer ends and is never re-established, and the detector accuses it
- **THEN** the remaining correct processes deliver the pending messages

#### Scenario: There is no third outcome

- **WHEN** a session ends
- **THEN** either it is established again and what was missed is resent, or the peer is eventually
  accused and no longer waited for

#### Scenario: Establishment is provoked rather than waited for

- **WHEN** a session has ended and this process sends nothing of its own
- **THEN** the failure detector's heartbeats still establish a session if the peer is reachable, so
  progress does not depend on the layer above happening to send

### Requirement: No duplication and no creation

Each process SHALL deliver each broadcast at most once, and only if it was previously broadcast by
the process named as its sender.

#### Scenario: Resending does not cause double delivery

- **WHEN** a message is re-broadcast after a session is re-established
- **THEN** no process delivers it twice
