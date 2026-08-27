## ADDED Requirements

### Requirement: The broadcast beneath is a parameter

This protocol SHALL be written against the port of the broadcast beneath it and SHALL NOT name an
implementation.

Its liveness depends on what the layer beneath can tell it. Where the layer beneath reports that a
scope was re-established, this protocol SHALL resend what that peer has not been seen to
acknowledge, and that resend SHALL be directed at the peer whose scope returned rather than
broadcast to every member. Where no such report is possible, liveness rests on the failure detector
alone, as it does today.

#### Scenario: The ordinary stack is unchanged

- **WHEN** this protocol is used without naming the layer beneath
- **THEN** it composes as it did before this change, and its guarantees are unchanged

#### Scenario: An established scope prompts a directed resend

- **WHEN** the layer beneath reports that a scope with one peer was established, and messages are
  pending that the peer has not been seen to acknowledge
- **THEN** those messages are sent to that peer, and to no other

#### Scenario: Nothing is attempted on an ending

- **WHEN** the layer beneath reports that a scope ended
- **THEN** nothing is sent to that peer, because nothing sent then could arrive

#### Scenario: Progress does not require the peer to return

- **WHEN** a peer never returns and the detector accuses it
- **THEN** delivery proceeds among the remaining correct processes
