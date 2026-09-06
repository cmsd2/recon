## MODIFIED Requirements

### Requirement: The state is unbounded, and this is a transcription

The state SHALL be permitted to grow with the number of slots handled: an acceptor keeps one
proposal per slot it has accepted for, and a leader keeps a proposal for every slot it has been
asked about.

An acceptor SHALL keep, for each slot, only the accepted proposal carrying the **highest ballot**,
and SHALL return only those in answer to a phase-one request. This is the source's §4.1: a leader
"only needs to know if this set is empty or not, and if not, what the maximum pvalue is", so
everything below the maximum is read by nothing and SHALL NOT be kept or sent. Growth with the
number of *ballots* a run has seen is thereby removed; growth with slots is not, so this capability
remains a transcription.

The source's §4.2 is what bounds what remains, and it belongs to a later change because bounding
weakens a guarantee to a scope. What this capability requires is that the module **states** its
bound rather than leaving a reader to assume one.

**Discarding a proposal below the maximum discards evidence, not agreement, and the module SHALL
say so.** After the reduction there may be no majority of acceptors holding the same proposal for a
slot that has nevertheless been chosen, because a later ballot accepted at one of them overwrote its
record — the source raises this itself and calls it a worrisome effect. What carries the choice
forward is that the leader of every later ballot had to read the maximum from a majority, and any
two majorities intersect. The module SHALL state that the record of a choice may be overwritten
while the choice stands, because a reader who assumes otherwise would take a correct run for a
broken one.

#### Scenario: The module states its own space bound

- **WHEN** a reader consults the module's documentation
- **THEN** it says the state is unbounded, that the module is a transcription, and which section of
  the source bounds it

#### Scenario: An acceptor keeps one proposal per slot however many ballots command it

- **WHEN** several ballots each get an acceptor to accept a proposal for the same slot
- **THEN** the acceptor holds one proposal for that slot, carrying the highest of those ballots

#### Scenario: A phase-one answer carries one proposal per slot

- **WHEN** an acceptor answers a phase-one request after accepting proposals for some slots under
  several ballots
- **THEN** its answer carries at most one proposal per slot, and its size grows with the slots it
  has accepted for rather than with the ballots the run has seen

#### Scenario: A proposal below the maximum is not kept

- **WHEN** an acceptor holding a proposal for a slot accepts a proposal for that slot under a
  higher ballot
- **THEN** the earlier proposal is not kept, and a later phase one learns only the higher one

#### Scenario: Agreement survives the record of it being overwritten

- **WHEN** a majority accepts a proposal for a slot, and a later ballot then overwrites that
  proposal at one of them, so that no majority still holds it
- **THEN** no other proposal is ever chosen for that slot
