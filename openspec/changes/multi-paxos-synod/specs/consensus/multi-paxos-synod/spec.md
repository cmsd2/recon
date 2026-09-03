## Purpose

The Synod protocol of *Paxos Made Moderately Complex* §2: what ballots, acceptors, scouts and
commanders guarantee about which proposal can be chosen for a slot, and what they deliberately do
not guarantee about progress. The consensus core Multi-Paxos is built on, before it has slots, a
replica or a log.

## ADDED Requirements

### Requirement: At most one proposal is ever chosen for a slot

Where two processes each learn that a proposal has been chosen for the same slot, those proposals
SHALL be equal. This SHALL hold whatever the schedule, whatever the ballots in flight, and whatever
processes have crashed — it is not conditional on a leader settling, on the network behaving, or on
the failure detector being right.

This is the whole of what the Synod protocol claims, and everything else about the algorithm exists
to keep it. A proposal is chosen when a majority of acceptors have accepted it under one ballot;
what makes two such majorities agree is that they intersect, and that a process taking up a higher
ballot first learns what the previous majority accepted and adopts it as its own proposal.

#### Scenario: Two leaders competing for a slot cannot split it

- **WHEN** two processes each run a ballot for the same slot with different proposals, and both
  ballots reach acceptors
- **THEN** at most one proposal is chosen, and every process that learns of a choice for that slot
  learns the same one

#### Scenario: A later ballot adopts what an earlier majority accepted

- **WHEN** a proposal has been accepted by a majority of acceptors under some ballot, and a process
  subsequently succeeds in taking up a higher ballot
- **THEN** the proposal it makes for that slot is the one already accepted, rather than the one it
  set out to propose

#### Scenario: Safety survives a minority crashing

- **WHEN** a run chooses proposals while fewer than half the acceptors crash
- **THEN** no two processes learn different proposals for any slot

### Requirement: A chosen proposal is one that some process proposed

Where a process learns that a proposal has been chosen for a slot, some process SHALL have proposed
it. The protocol SHALL NOT invent a proposal, and SHALL NOT choose one for a slot nobody proposed
for.

#### Scenario: Nothing is chosen that was not proposed

- **WHEN** a process learns of a choice for a slot
- **THEN** the proposal it learns was proposed by some process in the run

#### Scenario: A slot nobody proposed for stays empty

- **WHEN** a run proposes for some slots and not others
- **THEN** no choice is ever learned for a slot nobody proposed for

### Requirement: An acceptor's promise is monotonic

An acceptor SHALL take up strictly increasing ballots, and SHALL accept a proposal only under the
ballot it has currently taken up. Having taken up a ballot, it SHALL NOT afterwards accept anything
under a lower one.

This is what makes the intersection argument work: a majority that has taken up ballot `b` is a
majority that can no longer accept anything below `b`, so a proposal chosen below `b` must already
be visible to whoever holds `b`.

#### Scenario: A stale ballot is refused

- **WHEN** an acceptor that has taken up a ballot receives a request under a lower one
- **THEN** it does not accept it, and reports the ballot it has taken up instead

#### Scenario: The refusal names the ballot that beat it

- **WHEN** a process is refused because a higher ballot exists
- **THEN** it is told which ballot, so that it can take up a higher one rather than retrying the
  same one

### Requirement: Progress is conditional on a leader settling, and the module says so

Progress SHALL be claimed only for runs in which the eventual leader detector settles on one correct
process. Where two processes both believe themselves leader, the protocol MAY preempt each ballot
with the next indefinitely and choose nothing, and this SHALL be documented in the module as the
source documents it rather than presented as a defect.

The source is explicit that its Synod protocol "does not guarantee this, even in the absence of any
failure whatsoever", and adds a failure detector in a later section to obtain progress. This
capability composes the eventual leader detector already in this repository instead, which is a
departure and SHALL be stated as one. The condition inherits everything the detector's own condition
rests on, stated at each link rather than collapsed into one claim.

#### Scenario: A settled leader gets its proposals chosen

- **WHEN** the detector settles on one correct process and proposals are made
- **THEN** each proposed slot eventually has a proposal chosen

#### Scenario: Duelling leaders are permitted to make no progress

- **WHEN** two processes each believe themselves leader and keep preempting one another
- **THEN** the run may choose nothing, and this capability is satisfied

#### Scenario: A leader that loses the detector's trust stops competing

- **WHEN** a process that has been leading is no longer trusted by the detector
- **THEN** it stops starting new ballots, so that the process now trusted can make progress

### Requirement: A process that returns without its state is outside the model

Safety SHALL be claimed for runs in which every process either keeps running or stops for good. A
process that stops and returns **having forgotten what it knew** SHALL be documented as outside what
this capability covers, and the module SHALL say what such a process would break and what a later
capability buys by fixing it.

The source admits two behaviours and no third. A crash is permanent — a crashed state machine "will
make no more transitions and thus its current state is fixed indefinitely" — and a process that
comes back off disk "is not theoretically considered crashed—it is simply slow for a while. Only a
process that suffers a permanent disk failure would be considered crashed." A process that returns
with nothing is the first case making transitions it is not allowed to make.

There is a third way out that this capability does not take, and the source does specify it: a
returning process may rejoin as a **new** acceptor through a reconfiguration. That is safe because
the configuration change is itself decided, so majorities are counted against a configuration both
sides agree on. Swapping an identity underneath a fixed configuration is not the same thing and is
not safe — majorities of two different acceptor sets need not intersect. Reconfiguration is a later
change; until it exists, the boundary above is where this capability stops.

It matters here because the simulator can produce that case and the eventual leader detector will
trust such a process again. What breaks is ballot identity: the round counter restarts, the process
re-mints a ballot it has already used, and an acceptor still holding that ballot accepts a second,
different proposal under it — so two proposals can be accepted at one ballot and slot, and the
argument that makes two majorities agree no longer holds. This is the same shape as the durable
identity rule elsewhere in this repository: an identifier that crosses the wire outlives the handler
that minted it, so its generator is state whose scope must be stated.

#### Scenario: The module states the boundary and what crosses it

- **WHEN** a reader consults the module's documentation
- **THEN** it says that a process returning without its state is outside the model, why a reused
  ballot breaks the safety argument, and that a durable ballot counter is what makes leading again
  after a restart legal

#### Scenario: Safety is asserted over runs where crashes are permanent

- **WHEN** the suite injects crashes
- **THEN** the crashed processes are not restarted, and safety is asserted over the survivors

### Requirement: A scope ending is propagated, not absorbed

Where the link beneath reports that a scope it depended on has ended, this capability SHALL raise
that to the layer above rather than absorb it.

Nothing here bridges a session ending. What a process learns of a ballot lives in memory that a
crash takes, and the redundancy this protocol has is the other processes rather than a resend across
an ending. A layer above may need to know that an answer it was waiting for will not arrive.

#### Scenario: A session ending reaches the layer above

- **WHEN** the link reports a session with a peer has ended
- **THEN** this capability raises it, rather than treating the peer as merely slow

### Requirement: The membership is fixed for the run

The set of acceptors SHALL be fixed when the run begins, and this capability SHALL NOT add or remove
one.

Majorities are counted over that set, and the argument that two majorities intersect is what makes
one proposal per slot hold. Changing the set without deciding the change is what breaks it. The
source specifies how to change it safely — a reconfiguration command decided in a slot, taking
effect a window of slots later — and that needs slots, which this capability does not have.

#### Scenario: The acceptor set does not change

- **WHEN** a run proceeds, with or without crashes
- **THEN** the set of acceptors majorities are counted over is the one the run began with

### Requirement: The state is unbounded, and this is a transcription

The state SHALL be permitted to grow with the number of slots handled: an acceptor keeps every
proposal it has accepted, and a leader keeps a proposal for every slot it has been asked about.

This is the source's §2, which is explicitly the impractical version — the paper's own §4 opens by
saying "the described protocol is not practical" and gives the reductions that bound it. Those
belong to a later change, because bounding weakens a guarantee to a scope. What this capability
requires is that the module **states** its bound rather than leaving a reader to assume one.

#### Scenario: The module states its own space bound

- **WHEN** a reader consults the module's documentation
- **THEN** it says the state is unbounded, that the module is a transcription, and which section of
  the source bounds it
