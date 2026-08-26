## MODIFIED Requirements

### Requirement: State is unbounded, and this is a transcription

The durable record SHALL be understood to grow with every distinct message log-delivered. This
layer is a transcription of the page and inherits its omission of collection.

Each log-delivery SHALL cost **one append**, not a rewrite of everything recorded so far. A
protocol that log-delivers `n` messages therefore writes `O(n)` bytes over its life rather than
`O(n²)`. What remains unbounded is the record itself, and the in-memory index that answers "have I
seen this identifier before" — the second of which needs per-sender ordering to bound and is not
addressed here.

#### Scenario: The set grows with messages log-delivered

- **WHEN** a growing number of distinct messages is log-delivered
- **THEN** the durable record grows with them, and nothing retires an entry

#### Scenario: Recording one message costs one append

- **WHEN** a message is log-delivered
- **THEN** exactly one entry is appended, and nothing previously recorded is written again

#### Scenario: The write cost is linear, and this is checkable

- **WHEN** a run log-delivers many messages
- **THEN** the number of entries appended equals the number log-delivered, which the trace shows
  directly

### Requirement: A first start writes the empty log down

On starting with nothing in storage, this layer SHALL write its metadata, so that every later
restart of the process finds something and takes the recovery path rather than beginning afresh. It
SHALL NOT repeat that write on recovering, which would overwrite what was found.

#### Scenario: The first start is durable

- **WHEN** a process starts with nothing in storage
- **THEN** its metadata is written, without any message having arrived

#### Scenario: A crash before any message still recovers

- **WHEN** a process starts, writes nothing further, crashes and restarts
- **THEN** it finds its metadata and is recovered rather than initialised again

#### Scenario: Recovering does not repeat the initial write

- **WHEN** a process recovers
- **THEN** it does not write its metadata back unchanged

### Requirement: Recovery re-announces the log

On recovering, this layer SHALL read its durable record and notify the layer above, because the
notification it sent before crashing may have been lost with the incarnation that sent it.

Reading SHALL complete within the recovery handler, so that no message can arrive before the record
is loaded. A layer that had not finished loading would fail to recognise a message it had already
log-delivered, and log-deliver it a second time — the one thing this layer exists to prevent.

#### Scenario: The layer above is told again after a restart

- **WHEN** a process recovers
- **THEN** it is notified of what it read, without any message having arrived

#### Scenario: Nothing arrives before the record is loaded

- **WHEN** a process restarts with retransmissions already in flight to it
- **THEN** none is handled until recovery has read the record and returned
