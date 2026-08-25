# broadcast/session-uniform-reliable-broadcast Specification

## Purpose

Uniform reliable broadcast over session links and a perfect failure detector. Unlike the reliable
broadcast beside it, this layer keeps both of its guarantees across a session ending — because
between resending on re-establishment and being told when a peer is gone, there is no outcome in
which it waits for ever.

## Requirements

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

When a session with a peer is **established**, this layer SHALL send that peer every message it
still holds pending. It SHALL send only to that peer, and SHALL NOT attempt to resend on being told
a session ended, because the peer is then unreachable and anything sent would be discarded.

The resend is unconditional, and the obvious economy is unsound. The record of acknowledgements the
algorithm keeps says who relayed a message **to this process**; it says nothing about whether this
process's own relay reached them, and that relay is what they are waiting for. Skipping a peer
already recorded as having the message deadlocks: this process delivers, having seen everyone relay,
and therefore never resends the relay another process is still waiting for. Deciding when to stop
would require an acknowledgement message, which is a new communication step and is out of scope for
this rung.

This uses only the broadcast layer's directed send and the payloads the algorithm already keeps: no
new message type and no state beyond what is required to decide delivery. Its cost is that pending
messages are never pruned, so an establishment sends one message per broadcast so far — the
transcription's unbounded growth appearing as traffic as well as memory.

#### Scenario: What the peer missed is sent again

- **WHEN** a session with a peer is established and pending messages exist
- **THEN** each of those messages is sent to that peer

#### Scenario: Nothing is attempted on the ending

- **WHEN** this layer is told a session with a peer ended
- **THEN** it does not resend to that peer, there being no session over which to do so

#### Scenario: Only the peer whose session returned is sent to

- **WHEN** a session with one peer is established while sessions with the others held throughout
- **THEN** nothing is sent to those others on that account

#### Scenario: A skipped resend would deadlock

- **WHEN** a process is missing only one peer's relay of a message that the peer has already
  delivered
- **THEN** the peer sends its relay again on re-establishment, rather than concluding from its own
  record that the process already has it

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
