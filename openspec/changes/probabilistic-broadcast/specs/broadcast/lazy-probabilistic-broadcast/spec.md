## Purpose

Gossip with recovery: a process that spots a gap in a sender's sequence asks a peer for what it
missed, rather than accepting the loss. It converts the eager algorithm's occasional misses into
delays, for as long as some peer still holds the message.

## ADDED Requirements

### Requirement: A gap in a sender's sequence is detected and requested

Each broadcast SHALL carry its sender's identity and a sequence number that advances by one per
broadcast from that sender. A process that receives a sequence number ahead of the next it expects
from that sender SHALL treat the intervening numbers as missing, and SHALL request them.

A request SHALL be addressed to a peer rather than to the whole membership, so that recovery costs
less than the broadcast it repairs.

#### Scenario: A gap prompts a request

- **WHEN** a process receives sequence number `n` from a sender while expecting `k`, with `k < n`
- **THEN** it requests the messages numbered `k` through `n − 1`

#### Scenario: A message arriving in order prompts no request

- **WHEN** a process receives the sequence number it was expecting
- **THEN** it requests nothing

#### Scenario: A request is not a broadcast

- **WHEN** a process requests a missing message
- **THEN** the request is addressed to a subset of peers rather than to the whole membership

### Requirement: A message ahead of the gap is held, not dropped and not delivered

A message whose sequence number is ahead of the next expected from its sender SHALL be retained and
SHALL NOT be delivered upward until the gap before it is closed. Delivering it immediately would
report an order the sender did not produce; dropping it would waste a message already received.

#### Scenario: An out-of-order message waits

- **WHEN** a message arrives ahead of a gap
- **THEN** it is not delivered upward at that moment, and is not discarded

#### Scenario: Closing the gap releases what was waiting

- **WHEN** the missing messages arrive and the gap closes
- **THEN** the held messages are delivered upward, in sequence order

### Requirement: A gap that cannot be closed does not block delivery for ever

A process that has requested a missing message and not received it SHALL, after a configured
period, cease waiting for it and deliver what it holds beyond the gap.

Without this the abstraction would convert a probabilistic miss into a permanent stall, which is
worse than the miss. The skipped message SHALL NOT be delivered later if it subsequently arrives:
having moved past the gap, the process has reported an order that a late delivery would contradict.

#### Scenario: Waiting ends

- **WHEN** a requested message has not arrived after the configured period
- **THEN** the process delivers the messages it holds beyond the gap

#### Scenario: A skipped message stays skipped

- **WHEN** a message arrives after the process has already moved past its position
- **THEN** it is not delivered

### Requirement: Recovery depends on some peer having stored the message

A process SHALL store a copy of some fraction of the messages it receives, so that it can answer a
request. The fraction SHALL be configurable, including the setting under which every process stores
every message.

Storing at every process is the certain case and the expensive one; storing at a fraction is the
trade this algorithm exists to offer. The guarantee that a gap is repairable therefore holds only
while some process that stored the message is reachable, and SHALL be stated that way rather than
absolutely.

#### Scenario: A request is answered by a process that stored the message

- **WHEN** a process requests a missing message and a peer holding it receives the request
- **THEN** that peer sends the message back to the requester

#### Scenario: Every process storing everything is expressible

- **WHEN** the storing fraction is configured to its maximum
- **THEN** every process stores every message it receives, and any peer can answer any request

#### Scenario: A gap nobody stored is not repaired

- **WHEN** no reachable process stored a missing message
- **THEN** the gap is not repaired, and the process moves past it rather than waiting for ever

### Requirement: Recovery makes delivery more likely than gossip alone

Under a configuration where messages are lost, the fraction of runs in which every correct process
delivers SHALL be higher with recovery than without it.

This is the reason the abstraction exists, and stating it as a requirement makes it something the
suite has to show rather than assert. It SHALL be evidenced the same way the underlying
probabilistic guarantee is: over many runs, against a stated threshold, with each run reproducible
from its seed.

#### Scenario: Recovery raises coverage

- **WHEN** the same seeds, membership and loss rate are run with recovery and with gossip alone
- **THEN** the fraction of runs reaching every correct process is higher with recovery

#### Scenario: The comparison is not vacuous

- **WHEN** the comparison is made
- **THEN** the configuration is one in which gossip alone demonstrably fails on some runs, so that
  there is something for recovery to improve

### Requirement: The stored copies and the pending messages are bounded

The messages a process stores for answering requests, and those it holds while waiting for a gap to
close, SHALL both be bounded by a configured retention window rather than growing with the number
of messages handled.

The book omits this explicitly, so it is this project's design. Reclaiming SHALL NOT cost time
proportional to everything ever received.

A request for a message whose stored copy has been reclaimed SHALL be answered as unavailable
rather than by an unbounded search, and the requester SHALL then move past the gap as it would for
any gap it cannot close.

#### Scenario: Neither collection grows with messages handled

- **WHEN** a process handles a number of messages far exceeding the retention window
- **THEN** both the stored copies and the pending messages stay bounded by that window

#### Scenario: A request beyond the window is not repairable

- **WHEN** a process requests a message old enough to have left every peer's retention window
- **THEN** no copy is returned, and the requester moves past the gap

### Requirement: The gossip beneath is a parameter

This capability SHALL compose over a probabilistic broadcast satisfying the port beneath it, and
SHALL NOT name a particular implementation of it.

#### Scenario: The layer composes over the gossip abstraction, not a named module

- **WHEN** this layer is written
- **THEN** it names the abstraction beneath it and no particular implementation of that abstraction
