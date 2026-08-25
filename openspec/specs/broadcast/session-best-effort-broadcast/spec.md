# broadcast/session-best-effort-broadcast Specification

## Purpose

Fan-out to every process over session links. It promises delivery to correct processes only while
the sessions carrying the message hold, and reports a session change rather than concealing it.

## Requirements

### Requirement: Validity within a session

If a correct process broadcasts a message, every correct process whose session with the sender
holds throughout SHALL eventually deliver it.

#### Scenario: All sessions hold

- **WHEN** a correct process broadcasts and no session ends
- **THEN** every process delivers the message exactly once

#### Scenario: A session ending is not repaired

- **WHEN** a message to some process is lost to a session ending
- **THEN** that process may never deliver it, and this layer does not retry

### Requirement: Session endings and establishments are reported to the layer above

When a link reports that a session with a peer ended, or that one was established, this layer SHALL
report it upward rather than absorbing it. It holds no per-message state and cannot repair the loss, so concealing
the change would deny the layers above the only signal they have.

#### Scenario: Both reports reach the layer above

- **WHEN** a session with a peer ends, and when a later one is established
- **THEN** the layer above is told of each, distinguishably

### Requirement: A directed send to one member

This layer SHALL offer, alongside the broadcast, a send addressed to one member. It is not part of
the module it transcribes, which has only a broadcast, and it adds no communication step: the same
wire message travels over the same link to strictly fewer recipients.

It exists so that a layer above can answer a session that has just come back without sending to
every other process as well, which would otherwise multiply the cost of every reconnection by the
size of the membership.

#### Scenario: Only the addressed member receives it

- **WHEN** a directed send names one member
- **THEN** that member receives the message and no other process does

### Requirement: No duplication and no creation

Each process SHALL deliver each broadcast at most once, and only if it was previously broadcast by
the process named as its sender.

#### Scenario: Deliveries match broadcasts

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast by the named sender, and no process
  delivered the same broadcast twice

### Requirement: State is bounded by membership

This layer SHALL hold state proportional to the number of processes, not to the number of messages
handled.

#### Scenario: State does not grow with messages

- **WHEN** a growing number of messages is broadcast
- **THEN** this layer's own state does not grow with them
