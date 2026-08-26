## Purpose

Uniform agreement in the fail-recovery model: a message log-delivered by any process, whether it
later crashes or not, is eventually log-delivered by every correct process — where a correct
process is one that always recovers from its crashes, and what it knows is what it wrote down.

## ADDED Requirements

### Requirement: Validity

If a process that never crashes broadcasts a message, every correct process SHALL eventually
log-deliver it, provided a majority of processes are correct.

#### Scenario: A broadcast reaches everyone when nothing fails

- **WHEN** a process broadcasts and no process crashes
- **THEN** every process eventually log-delivers it

#### Scenario: A minority crashing and recovering does not prevent it

- **WHEN** fewer than half the processes crash and recover during the run
- **THEN** every correct process still eventually log-delivers the message

### Requirement: Uniform agreement across incarnations

If a message is log-delivered by any process, whether it subsequently crashes or not, it SHALL
eventually be log-delivered by every correct process, provided a majority of processes are
correct.

#### Scenario: A process log-delivers and then crashes for ever

- **WHEN** a process log-delivers a message and then crashes and never recovers
- **THEN** every correct process eventually log-delivers that message

#### Scenario: A process log-delivers, crashes, and recovers

- **WHEN** a process log-delivers a message, crashes, and recovers
- **THEN** it still holds that message, and does not log-deliver it a second time

### Requirement: No duplication and no creation, across incarnations

Each process SHALL log-deliver each broadcast at most once including across restarts, and only if
it was previously broadcast by the process named as its sender.

#### Scenario: A restart does not re-deliver

- **WHEN** a process crashes and recovers after log-delivering a message
- **THEN** the durable record shows it once, and no second log-delivery occurs

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
- **THEN** the layer above is notified of the retrieved log-delivered set

### Requirement: Delivery waits for a majority

A message SHALL be log-delivered once more than half of the processes have been seen to
re-broadcast it. This layer SHALL NOT consult a failure detector and SHALL NOT maintain a set of
processes believed correct.

#### Scenario: A bare majority is enough

- **WHEN** more than half the processes have re-broadcast a message
- **THEN** it is log-delivered

#### Scenario: Half is not enough

- **WHEN** exactly half the processes have re-broadcast a message
- **THEN** it is not yet log-delivered

### Requirement: The assumption is a correct majority, and its failure blocks rather than diverges

The guarantees SHALL hold whenever more than half the processes are correct. When that assumption
fails, this layer SHALL cease to log-deliver rather than log-deliver inconsistently.

#### Scenario: Without a majority nothing new is log-delivered

- **WHEN** half or more of the processes are crashed and not recovering
- **THEN** messages not already log-delivered are not log-delivered, and no process holds a
  message another correct process will never hold

#### Scenario: Progress resumes when a majority recovers

- **WHEN** crashed processes recover so that a majority is available again
- **THEN** the messages that were waiting are log-delivered

### Requirement: Durable state is unbounded, and this is a transcription

The pending and log-delivered sets SHALL be understood to grow with every message handled. This
layer is a transcription and inherits the book's omission of collection; the growth is on disk.

#### Scenario: The durable sets grow with messages handled

- **WHEN** a growing number of messages is broadcast
- **THEN** the durable state grows with them, and nothing retires an entry
