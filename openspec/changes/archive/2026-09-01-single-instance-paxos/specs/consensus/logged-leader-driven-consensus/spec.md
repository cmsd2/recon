## Purpose

Paxos in the fail-recovery model: uniform consensus that survives processes crashing and coming
back, which is the version a deployment runs.

## ADDED Requirements

### Requirement: Agreement holds across crashes, recoveries, and a lying detector at once

No two processes SHALL decide different values in a run containing crashes, recoveries, and an
inaccurate leader detector.

Each of those alone is survivable by something simpler. Together they are what a deployed consensus
meets, and what this capability exists to claim.

#### Scenario: The three faults together do not split the decision

- **WHEN** a run injects crashes and recoveries while the timing assumption is withdrawn, so that
  correct processes are suspected and leadership is disputed
- **THEN** no two processes decide different values

#### Scenario: The run really contained all three

- **WHEN** that run is examined
- **THEN** at least one crash, at least one recovery, and more than one acting leader are observed,
  so the agreement above is not a claim about a quiet run

### Requirement: A decision survives the deciding process restarting

A process that has decided SHALL, after a crash and recovery, still hold that decision and SHALL NOT
decide differently.

#### Scenario: A decision is remembered across a restart

- **WHEN** a process decides, crashes, and recovers
- **THEN** it does not decide a different value

### Requirement: Progress resumes when enough processes return

Where a run loses its majority and later regains it through recovery, every correct process SHALL
eventually decide.

#### Scenario: A recovered majority decides

- **WHEN** a majority is lost to crashes and later restored by those processes recovering
- **THEN** every correct process eventually decides

#### Scenario: Without a majority it waits

- **WHEN** no majority is available and none recovers
- **THEN** no process decides, and safety is unaffected
