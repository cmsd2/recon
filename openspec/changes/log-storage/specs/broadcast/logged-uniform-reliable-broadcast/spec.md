## MODIFIED Requirements

### Requirement: Durable state is unbounded, and this is a transcription

The pending and log-delivered records SHALL be understood to grow with every message handled. This
layer is a transcription and inherits the book's omission of collection.

Recording a message SHALL cost **one append**, not a rewrite of everything recorded so far, so a
run handling `n` messages writes `O(n)` bytes rather than `O(n²)`. What remains unbounded is the
record itself and the in-memory state that indexes it.

#### Scenario: The durable sets grow with messages handled

- **WHEN** a growing number of messages is broadcast
- **THEN** the durable state grows with them, and nothing retires an entry

#### Scenario: Recording one message costs one append

- **WHEN** a message becomes pending, or is log-delivered
- **THEN** one entry is appended, and nothing previously recorded is written again

### Requirement: Pending messages and log-deliveries are durable; acknowledgements are not

The set of messages seen but not yet log-delivered, and the set log-delivered, SHALL survive a
crash. The record of which processes have acknowledged each message SHALL NOT be made durable,
because it is rebuilt by re-broadcasting what is pending on recovery.

Writing the acknowledgement record too would cost a write per acknowledgement to save work that
retransmission does anyway, and would make the durable state grow with traffic rather than with
messages.

#### Scenario: What survives is what is needed

- **WHEN** a process crashes and recovers
- **THEN** it holds the messages it had seen and those it had log-delivered, and holds no
  acknowledgements

#### Scenario: Acknowledgements are rebuilt by re-broadcasting

- **WHEN** a process recovers holding pending messages
- **THEN** it broadcasts each of them again, and acknowledgements accumulate as the responses
  arrive

#### Scenario: Recovery re-announces what was already log-delivered

- **WHEN** a process recovers
- **THEN** it reads what it had log-delivered and the layer above is notified of it

#### Scenario: Recovery reads before anything else reaches it

- **WHEN** a process restarts with broadcasts already in flight to it
- **THEN** none is handled until recovery has read what survived and returned
