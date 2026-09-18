## RENAMED Requirements

- FROM: `### Requirement: A process that returns without its state is outside the model`
- TO: `### Requirement: A process that returns with its state is slow, and one that returns without it stops`

## MODIFIED Requirements

### Requirement: A process that returns with its state is slow, and one that returns without it stops

Safety SHALL be claimed for runs in which a process may stop and return **with every write its
storage acknowledged**. Such a process is, in the source's words, "not theoretically considered
crashed — it is simply slow for a while", and the capability SHALL treat it so: it resumes as the
same acceptor and the same leader, under the same identity, with nothing it promised or accepted
forgotten.

A process that returns **without** an acknowledged write is the source's other case — "only a
process that suffers a permanent disk failure would be considered crashed" — and this capability
SHALL NOT tolerate it, because it cannot: an accept that was counted toward a majority and is then
forgotten lets a later phase one read the slot as empty at the one acceptor where two majorities
meet, and no rule local to the returning process recovers what was lost. What the capability SHALL do
is detect the case wherever a witness is reachable, stop the process, and propagate the ending of
its storage scope as an indication. The module SHALL state this boundary, why it is where it is, and
that a replaced server rejoins under a new identity through reconfiguration, which is a later
change.

Swapping an identity underneath a fixed configuration remains unsafe for the reason the previous
form of this requirement gave: majorities of two different acceptor sets need not intersect.

#### Scenario: A process that restarts with its state is the same acceptor

- **WHEN** an acceptor promises a ballot, crashes with every write acknowledged, and restarts
- **THEN** it refuses every ballot below the one it promised, exactly as it would had it not crashed

#### Scenario: A process that restarts with its state is the same leader

- **WHEN** a leader takes up a ballot, crashes with every write acknowledged, restarts, and is
  trusted again
- **THEN** every ballot it takes up afterwards is above every ballot it took up before, and no
  acceptor ever holds two different proposals under one ballot and slot

#### Scenario: The module states the boundary and what crosses it

- **WHEN** a reader consults the module's documentation
- **THEN** it says a process returning with its acknowledged writes is slow rather than crashed,
  that one returning without them is outside the model, why the loss of an acknowledged accept
  cannot be recovered locally, and that reconfiguration is the way back under a new identity

#### Scenario: Safety is asserted over runs where crashes are permanent

- **WHEN** the suite injects crashes and never restarts the crashed processes
- **THEN** safety is asserted over the survivors, as before this capability kept anything durably

#### Scenario: Safety is asserted over runs with crashes and recoveries

- **WHEN** the suite injects crashes and restarts processes with their state
- **THEN** agreement is asserted over the whole run, restarted processes included, and a
  non-vacuity half asserts that a restarted process voted again after recovering

### Requirement: Acceptor state is durable before it is visible

An acceptor SHALL write its promise before sending the `p1b` that reveals it, and SHALL write an
accepted pvalue before sending the `p2b` that reveals it. It SHALL write its collection watermark
before discarding anything below it. A promise, an accept or a collection that a peer can have seen
SHALL therefore be one the acceptor finds again on recovery.

The order is in the handler's own text, not left to a driver: the write returns, and only then is
the effect emitted.

Accepted pvalues SHALL be kept by appending one record per accept rather than by rewriting the
set, so that the cost of an accept does not grow with the width of the window; recovery SHALL
rebuild the per-slot map from the records, latest accept per slot, and SHALL ignore records below
the recovered watermark.

#### Scenario: A promise survives a crash between the write and the send

- **WHEN** an acceptor is armed to die inside its next write, receives a `p1a`, and restarts
- **THEN** either it holds the promise and its peers may hold the `p1b`, or it holds no promise and
  no peer holds the `p1b`; never a `p1b` without a promise

#### Scenario: An accept survives a crash between the write and the send

- **WHEN** an acceptor is armed to die inside its next write, receives a `p2a`, and restarts
- **THEN** either it holds the pvalue and its peers may hold the `p2b`, or it holds no pvalue and
  no peer holds the `p2b`; never a `p2b` without a record

#### Scenario: A recovered acceptor answers phase one with what it accepted

- **WHEN** an acceptor accepts a pvalue for a slot, crashes with every write acknowledged, restarts,
  and receives a `p1a` under a higher ballot
- **THEN** its `p1b` carries that pvalue, and a leader whose majority meets the old majority only at
  this acceptor proposes that pvalue's command

#### Scenario: An accept costs one appended record

- **WHEN** an acceptor accepts pvalues for many slots under one ballot
- **THEN** the store's acknowledged-write count rises by one per accept, however wide the window

### Requirement: A leader's ballot round is durable before it is used

A leader SHALL write the round of a ballot before sending the first `p1a` under it, and on recovery
SHALL take up no ballot whose round is at or below the last one written. A ballot that any acceptor
can have seen SHALL therefore never be minted again by the same leader.

Everything else a leader holds — proposals, scouts, commanders, its record of decided slots — is
scoped to the incarnation and is not written. A restarted leader is passive until trusted; what it
was proposing is re-proposed by the layer above, and what it had learned decided is re-learned
through catch-up.

#### Scenario: A recovered leader mints above everything it used before

- **WHEN** a leader takes up several ballots, crashes with every write acknowledged, restarts, and
  is trusted again
- **THEN** the first ballot it takes up afterwards has a round above every round it used before

#### Scenario: Taking up a ballot costs one rewritten record

- **WHEN** a leader takes up a ballot
- **THEN** the store's acknowledged-write count rises by one, and does not rise again as that
  ballot decides entries

### Requirement: A recovered process announces its storage and does not vote until every member has answered

On recovery, and again whenever a session with a member is established, a process SHALL announce
the number of writes its storage has acknowledged. Every member SHALL keep the highest count it has
seen from each peer — always in memory, and in its own durable record whenever it writes that record
for any other reason — and SHALL answer an announcement with the count it holds for the announcer.

A recovered process SHALL NOT answer a `p1a` or a `p2a`, and SHALL NOT take up a ballot, until every
member of the configuration has answered its announcement. Until then it receives and learns, and
answers catch-up requests from what it holds.

The wait is for every member and not a majority because the only witness to a lost write may be a
single leader that holds this process's promise or accept and has not yet completed a majority with
it. A member that never answers keeps the recovered process a learner; the module SHALL state that a
member gone for good while another recovers costs that recovery its vote until reconfiguration, and
that this is the price of not trusting storage that has not been checked.

#### Scenario: A recovered process with an honest store resumes after one round of answers

- **WHEN** a process restarts with every write acknowledged and every member answers its
  announcement with a count at or below its own
- **THEN** it resumes voting, having answered nothing in between

#### Scenario: A recovered process does not vote before the answers arrive

- **WHEN** a process restarts and a `p2a` reaches it before some member has answered its
  announcement
- **THEN** it does not answer the `p2a`, and answers it once every member has

#### Scenario: A member gone for good keeps a recovered process a learner

- **WHEN** one member has crashed for good and another restarts
- **THEN** the restarted process never votes, the survivors still decide if they are a majority
  without it, and the module documents this as the stated cost

#### Scenario: A member's memory of a peer's count survives its own restart

- **WHEN** a member has seen a count from a peer, then written its own record for any reason, then
  crashed and restarted with its state
- **THEN** the count it answers the peer with is at least the one it had written

### Requirement: A process whose acknowledged writes are missing is detected and stops

When a member answers an announcement with a count higher than the one announced, the announcing
process has lost a write its storage acknowledged. It SHALL stop: it SHALL answer nothing further
under its identity, and SHALL raise an indication that its storage scope has ended. The member that
detected it SHALL raise the same indication naming the peer.

The indication is propagated, not absorbed: nothing in this capability can bridge a storage scope
ending, and a layer above must know that the process it is running on is no longer a member.

Detection is complete only when a witness is reachable before the recovered process votes, which
is what the previous requirement's wait provides. The module SHALL state what the handshake cannot
catch: a witness that has itself lost its memory of the count.

#### Scenario: A truncated store is detected by a witness

- **WHEN** a process accepts a pvalue, its `p2b` is received by a leader, it restarts with that
  write gone from its storage, and it announces its count
- **THEN** the leader answers with the higher count, the process raises the ending of its storage
  scope, and it never votes again under that identity

#### Scenario: An empty store under a known identity is detected

- **WHEN** a process that has promised and accepted restarts with nothing in storage and announces
  a count of zero
- **THEN** every member that has seen it answers with a higher count, and the process stops

#### Scenario: A detected process does not break agreement

- **WHEN** a process restarts with an acknowledged accept gone, in a schedule where a new leader's
  majority meets the old one only at that process
- **THEN** no second proposal is chosen for the slot, because the process never answers the new
  leader's phase one

#### Scenario: Without the wait, the same schedule breaks agreement

- **WHEN** the same schedule is run with the recovered process voting before its announcement is
  answered
- **THEN** two proposals are chosen for one slot, which is what the safety-evidence guard compiles
  this mutation to demonstrate

### Requirement: The cost of durability is an identity

The writes a run makes SHALL be a stated function of what it did — one append per accept, one
rewrite per promise, one per ballot taken up, one per watermark advance, one per recovery — and that
function SHALL be asserted rather than described, at two membership sizes.

#### Scenario: A settled run writes exactly what the algorithm needs

- **WHEN** leadership settles and a known number of entries is decided over a run that loses nothing
- **THEN** each process's acknowledged-write count equals the stated function of its role, the
  entries, and the leadership changes

#### Scenario: An idle run writes nothing

- **WHEN** a run has decided everything proposed and nothing further is proposed
- **THEN** no process's acknowledged-write count rises during the idle window
