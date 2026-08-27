# consensus/flooding-consensus Specification

## Purpose

Regular consensus in the fail-stop model: processes propose values and every correct process
decides the same one. Its agreement holds only while the failure detector it depends on is
perfect, which makes it the demonstration of what strong accuracy is worth rather than an
algorithm anyone would deploy.

## Requirements

### Requirement: Termination


Every correct process SHALL eventually decide some value, provided every correct process proposes
one.

#### Scenario: A decision is reached with no faults

- **WHEN** every process proposes and none crashes
- **THEN** every process eventually decides

#### Scenario: A decision is reached despite crashes

- **WHEN** processes crash during the run and at least one process remains correct
- **THEN** every correct process eventually decides

#### Scenario: Termination is bounded by the membership

- **WHEN** processes crash one after another, each in a separate round
- **THEN** a decision is still reached, in no more rounds than there are processes

### Requirement: Validity


If a process decides a value, that value SHALL have been proposed by some process.

#### Scenario: Nothing is invented

- **WHEN** a run completes
- **THEN** every decided value was proposed by some process in that run

#### Scenario: A sole proposal is the decision

- **WHEN** every process proposes the same value
- **THEN** that value is the one decided

### Requirement: Integrity


No process SHALL decide twice.

#### Scenario: One decision per process

- **WHEN** a run completes
- **THEN** each process reports at most one decision

#### Scenario: A later message does not re-decide

- **WHEN** a process has decided and afterwards receives a decision from another process
- **THEN** it does not decide again

### Requirement: Agreement while the failure detector is perfect


No two correct processes SHALL decide differently, **for as long as the failure detector makes no
mistake**. This layer depends on the detector's strong accuracy and nothing weaker: it has no
quorum discipline and no mechanism by which a decision, once taken, could be revised.

A correct process is one that does not crash, and the guarantee is stated between such processes.
A false suspicion does not remove a process from that set — it gives some other process a wrong
view of it. The failure is therefore between processes that are correct throughout, and it does
not require any process to be lost.

#### Scenario: Agreement holds when nobody is wrongly suspected

- **WHEN** the timing assumption that makes the detector perfect holds throughout a run, whatever
  crashes occur
- **THEN** no two correct processes decide differently

#### Scenario: Agreement holds when the deciding process crashes immediately afterwards

- **WHEN** a process decides and crashes before every other process has completed its round
- **THEN** the remaining correct processes still all decide, and decide the same value

#### Scenario: A false suspicion splits the decision

- **WHEN** the timing assumption is withdrawn so that a correct process is wrongly suspected
- **THEN** two correct processes may decide differently, and this is the stated limit of the
  guarantee rather than a defect

#### Scenario: The limit is a safety failure, not an inefficiency

- **WHEN** a false suspicion has split the decision
- **THEN** nothing in this layer detects or repairs the split, and it persists for the rest of the
  run

#### Scenario: The split outlives the system stabilising

- **WHEN** a false suspicion has split the decision and the conditions that caused it then pass,
  so that every process is again reachable by every other and would be held correct by every other
- **THEN** both decisions stand, because a decision is irrevocable and both were taken before
  stability returned

#### Scenario: The processes that disagree are correct throughout

- **WHEN** a false suspicion has split the decision
- **THEN** no process involved has crashed, and each wrongly held view of which processes are
  correct is a non-empty proper subset of the membership

### Requirement: The decision rule is deterministic and agreed in advance


Every process SHALL apply the same deterministic function to its accumulated proposal set, so that
two processes holding the same set decide the same value without further communication.

#### Scenario: The same set yields the same decision

- **WHEN** two processes hold identical proposal sets at the moment of deciding
- **THEN** they decide the same value

### Requirement: A round completes only when every process not detected as crashed has been heard from


A process SHALL NOT leave a round until it has received that round's message from every process it
has not been told has crashed. It SHALL decide at the end of a round in which the set of processes
heard from is unchanged from the previous round, and otherwise proceed to another round.

#### Scenario: A round waits for a slow but correct process

- **WHEN** one process's round message is delayed but the process has not been detected as crashed
- **THEN** the others do not leave the round until it arrives

#### Scenario: A newly detected crash forces another round

- **WHEN** a process is detected as crashed during a round
- **THEN** the round does not end in a decision, and another round follows

#### Scenario: No new crash permits a decision

- **WHEN** a round ends having heard from exactly the same processes as the previous round
- **THEN** a decision is taken

### Requirement: State is bounded by membership and rounds


This layer SHALL hold state proportional to the number of processes and the number of rounds, and
SHALL NOT grow with the number of messages handled. Rounds are themselves bounded by the number of
processes, because a round without a decision requires a newly detected crash.

#### Scenario: State does not grow with messages handled

- **WHEN** a run delivers many more messages than there are processes, through repeated rounds
- **THEN** this layer's state remains bounded by the membership and the rounds actually entered

### Requirement: One consensus instance decides once


This capability SHALL specify a single consensus instance: one proposal per process, one decision.
Deciding a sequence of values is a separate abstraction built on top and is not provided here.

#### Scenario: A second proposal is not a second consensus

- **WHEN** a process proposes after a decision has been reached
- **THEN** no second decision is reported by this instance

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

