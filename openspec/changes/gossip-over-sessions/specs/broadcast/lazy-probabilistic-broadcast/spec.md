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
