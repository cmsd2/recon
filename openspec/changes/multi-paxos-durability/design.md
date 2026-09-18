## Context

See `proposal.md` for what §4.3 says and for the finding about lost acknowledged writes. What
follows is how durability lands on the two modules and the two crates beneath them.

What exists and is reused unchanged:

- `Store<Meta, Entry>`: one rewritten value, one appended sequence, a write durable when it returns.
- `Slot` and `SeqSlot`: a durable parent composing a durable child into one record and one sequence.
  `logged_leader_driven_consensus` is the precedent for the metadata half; nothing yet uses the
  sequence half for a child that appends, and this change is its second consumer.
- `on_init` / `on_recovery`, exactly one of which runs; `Sim::crash_on_next_write`; the durability
  guard and its registered list.
- `SynodMsg` and the resend-on-`SessionEstablished` hook, which is where the handshake goes.

What the two modules hold today, and what this change does with each:

| State | Today | After |
|---|---|---|
| acceptor `ballot_num` | volatile | `Meta.promise`, written before `p1b` |
| acceptor `accepted` (one pvalue per slot) | volatile | `Entry::Accepted`, one append per accept; rebuilt on recovery |
| acceptor `collected` | volatile | `Meta.collected`, written when it advances |
| leader round (inside `ballot_num` as leader) | volatile | `Meta.round`, written before the first `p1a` |
| leader `proposals`, `decided`, scout, commanders | volatile | volatile; scope stated as the incarnation |
| `reported` | volatile | volatile; re-learned from periodic reports |
| replica `sequence` | volatile | `Entry::Applied`, one append per applied entry |
| replica `slot_out`, `performed` | volatile | derived from the sequence on recovery |
| replica `cid` counter | volatile, scope incarnation | `(Meta.incarnation, counter)`; incarnation written once per recovery |
| new: `seen: BTreeMap<NodeId, u64>` | — | in memory always; in `Meta` when `Meta` is written anyway |

## Goals / Non-Goals

**Goals:**

- A restart with an honest store is invisible to safety and costs one handshake round to liveness.
- Every write on the hot path is one small append; rewrites happen per leadership change, per
  watermark advance and per recovery, never per entry.
- A lost acknowledged write is detected wherever a witness is reachable, and its detection ends the
  storage scope loudly.
- The write cadence is asserted as an identity, like the message cadence before it.

**Non-Goals:**

- A file-backed store. The simulator's `MemStore` is the store; the file is a driver for
  constraint 5.
- Tolerating a lost acknowledged write. The proposal says why that is not available.
- Snapshots, reconfiguration, and the wait a permanently crashed member imposes on a recovering one.
  Each is named where the module reaches it.

## Decisions

### Accepts are appended, not rewritten

An acceptor's accepted set is one pvalue per slot across the window, so rewriting it as `Meta` on
every `p2b` costs `WINDOW` pvalues per accept. Appending `Entry::Accepted { slot, pvalue }` costs
one record, and recovery folds the records into the per-slot map last-writer-wins, dropping those
below the recovered `collected`. The watermark is what keeps the fold bounded: records below it are
dead, and the sequence's growth is bounded by the window plus what a compaction would reclaim.

*Alternative considered:* rewriting the window, which is the shape `logged_epoch_consensus` uses.
Right there because an epoch consensus holds one value; wrong here because the window is the
state. Also considered: a keyed rewrite (`set_key(slot, pvalue)`) as a new store operation. That is
a third kind of durable state the port does not have, and the sequence already expresses it.

### One record and one sequence, through `Slot` and `SeqSlot`

The replica's `Meta` holds the synod's `Meta` in a `slot!(…, synod)` and the replica's `Entry` is a
sum over `Applied` and the synod's `Accepted`, composed through a `SeqSlot`. The synod's own `Entry`
type is then the accept record and its `Meta` the promise/round/watermark/seen struct; the replica's
`Meta` adds the incarnation. A crash cannot land between the parent's record and the child's,
which is the reason `Slot` exists, and the order between an applied entry and an accept is the
store's order rather than one reconstructed at recovery.

The positions a `SeqSlot` child sees are the parent's and are sparse. The acceptor never counts
positions, only folds records, so this costs nothing here.

### The handshake rides on the Synod wire, on session establishment

`SynodMsg::Hello { writes }` is sent to every member on recovery and on each `SessionEstablished`;
`SynodMsg::HelloAck { seen }` answers it with the highest count the answerer holds for the sender.
The session link is ordered within a session, so a `Hello` sent at establishment arrives before
anything sent after it, and a peer's `seen` is current when it answers.

The count rides on `Hello` alone. A witness learns a peer's count from that peer's `Hello`, and a
peer sends `Hello` whenever a session is established, so the witness's `seen` is at most one
session old. The durable copy of `seen` lags further: it is written when `Meta` is written for
another reason, so a witness that itself restarts answers with the count it had at its last
promise, ballot or watermark write. Both lags are stated in the module and in the spec, and both
are conservative: a stale `seen` is lower, so it under-detects and never falsely accuses.

*Alternative considered:* the count on every message as a header field. It makes `seen` exact at
every witness at the cost of eight bytes per message, and "a layer that adds no per-hop state adds
no wire field" is the convention. The `Hello`-only form is enough for the guarantee the spec makes
and is what a deployment's transport would do at connection time anyway. Reconsider if the lag
turns out to matter in a test.

### The recovered process waits for every member, and the mutation that skips the wait

`recovering: BTreeSet<NodeId>` holds the members that have not yet answered; while non-empty the
acceptor answers no `p1a`/`p2a` and the leader starts no scout. Ω may trust it meanwhile; it simply
does not act on the trust until the set is empty. A `HelloAck` with a count above its own moves the
process to `stopped`, which raises `Ind::StorageScopeEnded { peer: self }` once and answers nothing
thereafter.

`synod-vote-before-answered` is the new feature in `scripts/check-safety-tests.sh`: it empties
`recovering` at recovery. The detection tests must go red under it, and one test must show the
agreement violation it permits, so the wait is evidence rather than caution.

*Alternative considered:* *Paxos Made Live*'s learner rule (wait for a complete instance started
after recovery). The proposal gives the schedule that gets past it. Also considered: waiting for a
majority of answers. Insufficient for the same schedule, since the sole witness may be the one
leader outside the majority.

### The count is the store's, not the protocol's

`Store::writes() -> u64`, on `MemStore`, `NoStore` (zero) and both slot adapters (the parent's
count). Reconstructing it inside each protocol would mean every durable protocol keeps a counter
in `Meta` and increments it on every append, which is a rewrite per append and exactly the cost
the first decision avoids. In the simulator the count is part of the node's storage and survives
`restart`; `restart_empty` zeroes it; `restart_truncated(n)` drops the last `n` acknowledged writes
in order and lowers it by `n`.

### Two faults, distinct from the guard's feature

`Sim::restart_empty(node)` and `Sim::restart_truncated(node, n)` are per-test knobs recorded as
`TraceEvent::Recovered { had_state, lost: Lost::{Nothing, Everything, Last(n)} }`. The
`lose-storage-on-restart` feature stays, because it asks every existing test the question without
editing any, which a knob cannot. The two are different instruments for different questions.

### Recovery order in the replica

`on_recovery`: restore `Meta`; fold the sequence into `sequence`, `slot_out = sequence.len()`,
`slot_in = slot_out`; rebuild `performed` from the last `RETAIN` entries; bump and write
`incarnation`; run the synod child's `on_recovery` through the slot; then, once the child reports
its handshake complete (it raises `Ind::Recovered`), issue `CatchUp { from_slot: slot_out }`. The
catch-up waits for the handshake because a stopped process should not be teaching or learning under
an identity it is about to lose.

## Risks / Trade-offs

- [A member gone for good keeps every later recovery a learner] → stated in the module and tested as
  the cost; reconfiguration lifts it and is the next change. For `f = 1` this means one permanent
  crash plus one restart loses the second vote, which is at the model's limit anyway.
- [`seen` lags at a witness that restarted] → stated; conservative in direction (under-detects,
  never accuses falsely); the exact form (count on every message) is a small follow-up if a test
  needs it.
- [The sequence half of `Slot` has one consumer today] → this is its second, and the fold at
  recovery is the only new operation on it; if the sparse positions bite, the acceptor is the wrong
  place to be counting them and the test will say so.
- [More writes on the hot path than the identity predicts] → the identity test is written before
  the writes are added (measure-then-change, as the message-cost change did), so any extra write
  shows up as a failing count rather than a slow run.
- [The safety guard's existing mutations may gain or lose detectors] → run it after each phase;
  a change in the detector count is a finding, not noise, as the last change showed.
