## MODIFIED Requirements

### Requirement: The network provides fair-loss semantics with configurable faults

The simulator SHALL act as the fair-loss link layer. It SHALL support configuring message loss,
duplication, reordering, delivery delay, and severed connectivity between processes.
Under any configuration that does not permanently drop all messages between two correct processes,
a message retransmitted infinitely often SHALL eventually be delivered.

Connectivity SHALL be a property of a **pair** of processes rather than of a grouping, so that
reachability need not be transitive: a network in which `A` reaches `B` and `B` reaches `C` while `A`
does not reach `C` SHALL be expressible. Partitioning into groups SHALL remain available and SHALL
mean severing every pair that spans two groups.

A test SHALL be able to ask whether two processes can currently reach each other, so that the
topology it built can be asserted rather than assumed.

#### Scenario: Messages are dropped at the configured rate

- **WHEN** a run is configured with a non-zero loss rate and many messages are sent
- **THEN** the trace records losses, and the observed rate is consistent with the configuration

#### Scenario: A partition prevents delivery

- **WHEN** two processes are placed in disjoint partitions
- **THEN** no message sent between them is delivered while the partition holds

#### Scenario: A healed partition permits delivery again

- **WHEN** a partition is removed and a message is retransmitted afterward
- **THEN** delivery becomes possible again

#### Scenario: A severed pair prevents delivery in both directions

- **WHEN** connectivity between two processes is severed
- **THEN** no message between them is delivered in either direction while it stays severed, and
  messages to and from every other process are unaffected

#### Scenario: Reachability need not be transitive

- **WHEN** connectivity is severed between two processes but each still reaches a third
- **THEN** messages between each of them and the third are delivered, and messages between the two
  are not

#### Scenario: A test can ask what is reachable

- **WHEN** a test builds a topology
- **THEN** it can ask whether any two processes reach each other, and the answer reflects every
  severing and healing applied so far

#### Scenario: Retransmission overcomes loss

- **WHEN** a correct process retransmits a message indefinitely to a correct process over a lossy
  but not partitioned network
- **THEN** the message is eventually delivered
