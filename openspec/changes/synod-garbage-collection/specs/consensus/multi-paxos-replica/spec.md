## ADDED Requirements

### Requirement: A replica reports how far it has applied

A replica SHALL periodically make known how far it has applied the ordered sequence, so that the
consensus beneath it can discard what enough replicas have already taken.

This is the source's §4.2: "much state, and work, can be saved if each replica periodically updates
leaders and acceptors about its `slot out` variable". Without it nothing below the replica can ever
collect, because nothing else knows what has been applied.

The report SHALL be periodic rather than a consequence of doing work. A replica that is applying
nothing is exactly the one whose position the others most need, and a report carried on the
replica's own traffic would fall silent when the traffic did.

#### Scenario: A replica's progress reaches the consensus beneath it

- **WHEN** a replica applies entries
- **THEN** how far it has applied becomes known to the consensus layer at every member

#### Scenario: An idle replica still reports

- **WHEN** a replica applies nothing for a period
- **THEN** it still makes its position known, so that collection elsewhere is not blocked by its
  silence

## MODIFIED Requirements

### Requirement: Nothing is invented, and nothing is ordered twice

Every entry in the ordered sequence SHALL be one some process appended. A value appended once SHALL
occupy exactly one position, however many slots its command was decided in, **for as long as the
retention window keeps the record that says so**.

The second half is the source's `perform` guard, and the source is explicit that it is bounded by
time rather than by anything else: because different replicas may propose the same command for
different slots, one command can be decided more than once, and "in practice, it is often sufficient
if such information is only kept for a certain amount of time, making the probability of duplicate
execution negligible". So this is a guarantee with a scope, and the scope SHALL be stated in the
module rather than left for a reader to discover.

The window SHALL NOT be bounded by the collection watermark used beneath. That watermark says
`f + 1` replicas have applied up to a slot, which says nothing about whether a command decided below
it may still be decided again above it; a duplicate filter has to outlive every slot a duplicate
could span.

Two *distinct* appends of the same value SHALL occupy two positions, within the window and outside
it alike — they are different requests, and collapsing them would lose one.

#### Scenario: Nothing appears that was not appended

- **WHEN** any process reports its ordered sequence
- **THEN** every entry in it was appended by some process in the run

#### Scenario: A command decided in two slots occupies one position

- **WHEN** the same command is decided for more than one slot, and both decisions fall within the
  retention window
- **THEN** it appears exactly once in the ordered sequence

#### Scenario: The module states the window and what it costs

- **WHEN** a reader consults the module's documentation
- **THEN** it says the duplicate filter is bounded by a retention window, and what a duplicate
  arriving after the window would cause

#### Scenario: The same value appended twice occupies two positions

- **WHEN** a process appends the same value twice
- **THEN** both appends take their own position in the sequence

### Requirement: The state is bounded, and this is an implementation

The bookkeeping a replica keeps SHALL NOT grow with the number of commands handled: the decisions it
holds and the record of what it has applied are bounded by the retention window, and the requests
and proposals outstanding are bounded by the window ahead of what it has applied.

The **ordered sequence itself is exempt and SHALL be stated as such**. A log grows with what is
appended to it; that is the data rather than the bookkeeping, and a log that discarded it would not
be a log. The module SHALL say so, and SHALL name what would bound it if a caller needed that,
rather than leaving a reader to think the exemption was an oversight.

#### Scenario: The bookkeeping does not grow with commands handled

- **WHEN** a run appends many more commands than another, with the retention window unchanged
- **THEN** the decisions and applied-record a replica holds are bounded by the same figure in both

#### Scenario: The module states which part is exempt and why

- **WHEN** a reader consults the module's documentation
- **THEN** it says the ordered sequence is not bounded, that it is the data rather than the
  bookkeeping, and what would bound it
