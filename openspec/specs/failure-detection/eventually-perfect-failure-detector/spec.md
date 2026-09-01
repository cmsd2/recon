# failure-detection/eventually-perfect-failure-detector Specification

## Purpose

◇P: a failure detector that may be wrong for a while and takes it back, and whose timeout adapts to
the network in both directions within a stated bound. This is the detector a deployment can run,
where the perfect one needs a delivery bound known in advance.

## Requirements

### Requirement: A crashed process is eventually suspected by everyone, permanently

There SHALL be a time after which every correct process suspects every crashed process, and does not
retract that suspicion.

#### Scenario: A crash is detected everywhere

- **WHEN** a process crashes and does not return
- **THEN** every correct process eventually suspects it

#### Scenario: A crash is not retracted

- **WHEN** a process has crashed and remains crashed
- **THEN** no process that suspects it stops suspecting it

### Requirement: A suspicion of a correct process is retracted

A suspicion SHALL be withdrawn when the suspected process is heard from again, and the withdrawal
SHALL be reported to the layer above as its own event.

This is the whole difference from the perfect failure detector. A layer above that keeps a set of
processes it believes correct must be able to put one back.

#### Scenario: A process wrongly suspected is restored

- **WHEN** a correct process is suspected because its messages were slow or lost, and is then heard
  from again
- **THEN** the suspicion is withdrawn and the layer above is told

#### Scenario: A recovered process is restored

- **WHEN** a crashed process restarts and resumes communicating
- **THEN** the processes that suspected it stop suspecting it, and are told

#### Scenario: Nothing is reported when nothing changed

- **WHEN** the suspected set is unchanged between rounds
- **THEN** no suspicion and no withdrawal is reported

### Requirement: Eventual accuracy, within a stated bound

There SHALL be a time after which no correct process is suspected, **provided** the network's true
delay bound is within the configured maximum and eventually stops changing.

Both conditions are departures from Algorithm 2.7 and both SHALL be stated in the module rather than
left as the reader's inference. The unconditional guarantee requires a timeout that grows without
bound, which is what this capability declines to do.

#### Scenario: Accuracy is reached when the network settles within the bound

- **WHEN** the network's delay stops changing at a value below the configured maximum
- **THEN** every correct process is eventually not suspected by anyone

#### Scenario: Accuracy is lost when the network exceeds the bound

- **WHEN** the network's delay settles above the configured maximum
- **THEN** correct processes continue to be suspected, and this is the stated condition failing
  rather than the implementation

### Requirement: The timeout adapts in both directions and is bounded

The timeout SHALL increase when a suspicion is found to have been wrong, SHALL decrease after a
period in which **nothing was suspected**, SHALL never fall below a configured floor, and SHALL
never exceed a configured maximum.

Algorithm 2.7 increases and never decreases. A timeout that only grows leaves detection permanently
slower after any bad period, with nothing reporting that it has — a liveness failure that does not
clear when the network does.

The condition for decreasing SHALL be that nothing is suspected, and not merely that no suspicion
was withdrawn. A network bad enough that suspicions are never taken back produces no withdrawals, so
the weaker condition eases the timeout off exactly while the detector is being consistently wrong.
The consequence — a permanently suspected crashed process freezes the timeout where it reached —
SHALL be stated rather than left to be discovered.

#### Scenario: A false suspicion raises the timeout

- **WHEN** a process is suspected and then heard from
- **THEN** the timeout is longer for the following round

#### Scenario: Sustained accuracy lowers the timeout

- **WHEN** several consecutive rounds pass with nothing suspected
- **THEN** the timeout is shorter, and not below the floor

#### Scenario: A bad network does not lower the timeout

- **WHEN** processes are suspected round after round and never heard from, so no suspicion is
  withdrawn
- **THEN** the timeout is not lowered

#### Scenario: The timeout does not thrash

- **WHEN** the network's delay rises and then falls again
- **THEN** the timeout follows it up and then down, rising faster than it falls, and settles

#### Scenario: The timeout is bounded above

- **WHEN** the network is bad enough for long enough that the timeout would otherwise grow without
  limit
- **THEN** it reaches the configured maximum and never passes it

The maximum is a ceiling rather than a resting place: once suspicions clear, the decrease moves the
timeout back down, and a run observes the trajectory rather than a final value.

### Requirement: State is bounded by membership and the send rate does not grow

State SHALL be one entry per process and nothing per message, and the messages sent per unit time
SHALL NOT grow with how long the detector has been running.

#### Scenario: The send rate is flat

- **WHEN** the detector runs for several times longer than a round
- **THEN** the number of messages sent per window of time does not increase

### Requirement: The detector satisfies a port that Ω can be written against

This capability and the perfect failure detector SHALL both satisfy one interface, stating what a
layer above may depend on: that an indication is either a suspicion or its withdrawal.

#### Scenario: Both detectors satisfy the port

- **WHEN** a layer above is written against the port
- **THEN** it composes over either detector without naming which

#### Scenario: A detector that never retracts says so by never producing one

- **WHEN** the perfect failure detector is used through the port
- **THEN** it yields suspicions and never a withdrawal
