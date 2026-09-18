## RENAMED Requirements

- FROM: `### Requirement: A process that returns without its state is outside the model`
- TO: `### Requirement: The ordered sequence survives a restart`

## MODIFIED Requirements

### Requirement: The ordered sequence survives a restart

A replica SHALL write an entry to its durable sequence before indicating that the entry is ordered,
and on recovery SHALL hold every entry it had indicated. The sequence a recovered replica reports
SHALL extend the sequence it reported before the crash, never shorten it.

The replica's record and the consensus core's SHALL be one record and one sequence: the core's
durable state is a named part of the replica's, so a crash cannot land between the two, and the
order between a replica's appended entries and the core's is the order the store gives them rather
than one invented at recovery.

On recovery a replica SHALL derive how far it has applied from the sequence, rebuild its duplicate
filter from the retained tail, and ask a peer for the decisions it missed while it was down, through
the catch-up the consensus core already carries. A replica whose gap exceeds what any peer retains
is the snapshot case, which is outside this capability and SHALL be stated as such.

The boundary this requirement replaced — a process returning without its state — is now the
consensus core's to detect and propagate, and a replica SHALL pass the ending of its storage scope
up to the layer above rather than absorb it.

#### Scenario: A recovered replica's read extends what it served before

- **WHEN** a replica orders entries, crashes with every write acknowledged, restarts, and is read
- **THEN** the sequence it reports has the same prefix it reported before, and is at least as long

#### Scenario: A recovered replica appends something new

- **WHEN** a replica recovers, and the layer above appends a new value
- **THEN** that value takes a position in the sequence at every process, including the recovered one

#### Scenario: A recovered replica catches up on what it missed

- **WHEN** entries are ordered while a replica is down, and it then restarts with its state
- **THEN** it applies those entries in order and reports the same sequence as a process that never
  failed

#### Scenario: Dying inside the sequence write is consistent

- **WHEN** a replica is armed to die inside its next write, applies an entry, and restarts
- **THEN** either the entry is in its sequence and was never indicated, or it is absent and is
  applied again on catch-up; it is never indicated without being written

#### Scenario: A storage scope ending is propagated

- **WHEN** the consensus core beneath a replica raises the ending of its storage scope
- **THEN** the replica raises it to the layer above and orders nothing further

#### Scenario: The module states the boundary

- **WHEN** a reader consults the module's documentation
- **THEN** it says a replica returning with its acknowledged writes resumes the same sequence, that
  one returning without them is detected by the core beneath and stops, and that a gap larger than
  any peer retains is the snapshot case outside this capability

#### Scenario: Safety is asserted over runs where crashes are permanent

- **WHEN** the suite injects crashes and never restarts the crashed processes
- **THEN** the sequence is asserted over the survivors, as before this capability kept anything
  durably

### Requirement: A request identifier is as durable as the state it keys

A request SHALL carry the identity of the process that appended it and an identifier distinguishing
it from every other request that process has ever appended, across restarts. The identifier SHALL
be scoped by a durable incarnation, written once at each recovery, so that the append path carries
no write for the identifier and a recovered process cannot mint an identifier a peer's duplicate
filter has already seen.

The identifier is what makes two appends of the same value two requests rather than one, so the
duplicate-command check depends on it. A counter that restarted at zero after a crash would mint an
identifier the run has already used, and a returning process's genuinely new request would be
dropped as a duplicate. That is the audit's recurring bug — a durable filter keyed by a volatile
counter — and this is the obligation that closes it here.

#### Scenario: A recovered replica's new append is not a duplicate

- **WHEN** a replica appends a value, crashes with its state, restarts, and appends again within the
  retention window
- **THEN** both appends take positions in the sequence

#### Scenario: The module states the identifier's scope

- **WHEN** a reader consults the module's documentation
- **THEN** it says the request identifier is scoped by a durable incarnation, that recovery costs one
  write for it and an append costs none, and what a reused identifier would cause

### Requirement: The state is bounded, and this is an implementation

The bookkeeping a replica keeps SHALL NOT grow with the number of commands handled: the decisions it
holds and the record of what it has applied are bounded by the retention window, and the requests
and proposals outstanding are bounded by the window ahead of what it has applied.

The **ordered sequence itself is exempt and SHALL be stated as such**, and it is now on disk. A log
grows with what is appended to it; that is the data rather than the bookkeeping, and a log that
discarded it would not be a log. The module SHALL say so, SHALL say that the durable sequence
grows with the run, and SHALL name the snapshot that would bound it as a later change rather than
leave a reader to think the exemption was an oversight.

#### Scenario: The bookkeeping does not grow with commands handled

- **WHEN** a run appends many more commands than another, with the retention window unchanged
- **THEN** the decisions and applied-record a replica holds are bounded by the same figure in both

#### Scenario: The module states which part is exempt and why

- **WHEN** a reader consults the module's documentation
- **THEN** it says the ordered sequence is not bounded, that it is the data rather than the
  bookkeeping, that it is durable, and what would bound it

## ADDED Requirements

### Requirement: Applying an entry costs one appended record

The writes a replica makes SHALL be a stated function of what it did — one append per applied
entry and one rewrite per recovery — and that function SHALL be asserted rather than described.

#### Scenario: A settled run writes one record per entry applied

- **WHEN** a run orders a known number of entries and loses nothing
- **THEN** each replica's own acknowledged-write count, net of the consensus core's, rises by
  exactly that number
