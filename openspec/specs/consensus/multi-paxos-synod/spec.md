# consensus/multi-paxos-synod Specification

## Purpose

The Synod protocol of *Paxos Made Moderately Complex* §2: what ballots, acceptors, scouts and
commanders guarantee about which proposal can be chosen for a slot, and what they deliberately do
not guarantee about progress. The consensus core Multi-Paxos is built on, and the layer beneath the
replica that turns per-slot agreement into a log.

## Requirements

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
under a lower one. Taking up MAY happen in the same transition as accepting: an acceptor offered a
proposal under a ballot at least as high as its own takes that ballot up and accepts under it, so a
process that missed a ballot's first phase still answers usefully in its second.

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

#### Scenario: An acceptor that missed phase 1 still counts in phase 2

- **WHEN** an acceptor receives a proposal under a ballot at least as high as the one it holds,
  having never received that ballot's phase 1 request
- **THEN** it takes the ballot up, accepts the proposal under it, and its answer counts toward the
  majority

### Requirement: A decision reaches every process, not only the one that counted the majority

Where a majority of acceptors accept a proposal under one ballot, **every** process SHALL learn that
the proposal is chosen for that slot, rather than only the process whose commander collected the
majority.

This is the source's Figure 6(a) as written — a commander's last act before exiting is to send the
decision to every replica. The previous form of this capability omitted it because there was no
replica to address, and stated the omission; there is one now, and a log above this capability
cannot be built from a decision that only one process ever learns.

A process MAY learn a decision for a slot more than once, because a later ballot can command a slot
that has already been decided, and because a leader answers a re-proposal for a decided slot with
the decision again. Invariant A5 makes the command the same one, so the repetition is safe, and the
layer above SHALL be idempotent rather than this capability suppressing it. Every process does keep
what it has learned decided — the answering requirement below needs it — but suppressing a repeat
with that record would make the layer above's recovery depend on this layer's volatile memory, and
the repetition is anyway inherent in a later ballot re-commanding.

#### Scenario: Every process learns a chosen proposal

- **WHEN** a majority of acceptors accept a proposal under one ballot
- **THEN** every correct process that can be reached learns that the proposal is chosen for that
  slot, including processes running no commander for it

#### Scenario: A repeated decision names the same command

- **WHEN** a process learns a decision for a slot it has already learned one for
- **THEN** the command is the same, and the layer above may discard the repetition

### Requirement: A proposal for a decided slot is answered with the decision

Where a process receives a forwarded proposal for a slot it knows to be decided, it SHALL answer
the asker with the decision for that slot, rather than ignoring the request.

This is the leader-side half of the re-proposal fix, and the half without which the other cannot
work. A decision is broadcast once, by the commander that counted the majority; a process the
broadcast never reached learns of the decision by asking, and re-proposal is how it asks. A leader
that silently ignores a proposal for a known slot leaves the asker re-proposing for ever: no new
commander starts, no decision is re-sent, and nothing else in this capability retransmits a
decision once its commander has exited. The source of the fix is the cross-check paper, which states
it directly: a leader "can then work on deciding for that slot if a decision for it has not been
made; otherwise, it can send back the decision for that slot".

The answer goes to the asker alone — one message, not a fan-out — and it SHALL carry the decided
command as the answerer learned it — from its own commander counting a majority, or from another
process's announcement — and NOT the answerer's own proposal for the slot. The two differ for a
leader that commanded something else for the slot, was preempted, and never adopted again: nothing
rewrites its proposal, the announcement still marks the slot decided, and answering from the
proposal would hand the asker a command that was never chosen.

#### Scenario: The answer is the decision, not the answerer's proposal

- **WHEN** a process commanded one command for a slot, was preempted before any acceptor took it,
  and learned by announcement that a different command was decided for that slot
- **THEN** its answer to a forwarded proposal for that slot carries the decided command

#### Scenario: A re-proposal for a decided slot is answered

- **WHEN** a leader that has learned a slot's decision receives a proposal for that slot
- **THEN** it sends the decision for that slot to the asker, and starts no commander

#### Scenario: A proposal for an undecided known slot is not answered with anything

- **WHEN** a leader receives a proposal for a slot it has proposed but not yet decided
- **THEN** it neither answers with a decision nor starts a second commander, and the attempt
  already in flight is what fills the slot

### Requirement: A leader that cannot act on a proposal forwards it to the one that can

Where a process is asked to propose a command for a slot and is not itself in a position to
command it, it SHALL forward the request to the process the eventual leader detector currently
trusts, rather than holding it until it is trusted itself.

This is §4.4's colocation: a replica hands its proposal to the leader on its own machine, and a
passive leader "monitoring another leader λ′ forwards the proposal to λ′". Without it, a proposal
made at a process the detector never trusts is never acted on at all, so a log above this capability
could only make progress at one of its members.

A forwarded request SHALL NOT be forwarded again. Two processes that disagree about who leads would
otherwise pass one back and forth for as long as they disagree, and the request would occupy the
network rather than waiting. Where the second process also cannot act, the request is dropped and
the layer above is responsible for asking again.

A process that is neither active nor trusted SHALL forward even for a slot it remembered a proposal
for while it was trusted, and SHALL forget what it remembered. A proposal remembered for an adoption
that never came is not a commander, so the one-proposal-per-slot guard — which exists for
commanders — has nothing to protect there; holding to it would drop every later request for that
slot from the process's own replica, and nothing else would ever fill it.

#### Scenario: A yielded leader forwards what it once remembered

- **WHEN** a process remembered a proposal for a slot while trusted, was preempted before adopting,
  is no longer trusted, and is asked to propose for that slot again
- **THEN** it forwards the request to the process the detector now trusts

#### Scenario: A proposal made at a passive process reaches the leader

- **WHEN** a process that is not leading is asked to propose a command for a slot, and the detector
  trusts another process
- **THEN** the request reaches that process, which proposes the command under its own ballot

#### Scenario: A forwarded proposal is not forwarded on

- **WHEN** a forwarded request arrives at a process that also cannot act on it
- **THEN** it is not forwarded again

#### Scenario: A process that is trusted keeps its own proposal

- **WHEN** a process the detector trusts is asked to propose a command
- **THEN** it proposes the command itself and forwards nothing

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

**The delivery of a proposal now rests on the detector too, and this SHALL be stated rather than
left as a consequence.** Because a process that cannot act on a proposal forwards it to the one the
detector trusts, a request handed to a process whose detector names a leader that has crashed or
become unreachable is lost. Nothing in this capability recovers it: the layer above SHALL ask again.
Before colocation, a proposal reached every leader, so an inaccurate detector cost nothing for
delivery; it now costs a request, and the suite SHALL contain a run in which the detector is wrong
and a forwarded proposal is lost.

#### Scenario: A settled leader gets its proposals chosen

- **WHEN** the detector settles on one correct process and proposals are made
- **THEN** each proposed slot eventually has a proposal chosen

#### Scenario: Duelling leaders are permitted to make no progress

- **WHEN** two processes each believe themselves leader and keep preempting one another
- **THEN** the run may choose nothing, and this capability is satisfied

#### Scenario: A leader that loses the detector's trust stops competing

- **WHEN** a process that has been leading is no longer trusted by the detector
- **THEN** it stops starting new ballots, so that the process now trusted can make progress

#### Scenario: A proposal forwarded to a process that has crashed is lost

- **WHEN** the detector names a process that has crashed, and a proposal is forwarded to it
- **THEN** no proposal is chosen for that slot on account of that request, and the layer above must
  ask again for it to be acted on

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
