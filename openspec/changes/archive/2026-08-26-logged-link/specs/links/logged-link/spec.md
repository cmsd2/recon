## Purpose

Perfect-link guarantees restated over *log-delivery*: instead of being told a message arrived, the
layer above is told that the durable record of arrivals has changed, and reads it. That is what
lets a restarted process know what it already delivered instead of delivering it again.

## ADDED Requirements

### Requirement: Delivery is logged, and the indication names the log

On receiving a message for the first time, this layer SHALL add it to a set held in stable
storage, and SHALL then notify the layer above that the set may have changed. The notification
SHALL NOT carry the message: a notification can be lost to a crash, and a message that exists only
in a lost notification is lost for ever.

#### Scenario: A first receipt is logged and then announced

- **WHEN** a message arrives that is not already in the durable set
- **THEN** it is added to the set, the set is made durable, and only then is the layer above
  notified

#### Scenario: The layer above reads rather than receives

- **WHEN** the layer above is notified
- **THEN** it can determine which messages have been log-delivered by reading the durable set

#### Scenario: The log is durable before the announcement

- **WHEN** a process crashes between logging a message and notifying the layer above
- **THEN** on recovery the message is in the retrieved set

### Requirement: Reliable delivery, conditioned on the sender not crashing

If a process that **never crashes** sends a message to a correct process, that process SHALL
eventually log-deliver it.

The condition is stronger than the crash-stop version's, and necessarily so: a sender that crashes
immediately after being asked to send may have no record that it was asked, and no process in the
system may ever have heard of the message.

#### Scenario: A message from a surviving sender arrives despite loss

- **WHEN** a process that does not crash sends to a correct process over a lossy network
- **THEN** the message is eventually log-delivered

#### Scenario: A sender crashing immediately after sending promises nothing

- **WHEN** a process is asked to send and crashes before the message reaches anyone
- **THEN** no delivery is required of any process, and this is the stated limit of the guarantee

### Requirement: No duplication, across incarnations

No message SHALL be log-delivered by a process more than once, **including across a restart**. The
record that makes this true is the durable set, which is why it is durable.

#### Scenario: A repeat receipt within one incarnation is not log-delivered again

- **WHEN** the same message arrives twice
- **THEN** the durable set does not change on the second arrival

#### Scenario: A repeat receipt after a restart is not log-delivered again

- **WHEN** a message is log-delivered, the process crashes and restarts, and the message arrives
  again because the sender is still retransmitting
- **THEN** it is not log-delivered a second time

#### Scenario: This is what the crash-stop link cannot promise

- **WHEN** the same schedule is run against a link whose record of deliveries is volatile
- **THEN** that link does log-deliver the message twice, and this contrast is what the durable
  record buys

### Requirement: No creation

If a process log-delivers a message with a named sender, that message SHALL have been previously
sent to it by that process.

#### Scenario: Log-deliveries match sends

- **WHEN** a run completes
- **THEN** every entry in every durable set corresponds to an earlier send by the named sender

### Requirement: A first start writes the empty log down

On starting with nothing in storage, this layer SHALL write its empty log, so that every later
restart of the process retrieves something and takes the recovery path rather than beginning
afresh. It SHALL NOT repeat that write on recovering, which would overwrite what was retrieved.

#### Scenario: The first start is durable

- **WHEN** a process starts with nothing in storage
- **THEN** an empty log is written, without any message having arrived

#### Scenario: A crash before any message still recovers

- **WHEN** a process starts, writes nothing further, crashes and restarts
- **THEN** it retrieves the empty log and is recovered rather than initialised again

#### Scenario: Recovering does not repeat the initial write

- **WHEN** a process recovers
- **THEN** it does not write the retrieved log back

### Requirement: Recovery re-announces the log

On recovering, this layer SHALL retrieve its durable set and notify the layer above, because the
notification it sent before crashing may have been lost with the incarnation that sent it.

#### Scenario: The layer above is told again after a restart

- **WHEN** a process recovers
- **THEN** it is notified of the retrieved set, without any message having arrived

### Requirement: State is unbounded, and this is a transcription

The durable set SHALL be understood to grow with every distinct message log-delivered. This layer
is a transcription of the page and inherits its omission of collection; the growth is now on disk,
which is worse than in memory rather than better.

#### Scenario: The set grows with messages log-delivered

- **WHEN** a growing number of distinct messages is log-delivered
- **THEN** the durable set grows with them, and nothing retires an entry
