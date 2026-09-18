# consensus/multi-paxos-replica Specification

## Purpose

The replica of *Paxos Made Moderately Complex* §2.1: how a request becomes a slot, what a replica
may propose and when, when a decision may be applied, and what the resulting ordered sequence
guarantees. This is what turns the Synod protocol's per-slot agreement into a replicated log, and it
is the member of the `TotalOrderLog` port that pays one consensus for a stable leader rather than
one per entry.

## Requirements

### Requirement: Every process sees one ordered sequence

Where two processes each report an entry at the same position of the ordered sequence, those
entries SHALL be equal. This SHALL hold whatever the schedule, whatever proposals were made
concurrently, and whatever processes have crashed.

This is the source's Invariant R1 seen through the log: two replicas that have applied the same
number of commands hold the same state. It is not a new claim on top of the consensus beneath —
it is the same claim as "at most one proposal is chosen per slot", made visible at the port where a
checker can read it. What the replica adds is that a *position* is not a slot: a command decided in
two slots occupies one position, so agreement on slots is not by itself agreement on the sequence.

#### Scenario: Two processes never disagree at a position

- **WHEN** several processes append concurrently and every process reports its ordered sequence
- **THEN** no two reports differ at any position they both have

#### Scenario: A read anywhere is a prefix of a read anywhere else

- **WHEN** two processes are read at any two instants in a run
- **THEN** one result is a prefix of the other

### Requirement: A request that is appended is eventually ordered, once a leader settles

Where a process appends a value and the eventual leader detector settles on one correct process
that the appending process can reach, that value SHALL eventually take a position in the ordered
sequence at every correct process.

Progress is conditional, and inherits every condition the consensus beneath it carries. What this
capability adds is a condition of its own: a request whose proposal is lost SHALL be proposed again
rather than abandoned. Losing one is ordinary — the slot it was proposed for may be taken by
another process's command, its proposal may be forwarded to a process that has crashed, or its
decision may never arrive — and without re-proposal the request is dropped in silence.

#### Scenario: An appended value reaches the sequence

- **WHEN** the detector settles and a value is appended at any process
- **THEN** that value eventually appears at the same position in every correct process's sequence

#### Scenario: A request whose slot was taken by another command is proposed again

- **WHEN** a process proposes a command for a slot and a different command is decided for it
- **THEN** the process proposes its command again for a later slot rather than dropping it

#### Scenario: A request for which no decision ever arrives is proposed again

- **WHEN** a process proposes a command for a slot and no decision for that slot arrives
- **THEN** after a bounded wait the process proposes for that slot again, so that neither the
  sequence nor the window is left stalled by one lost decision

#### Scenario: A decision lost on the wire is recovered by asking

- **WHEN** a slot's decision is made but the message announcing it never reaches this process
- **THEN** the process's re-proposal for that slot draws the decision back as an answer, and its
  sequence advances — the consensus beneath is what answers, and this capability's part is only to
  keep asking

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

### Requirement: A position is applied only when every earlier one is known

A process SHALL extend its ordered sequence only in position order: the entry at a position becomes
visible only once decisions for every earlier slot are known, and a sequence SHALL NOT shorten or
change once reported.

This is Invariants R2, R3 and R4 together, and it is why decisions arriving out of order is
ordinary rather than exceptional. The consensus beneath decides slots in whatever order majorities
form; what this capability guarantees is that the *sequence* is extended in order and never rolled
back.

#### Scenario: An out-of-order decision waits

- **WHEN** a decision arrives for a slot later than the next one this process needs
- **THEN** the sequence is not extended until the intervening decisions are known, and is then
  extended through them in order

#### Scenario: The sequence never shortens

- **WHEN** a process is read repeatedly through a run
- **THEN** each result extends the previous one, and no entry already reported changes

### Requirement: A process proposes only within a bounded window ahead of what it has applied

A process SHALL NOT propose for a slot more than a configured window beyond the position it has
applied to.

This is Invariant R5. In the source the window exists because a configuration decided in slot `s`
does not take effect until slot `s + WINDOW`, so proposals beyond it would be made against a
configuration that is not yet certain. This capability does not change the configuration, so the
window's *reason* does not yet apply and the guard SHALL be kept regardless: it bounds how far the
pipeline may run ahead, and it is what makes the stalled-decision case above a bounded failure
rather than an unbounded one. The module SHALL state that the reconfiguration reason is deferred
rather than implying the guard is only a pipeline cap.

#### Scenario: Proposals stop at the window

- **WHEN** decisions stop arriving and requests continue to be appended
- **THEN** the process proposes no further than the window ahead of what it has applied, and holds
  the remaining requests

#### Scenario: The window reopens when the sequence advances

- **WHEN** the missing decisions arrive and the sequence is extended
- **THEN** the held requests are proposed

### Requirement: A request identifier is as durable as the state it keys

A request SHALL carry the identity of the process that appended it and an identifier distinguishing
it from that process's other requests. The scope of that identifier SHALL be stated in the module,
and it SHALL be the incarnation while this capability keeps nothing durable.

The identifier is what makes two appends of the same value two requests rather than one, so the
duplicate-command check depends on it. A counter that restarts at zero after a crash mints an
identifier the run has already used, and a returning process's request can then be mistaken for one
already ordered. This is the same obligation the ballot's generator carries, for the same reason:
an identifier that crosses the wire outlives the handler that minted it.

#### Scenario: The module states the identifier's scope

- **WHEN** a reader consults the module's documentation
- **THEN** it says the request identifier is volatile, that its scope is the incarnation, and what a
  reused identifier would cause

### Requirement: A scope ending is propagated, not absorbed

Where the consensus beneath reports that a scope it depended on has ended, this capability SHALL
raise that to the layer above rather than absorb it.

Nothing here bridges a session ending. What a replica has not yet been told, it cannot invent, and
its redundancy is the other replicas rather than a resend across an ending.

#### Scenario: A session ending reaches the layer above

- **WHEN** the consensus beneath reports a session with a peer has ended
- **THEN** this capability raises it rather than treating the peer as merely slow

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

### Requirement: A process that returns without its state is outside the model

Safety SHALL be claimed for runs in which every process either keeps running or stops for good, as
the consensus beneath it claims. A process that stops and returns having forgotten what it knew
SHALL be documented as outside what this capability covers.

The boundary is inherited rather than new, and this capability adds one thing to it: a returning
replica has forgotten its ordered sequence as well as its ballots, so it would report a shorter
sequence than it had already reported, which is what the ordering requirement above forbids.

#### Scenario: The module states the boundary

- **WHEN** a reader consults the module's documentation
- **THEN** it says a process returning without its state is outside the model, and that a shortened
  sequence is what such a process would produce

#### Scenario: Safety is asserted over runs where crashes are permanent

- **WHEN** the suite injects crashes
- **THEN** the crashed processes are not restarted, and the sequence is asserted over the survivors

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
