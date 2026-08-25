## ADDED Requirements

### Requirement: A session network model

The simulator SHALL offer a mode in which communication between each pair of processes takes place
within a session. While a session holds, messages between connected, uncrashed processes SHALL be
delivered reliably, in the order sent, and without duplication. This mode is additional; the
fair-loss behaviour remains the default and is unchanged.

#### Scenario: Delivery within a session is reliable and ordered

- **WHEN** a run is session-based and one process sends several messages to another while their
  session holds
- **THEN** all of them are delivered, in the order they were sent, each exactly once

#### Scenario: The fair-loss default is unaffected

- **WHEN** a run is configured without requesting sessions
- **THEN** loss, duplication and reordering behave exactly as before

### Requirement: A session ends on disruption, losing an unknown suffix

A session SHALL end when the processes are partitioned, when either crashes, or when a break is
requested explicitly. On ending, an unknown suffix of the messages in flight SHALL be discarded,
and a new session SHALL begin at a higher epoch once communication is possible again.

#### Scenario: A break discards messages in flight

- **WHEN** messages are in flight between two processes and their session is broken
- **THEN** some suffix of those messages is never delivered

#### Scenario: A partition ends the session

- **WHEN** two processes with an established session are partitioned
- **THEN** their session ends

#### Scenario: A new session begins at a higher epoch

- **WHEN** a session between two processes ends and communication becomes possible again
- **THEN** a new session is established, and its epoch is greater than the previous one

#### Scenario: Ordering restarts with the new session

- **WHEN** a session ends and a new one is established
- **THEN** messages sent in the new session are delivered in their own order, independently of
  anything lost from the old one

### Requirement: Session events are visible in the trace

The simulator SHALL record session establishment, session ends and suffix losses in the trace, so
that a property can be asserted over them without inspecting protocol state.

#### Scenario: A session end is recorded

- **WHEN** a session ends for any reason
- **THEN** the trace records it, with the processes involved and the epoch that ended

#### Scenario: Discarded messages are distinguishable from delivered ones

- **WHEN** a session ends with messages in flight
- **THEN** the trace distinguishes those discarded by the session ending from those delivered
