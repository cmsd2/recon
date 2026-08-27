## MODIFIED Requirements

### Requirement: Every run produces an inspectable trace

The simulator SHALL record a trace containing, in order, each message sent, each delivery
outcome including drops and duplicates, each timer fired, and each indication raised, with the
virtual time and originating process for each entry.

A timer entry SHALL carry the handle of the timer that fired, so that a claim about *which* timer
fired can be settled from the trace rather than from protocol internals. The trace SHALL NOT be
parameterised by a timer type, having none to be parameterised by.

#### Scenario: Properties are asserted over the trace

- **WHEN** a run completes
- **THEN** the trace can be examined to decide whether a stated property held, without inspecting
  protocol internals

#### Scenario: Fault injection is visible in the trace

- **WHEN** a run is configured to inject faults
- **THEN** the trace distinguishes messages that were dropped or duplicated from those delivered
  normally

#### Scenario: Which timer fired is visible in the trace

- **WHEN** two layers of one process each have a timer outstanding and one of them fires
- **THEN** the trace names which, by the handle the registering layer was given

## ADDED Requirements

### Requirement: The run owns the source of timer identities

The simulator SHALL supply one source of timer identities per run and SHALL pass it to every
protocol it drives, so that identities are distinct across every layer of every process in that run.

A source owned per protocol, or begun afresh for each event, would hand two layers the same identity
and let each accept the other's expiry. Owning it at the run is what makes the guarantee that
identities do not collide something the driver provides rather than something each protocol must
arrange.

#### Scenario: Identities do not collide across a composition

- **WHEN** several layers of one process each register a timer during a run
- **THEN** every handle is distinct

#### Scenario: A run remains reproducible from its seed

- **WHEN** the same seed and configuration are run twice, with timers registered and fired
- **THEN** the two traces are identical, including which handle each timer entry names

#### Scenario: An expiry is delivered to the process that registered it

- **WHEN** a timer registered by one process fires
- **THEN** it is delivered to that process, and to no other
