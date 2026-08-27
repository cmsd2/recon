## ADDED Requirements

### Requirement: The broadcast beneath is a parameter

This protocol SHALL be written against the port of the broadcast beneath it and SHALL NOT name an
implementation.

Where the layer beneath reports that a scope was re-established, this protocol SHALL resend what is
pending to the peer whose scope returned. That resend SHALL be unconditional rather than filtered by
which peers have been seen to acknowledge, and directed rather than broadcast: acknowledgements
record who relayed to this process, not whether this process's own relay arrived, so filtering by
them deadlocks.

Where no such report is possible, the quorum alone carries liveness, and nothing is resent.

#### Scenario: The ordinary stack is unchanged

- **WHEN** this protocol is used without naming the layer beneath
- **THEN** it composes as it did before this change

#### Scenario: A re-established scope prompts an unconditional directed resend

- **WHEN** the layer beneath reports a scope established with one peer while messages are pending
- **THEN** every pending message is sent to that peer, whether or not that peer has been seen to
  acknowledge it, and to no other peer

#### Scenario: No peer is ever accused

- **WHEN** a peer never returns
- **THEN** delivery proceeds on the quorum alone, and no judgement about that peer is made or
  required
