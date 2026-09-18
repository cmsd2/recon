## ADDED Requirements

### Requirement: Leaders and acceptors collect what enough replicas have already applied

Where a process learns that at least `f + 1` of the `2 f + 1` members have applied every decision up
to some slot, it SHALL discard the state it holds for slots below that one: the proposals it holds
as a leader, the pvalues it holds as an acceptor, and its record of which slots are decided.

This is the source's §4.2. The state is unnecessary rather than unavoidable, and the reason is that
discarding it moves the information rather than destroying it: "replicas can learn decisions, and
the application state that results from those decisions, from one another". A slot whose decision
`f + 1` replicas hold is one the consensus layer no longer has to be able to reconstruct.

Members SHALL report how far they have applied on their own schedule, rather than only when they
answer something. A process that is currently answering nothing is exactly the one whose progress
the others most need to hear about, and a report carried on an existing reply would stop when the
work did.

#### Scenario: State below the watermark is discarded

- **WHEN** at least `f + 1` members have applied every decision up to a slot, and have said so
- **THEN** every process discards the proposals, accepted proposals and decision records it holds
  for lower slots

#### Scenario: State is not discarded before enough members have applied it

- **WHEN** fewer than `f + 1` members have reported applying up to a slot
- **THEN** nothing below that slot is discarded

#### Scenario: Collection stalls, correctly, when too few members remain

- **WHEN** `f` of `2 f + 1` members have crashed, leaving fewer than `f + 1` to report
- **THEN** no collection happens, and this is the specified behaviour rather than a failure

### Requirement: An acceptor says how far it has collected, and a leader skips below it

An acceptor SHALL keep the slot below which it has discarded its accepted proposals, and SHALL
report it in its answer to a phase-one request. A leader taking up a ballot SHALL NOT propose for
any slot below the highest such value reported by the acceptors that answered it; it SHALL skip
those slots.

**This is the clause the safety of the collection rests on.** Before collection, an acceptor
reporting nothing for a slot meant nothing had been accepted for it. Afterwards it means one of two
things, and the reported slot is the only thing that tells them apart. The source is explicit that
this is the hazard: "we must prevent other leaders from mistakenly concluding that the acceptors
have not accepted any pvalues for the garbage-collected slots".

A leader that read a collected slot as free would propose a command for a slot already decided, and
could get it chosen — two commands chosen for one slot, which is what this capability's first
requirement forbids.

The **highest** value reported, not the lowest. A slot collected at any acceptor of the answering
majority is a slot whose decision `f + 1` members hold, so it is settled whoever else still holds a
pvalue for it; taking the lowest would be safe and would waste most of the collection.

#### Scenario: A phase-one answer says how far the acceptor has collected

- **WHEN** an acceptor that has discarded state below a slot answers a phase-one request
- **THEN** its answer carries that slot

#### Scenario: A leader does not propose for a collected slot

- **WHEN** a leader takes up a ballot and an acceptor in its majority reports having collected below
  some slot
- **THEN** the leader proposes for no slot below that one, and starts no commander for one

#### Scenario: Agreement holds across a collection

- **WHEN** a proposal is chosen for a slot, every process's state for that slot is later collected,
  and a new leader then takes up a higher ballot
- **THEN** no different proposal is ever chosen for that slot

## MODIFIED Requirements

### Requirement: The state is bounded, and this is an implementation

The state SHALL NOT grow with the number of slots handled. An acceptor keeps one proposal per slot
between the collection watermark and the frontier; a leader keeps one proposal and one decision
record per slot in the same range; and the report of how far each member has applied is one entry
per member.

Every one of those is bounded by the membership and by how far the frontier is allowed to run ahead
of what has been applied, which the layer above caps. None is bounded by how many slots the run has
handled, which is what `docs/bounded-space.md` requires of an implementation and what the source's §2
does not do — §4 opens by saying "the described protocol is not practical", and §4.1 and §4.2 are its
two reductions. Both are applied.

The module SHALL state the bound, and SHALL carry a test that its state does not grow with the slots
handled: run a growing number of slots and require the bound to hold. It SHALL also state what the
bound is conditional on — collection needs `f + 1` members reporting, so a run with too few stalls
and grows, which is the source's own caveat rather than a defect.

#### Scenario: The module states its own space bound

- **WHEN** a reader consults the module's documentation
- **THEN** it says the state is bounded, by what, and what the bound is conditional on

#### Scenario: State does not grow with the slots handled

- **WHEN** a run decides many more slots than another, and enough members report their progress
- **THEN** the state held by each process is bounded by the same figure in both, rather than growing
  with the slots decided

#### Scenario: The bound is asserted over a run that actually collected

- **WHEN** a test asserts the bound
- **THEN** it also asserts that collection happened and the watermark advanced, so that the bound is
  not satisfied by a run in which nothing was ever collected
