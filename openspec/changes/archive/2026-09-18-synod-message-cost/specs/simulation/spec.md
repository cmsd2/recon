## ADDED Requirements

### Requirement: A message addressed to its own sender does not cross the network

Where a protocol asks to send a message to the process it is running on, the simulator SHALL hand it
to that process at the instant it was asked for, and SHALL NOT give it a latency, subject it to
loss, duplication, reordering or partition, or count it among the messages the run put on the
network.

A driver is what turns a request to send into a packet, so a driver is what should notice that a
packet is addressed to the process it came from. A protocol whose roles are co-located on one
process — a leader and an acceptor in the same state machine, say — reaches its other role by a
hand-off, not by the network, and a simulator that charges it a delivery bound both overstates what
a deployment costs and makes every phase take longer than it does, which changes what the run
retransmits.

This narrows what the simulator can lose and never widens it: a hand-off delivered at the instant it
is made cannot be delayed, dropped, duplicated, reordered, or discarded by a scope ending. There is
no scope between a process and itself to end.

The hand-off SHALL be delivered rather than executed inside the handler that asked for it, as every
other effect is. A simulator that ran a handler inside another handler's effects would have to argue
that the nesting terminates; delivering at the instant removes the question.

#### Scenario: A process's message to itself takes no time

- **WHEN** a protocol sends to the process it is running on
- **THEN** the recipient handles it at the instant it was sent, with no delivery delay

#### Scenario: A process's message to itself is not a network message

- **WHEN** a run is asked what it put on the network
- **THEN** messages a process addressed to itself are not among them

#### Scenario: A fault cannot reach a message a process addressed to itself

- **WHEN** a run partitions, loses, duplicates or reorders messages, or ends scopes
- **THEN** no message a process addressed to itself is affected

### Requirement: A hand-off is recorded, so it stays observable

The trace SHALL record a message a process addressed to itself, distinguishably from one that
crossed the network, and SHALL offer both together to a reader that wants every exchange rather than
only the network's.

Ceasing to be a network message must not mean ceasing to be observable. A protocol whose role
answers its other role is doing the same thing whether the answer crossed a wire or not, and a suite
asks the trace about it either way: whether a ballot was refused, whether a majority answered,
whether an exchange happened at all. An implementation that made the hand-off invisible was written
first and rejected, because it silently emptied a non-vacuity floor that two registered safety tests
depend on — the refusals those runs contained had all been a leader hearing from its own acceptor.

#### Scenario: A hand-off appears in the trace

- **WHEN** a protocol sends to the process it is running on
- **THEN** the trace records it, distinguishably from a message that crossed the network

#### Scenario: A reader can ask for every exchange or for the network alone

- **WHEN** a run contains both messages between processes and messages a process addressed to itself
- **THEN** the trace can be asked for the network's messages alone, and for both together
