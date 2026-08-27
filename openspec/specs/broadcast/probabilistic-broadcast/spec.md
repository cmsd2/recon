## Purpose

Delivery to every correct process *with high probability*, by gossip: each process relays what it
receives to a random subset of its peers for a bounded number of rounds. It buys scalability over
the certainty of reliable broadcast, and this capability states what that trade actually is.

## Requirements

### Requirement: A broadcast reaches every correct process with high probability


A broadcast SHALL reach every correct process with a probability determined by the fanout, the
number of rounds, and the membership size. It SHALL NOT be required to reach every correct process
on every run.

This is the whole of what separates this abstraction from best-effort broadcast, and it is a
weakening rather than a strengthening: best-effort broadcast reaches everyone whenever the sender is
correct, and this does not. What is bought is that no process sends to all of `Π`.

A run in which some correct process never delivers SHALL NOT be reported as a violation.

#### Scenario: A broadcast usually reaches everyone

- **WHEN** a correct process broadcasts, over a link that loses nothing, with a fanout and round
  count sufficient for the membership
- **THEN** every correct process delivers it on the large majority of runs

#### Scenario: Failing to reach everyone is not a violation

- **WHEN** a run leaves a correct process without a message
- **THEN** that run satisfies this capability, and the outcome is recorded rather than treated as
  a failure

### Requirement: The probabilistic guarantee is evidenced over many runs


The guarantee SHALL be asserted over many runs rather than one. A test asserting it SHALL state the
threshold it requires and the fanout, round count and membership the threshold is derived from, so
that a reader can tell a genuine regression from a re-tuning.

Each run in such a sweep SHALL remain individually reproducible from its seed, so that a run below
the threshold can be replayed and examined.

The assertion SHALL have a non-vacuity half: it SHALL also check that coverage is **not** total. A
configuration that reaches every process on every run is not exercising the probabilistic path, and
an assertion that cannot fail is the failure mode this project already guards against elsewhere.

#### Scenario: Coverage is asserted against a stated threshold

- **WHEN** the delivery property is asserted
- **THEN** it is measured across many seeds and compared against a threshold the test states
- **AND** the fanout, rounds and membership that threshold follows from are stated with it

#### Scenario: A run below the threshold can be reproduced

- **WHEN** a sweep finds a run in which some correct process did not deliver
- **THEN** that run replays identically from its seed

#### Scenario: The assertion is not satisfied by certainty

- **WHEN** the configuration under test reaches every correct process on every run
- **THEN** the assertion fails, because it is no longer evidence of a probabilistic guarantee

### Requirement: Nothing is delivered that was not broadcast, and nothing twice


A process SHALL deliver a message at most once, and SHALL deliver only messages some process
broadcast. These hold always, not probabilistically.

Redundant *receipt* is expected and is not duplication: the algorithm's relay is deliberately not
guarded by the delivery check, so a process may receive the same message many times. What is
forbidden is delivering it upward more than once.

#### Scenario: A message received many times is delivered once

- **WHEN** the same message arrives at a process repeatedly through different gossip paths
- **THEN** it is delivered upward exactly once

#### Scenario: Nothing is delivered that nobody broadcast

- **WHEN** a run completes
- **THEN** every delivery corresponds to an earlier broadcast

### Requirement: Relaying is bounded by rounds and stops


Every relayed message SHALL carry a rounds-to-live count, decremented at each hop, and a process
SHALL NOT relay a message whose count is exhausted. A broadcast SHALL therefore generate a finite
number of transmissions and the run SHALL fall silent.

#### Scenario: Gossip terminates

- **WHEN** a single message is broadcast and the run is allowed to continue well beyond the point
  of delivery
- **THEN** transmissions cease, rather than continuing for as long as the run does

#### Scenario: The relay reaches a bounded number of hops

- **WHEN** a message is relayed
- **THEN** the number of hops it travels is bounded by the configured round count

### Requirement: No process sends to the whole membership


A process SHALL relay to a randomly chosen subset of its peers whose size is the configured fanout,
not to all of them. Fanout SHALL be configurable, and the choice SHALL come from the randomness the
protocol is supplied rather than from any other source.

Sending to everyone is best-effort broadcast, which this repository already has; a probabilistic
broadcast that fans out to all of `Π` has paid the cost of uncertainty and bought nothing.

#### Scenario: A relay addresses fanout peers, not all peers

- **WHEN** a process relays a message in a membership larger than the fanout
- **THEN** it addresses exactly the fanout number of peers, and not the whole membership

#### Scenario: The peer choice is reproducible

- **WHEN** the same seed and configuration are run twice
- **THEN** each process chooses the same peers in the same order

### Requirement: State is bounded by a retention window


The set a process keeps in order to recognise a message it has already delivered SHALL be bounded
by a configured retention window, and SHALL NOT grow with the number of messages handled.

The book omits garbage collection deliberately, so the window is this project's own design and its
consequences belong here rather than in a footnote. Reclaiming SHALL NOT cost time proportional to
everything ever received: a per-event pass over the whole set is the specific defect this
requirement exists to forbid.

Bounding the set scopes the no-duplication guarantee: a message re-arriving after its record has
been reclaimed SHALL be delivered again. The window is therefore a statement about how long
no-duplication holds, and SHALL be described that way.

#### Scenario: State does not grow with messages handled

- **WHEN** a process handles a number of messages far exceeding the retention window
- **THEN** the state it keeps for deduplication stays bounded by that window

#### Scenario: Reclaiming does not cost more as the run goes on

- **WHEN** a process has handled many messages
- **THEN** the work done to reclaim expired records on any single event does not grow with the
  number handled

#### Scenario: No duplication holds within the window and not beyond it

- **WHEN** a message re-arrives after its record has left the retention window
- **THEN** it is delivered a second time, which is the stated scope of the guarantee rather than a
  violation of it

### Requirement: The link beneath is a parameter


This capability SHALL compose over any link satisfying the link port, and SHALL NOT name a
particular link implementation.

#### Scenario: The same module runs over more than one link

- **WHEN** the protocol is composed over a link that reports scope boundaries, and over one that
  does not
- **THEN** both compose, and the protocol is written once

