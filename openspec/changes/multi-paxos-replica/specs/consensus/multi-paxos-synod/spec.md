## ADDED Requirements

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
layer above SHALL be idempotent rather than this capability suppressing it: suppression would need
every *receiving* process to remember every decided slot, where the answering requirement below
needs the set only at the leader that decided, and the repetition is anyway inherent in a later
ballot re-commanding.

#### Scenario: Every process learns a chosen proposal

- **WHEN** a majority of acceptors accept a proposal under one ballot
- **THEN** every correct process that can be reached learns that the proposal is chosen for that
  slot, including processes running no commander for it

#### Scenario: A repeated decision names the same command

- **WHEN** a process learns a decision for a slot it has already learned one for
- **THEN** the command is the same, and the layer above may discard the repetition

### Requirement: A proposal for a decided slot is answered with the decision

Where a leader receives a proposal for a slot it knows to be decided, it SHALL answer the asker
with the decision for that slot, rather than ignoring the request.

This is the leader-side half of the re-proposal fix, and the half without which the other cannot
work. A decision is broadcast once, by the commander that counted the majority; a process the
broadcast never reached learns of the decision by asking, and re-proposal is how it asks. A leader
that silently ignores a proposal for a known slot leaves the asker re-proposing for ever: no new
commander starts, no decision is re-sent, and nothing else in this capability retransmits a
decision once its commander has exited. The source of the fix is the cross-check paper, which states
it directly: a leader "can then work on deciding for that slot if a decision for it has not been
made; otherwise, it can send back the decision for that slot".

The answer goes to the asker alone — one message, not a fan-out — and it carries the decided
command, which the leader's own proposal for that slot holds: it was the commanded value in the
ballot that decided, and any later adoption writes the same command back.

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

## MODIFIED Requirements

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
