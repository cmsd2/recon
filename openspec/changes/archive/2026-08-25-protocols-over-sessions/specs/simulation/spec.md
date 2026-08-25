## ADDED Requirements

### Requirement: A session is re-established without being prompted

Once communication with a peer becomes possible again, the simulator SHALL establish a session with
it without waiting for either process to send. It MAY delay before doing so, modelling a link that
retries with backoff.

This models the link a deployment would have: one that keeps trying to reconnect on its own,
reporting its epoch and connected status upward. It matters because the alternative — establishing
lazily, when something happens to transmit — makes reconnection depend on the state of the layers
above, which neither end controls and which may be silent indefinitely.

#### Scenario: A healed partition reconnects on its own

- **WHEN** a session ends because of a partition, the partition heals, and no process sends
  anything
- **THEN** a session is established anyway, and both processes are told

#### Scenario: A restarted process reconnects on its own

- **WHEN** a session ends because a process crashed, and that process restarts
- **THEN** a session is established without either process being prompted

#### Scenario: An unreachable peer is retried, not abandoned

- **WHEN** a peer remains unreachable
- **THEN** the simulator continues to attempt establishment, and establishes one as soon as it
  becomes possible

### Requirement: A session establishment is reported to the processes

When a session is established with a peer — whether prompted by this process sending, by the peer
sending, or by any other traffic — the simulator SHALL report it to both processes, naming the
epoch now in force.

This is a distinct event from the ending, and the two are not interchangeable. An ending is known
at the moment of failure but cannot be acted on, the peer being unreachable. An establishment is
what a protocol can act on, and it happens at a moment neither end fully controls: it may be
provoked by a heartbeat, by an application send, or by the peer connecting inward.

#### Scenario: Establishment is reported when a session opens

- **WHEN** a session is established with a peer
- **THEN** both processes are told, naming the epoch now in force

#### Scenario: A message sent in response is delivered

- **WHEN** a process sends to a peer on being told a session with it was established
- **THEN** that message is delivered, the session being in force

#### Scenario: A peer that never returns is never reported established

- **WHEN** a session ends and no later session is established with that peer
- **THEN** no establishment is reported for it, and a process must learn of its absence by other
  means

#### Scenario: Nothing above need provoke it

- **WHEN** a session becomes possible again and neither process sends
- **THEN** it is established and both processes are told, because reconnection is the link's own
  business and not the business of the layers above
