## MODIFIED Requirements

### Requirement: Data is gossiped and recovery is direct

This capability SHALL disseminate data through the probabilistic broadcast beneath it, and SHALL
send retransmission requests and their answers **directly over the link**, not through that
broadcast.

Routing a request through the gossip would flood the membership to repair one process's gap, which
is the cost the recovery phase exists to avoid. Pushing data and pulling repairs is what separates
this abstraction from the eager one.

The gossip beneath SHALL take its link as a parameter of the same kind as the recovery link, so
that both halves can run over the link a deployment supplies.

#### Scenario: A broadcast goes through the gossip

- **WHEN** the layer above broadcasts
- **THEN** the message is disseminated by the probabilistic broadcast beneath

#### Scenario: A request does not go through the gossip

- **WHEN** a process requests a missing message
- **THEN** the request travels over the link directly, and is not disseminated by the broadcast

#### Scenario: An answer goes back to the requester alone

- **WHEN** a process holding a requested message answers it
- **THEN** the answer is addressed to the requester, and to no other process

#### Scenario: Both halves run over the supplied link

- **WHEN** the capability is composed over a link that reports scope boundaries
- **THEN** both the gossip and the recovery traffic travel over that kind of link, and a boundary
  is reported upward exactly once whichever half observed it

## ADDED Requirements

### Requirement: Recovery bridges a session ending

A message lost because the session carrying it ended SHALL be recovered, provided some reachable
process stored it, and delivery from that sender SHALL continue in sequence.

#### Scenario: A gap opened by a session ending is closed

- **WHEN** a session ends with a data message in flight on it, and the next message from that
  sender arrives
- **THEN** the gap is detected, requested and repaired, and the receiver delivers both in sequence

### Requirement: Recovery traffic is bounded by the gaps

Request messages SHALL equal the fanout times the gaps detected plus the requests relayed — a
detected gap is gossiped to the fanout, as Algorithm 3.10 has it, and nothing else sends one —
answers SHALL not exceed the requests received by processes that had stored the message, and a
process with no gap and no request SHALL send nothing.

#### Scenario: Requests match gaps

- **WHEN** a run completes with one round of requests, so that none is relayed
- **THEN** the number of request messages sent is exactly the fanout times the number of distinct
  gaps detected, and the number of answers is at most the number of requests that reached a
  process holding the message

#### Scenario: Quiet means silent

- **WHEN** every gap is closed or skipped and every broadcast has finished relaying
- **THEN** no further message is sent

### Requirement: Identity is scoped to the originator's incarnation

The sender of a message SHALL be the originator in a particular incarnation, and the sequence
expected next, the messages held ahead of a gap and the copies stored SHALL be kept per sender. A
receiver SHALL keep state for at most two incarnations of one originator, retiring the oldest when a
third is heard from.

#### Scenario: A restarted originator's messages are delivered

- **WHEN** an originator crashes, restarts, and broadcasts again, so that its sequence numbers
  repeat ones every receiver has already delivered
- **THEN** the new messages are delivered in sequence, and are not dropped as already delivered

#### Scenario: A straggler from the previous incarnation still lands

- **WHEN** a message from an originator's previous incarnation arrives after its new incarnation
  has been heard from
- **THEN** it is delivered against the previous incarnation's sequence, and the new incarnation's
  state is undisturbed

#### Scenario: A third incarnation retires the oldest

- **WHEN** a receiver hears from a third incarnation of one originator
- **THEN** the oldest incarnation's sequence, pending and stored state is gone, and a message from
  it is treated as from a sender never heard from
