## MODIFIED Requirements

### Requirement: Nothing is delivered that was not broadcast, and nothing twice

A process SHALL deliver a message at most once, and SHALL deliver only messages some process
broadcast. These hold always, not probabilistically.

Redundant *receipt* is expected and is not duplication: the algorithm's relay is deliberately not
guarded by the delivery check, so a process may receive the same message many times. What is
forbidden is delivering it upward more than once.

A broadcast's identity SHALL be scoped to the incarnation of its originator, so that an originator
which restarts does not name its new broadcasts as ones its receivers have already delivered.

#### Scenario: A message received many times is delivered once

- **WHEN** the same message arrives at a process repeatedly through different gossip paths
- **THEN** it is delivered upward exactly once

#### Scenario: Nothing is delivered that nobody broadcast

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast

#### Scenario: A restarted originator's broadcasts are delivered

- **WHEN** an originator crashes, restarts, and broadcasts again while its earlier identifiers are
  still within every receiver's window
- **THEN** the new broadcasts are delivered, and are not discarded as duplicates of the old

## ADDED Requirements

### Requirement: A broadcast costs what the algorithm specifies, and an idle process costs nothing

The messages sent SHALL equal exactly the fanout times one more than the number of receipts with
rounds still to live, over any run; and when nothing is lost, exactly the closed-form sum of the
fanout's powers up to the number of rounds, per broadcast. A process with nothing to relay SHALL
send nothing.

#### Scenario: The send count is the algorithm's

- **WHEN** a run completes over a link that loses nothing
- **THEN** the number of messages sent per broadcast equals the sum of the fanout's powers from one
  to the number of rounds, and no more

#### Scenario: An idle gossip is silent

- **WHEN** every broadcast has finished relaying and the run continues
- **THEN** no further message is sent

### Requirement: A session boundary is propagated and costs only what was in flight

Over a link that reports scope boundaries, this capability SHALL report each boundary upward once,
and SHALL lose only the messages that were in flight on the session that ended.

#### Scenario: An ending is reported and its cost counted

- **WHEN** a session ends while a broadcast is being relayed across it
- **THEN** the layer above is told the session ended, the messages in flight on it are the only
  ones lost, and delivery elsewhere continues
