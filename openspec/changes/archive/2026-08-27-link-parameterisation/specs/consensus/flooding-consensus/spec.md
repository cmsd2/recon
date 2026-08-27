## ADDED Requirements

### Requirement: The broadcast beneath is a parameter

Consensus SHALL be written against the port of the broadcast beneath it and SHALL NOT name an
implementation. It composes over any broadcast satisfying that port, including one carried by a link
this project did not write.

Its guarantees continue to rest on the failure detector's timing assumption and on what the layer
beneath can carry; parameterising the layer beneath changes neither.

#### Scenario: The ordinary stack is unchanged

- **WHEN** consensus is used without naming the layer beneath
- **THEN** it composes as it did before this change, and agreement and termination are unchanged

#### Scenario: Consensus decides over a link the project never wrote

- **WHEN** consensus is composed over a broadcast carried by a link supplied by an application, and
  every correct process proposes
- **THEN** every correct process decides, no two decide differently, and what is decided was
  proposed
- **AND** neither the link nor the consensus implementation was modified to achieve it
