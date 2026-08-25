## Purpose

Fan-out to every process over session links. It promises delivery to correct processes only while
the sessions carrying the message hold, and reports a session change rather than concealing it.

## ADDED Requirements

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
