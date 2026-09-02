# consensus/logged-uniform-total-order-broadcast Specification

## Purpose

The book's fail-recovery total-order broadcast: the same agreed sequence, over logged uniform
reliable broadcast and logged uniform consensus, where what a process has ordered survives its
restart. The member of the pair a deployment would care about.

## Requirements

### Requirement: The ordered sequence survives a restart

A process that has ordered a sequence of entries and then crashes SHALL, on recovering, hold that
same sequence rather than beginning empty.

This is what separates a log from a cache, and it is the reason the pair behind the port is worth
having: the crash-stop member makes the same ordering claim and cannot keep it across a failure.

#### Scenario: A restarted process still has what it ordered

- **WHEN** a process orders entries, crashes, and restarts
- **THEN** it holds the entries it had ordered, in the same order

#### Scenario: A restarted process agrees with one that never failed

- **WHEN** a process crashes and recovers while others continue
- **THEN** its sequence and theirs remain prefixes of one another

#### Scenario: A process that dies inside a write recovers consistently

- **WHEN** a process crashes during a durable write and later recovers
- **THEN** it holds a sequence that is a prefix of the agreed one, whether or not the write landed

### Requirement: A round's proposal is durable before it is made

What a process proposes for a round SHALL be written down before the proposal is visible to anyone
else, and SHALL be re-proposed on recovery for a round that had not decided.

A process that proposed and then forgot would, on recovering, propose something different for the
same round — and a uniform consensus that has already decided cannot accommodate it. Re-proposing
what was recorded is how the page resumes.

#### Scenario: A recovering process resumes an undecided round

- **WHEN** a process crashes with a round outstanding and recovers
- **THEN** it proposes for that round what it had recorded, rather than something new

#### Scenario: The proposal is written before it is sent

- **WHEN** a process proposes for a round
- **THEN** the record of that proposal is durable before any process could observe it

### Requirement: The same ordering guarantees as the crash-stop member

Every ordering property the crash-stop member of this pair holds SHALL hold here: one sequence agreed
by all, entries appended by correct processes eventually delivered, nothing delivered that was not
appended, and nothing delivered twice.

They are held to one suite, written against the port, so that where the two differ is visible rather
than asserted. What differs is what survives a restart, and nothing else.

#### Scenario: The shared suite passes against this implementation

- **WHEN** the properties asserted against the port are run against this implementation
- **THEN** they hold

### Requirement: The state is unbounded, and this is a transcription

As with the crash-stop member, the unordered set and the ordered sequence SHALL be permitted to grow
with the number of entries handled — and here the sequence grows in **stable storage** as well as in
memory.

Stated rather than fixed: this is a transcription of the page, and the page omits garbage collection.

#### Scenario: The module states its own space bound

- **WHEN** a reader consults the module's documentation
- **THEN** it says the state is unbounded, that the durable half is unbounded too, and that the
  module is a transcription
