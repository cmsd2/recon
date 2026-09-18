## Why

Both halves of Multi-Paxos are crash-stop, and their own documentation says what that costs: a
process that restarts re-mints a ballot it has already used, an acceptor still holding that ballot
accepts a second proposal under it, and Invariant A4 — the clause the two-majorities argument rests
on — no longer holds. `Sim::crash` then `Sim::restart` produces exactly that process, and Ω will
trust it again. So today a restart is a safety hazard, not a liveness gap, and the specification
draws a boundary around it: "a process that returns without its state is outside the model".

§4.3 of the source, *Keeping State on Disk*, is what moves the boundary:

> The Paxos protocol can tolerate a minority of its acceptors failing, and all but one of its
> replicas failing. If more than that fail, consistency is still guaranteed but liveness will be
> violated. For this reason, one may want to keep the state of acceptors and replicas on disk. A
> process that suffers from a power failure but can recover from disk is not theoretically
> considered crashed — it is simply slow for a while. Only a process that suffers a permanent disk
> failure would be considered crashed.

This is the last obligation between Multi-Paxos and a deployment: the session-link obligation and
both halves of the resource-use obligation are met, and `README.md` names durability as what is
still lacking. Every mechanism it needs already exists in `recon-core` and the simulator — `Slot`
and `SeqSlot` for a durable parent composing a durable child, `crash_on_next_write` for dying inside
a write, and the durability guard that requires a restart test to go red when storage is wiped.

## What Changes

- **The acceptor's promise, its accepted pvalues and its collection watermark become durable**, each
  written before the message that reveals it is sent: the promise before `p1b`, the accept before
  `p2b`. The accepted pvalues are *appended*, one record per accept, rather than rewritten as a
  window — a rewrite would cost `WINDOW×` per entry on a real file — and the promise, the round and
  the watermark are the one small rewritten value. Replay rebuilds the per-slot map last-writer-wins
  and drops what is below the watermark.

- **The leader's ballot round becomes durable**, written before a scout is started under it. This is
  the durable ballot counter the module already names as what makes leading again after a restart
  legal. Proposals, scouts, commanders and the decided map stay volatile and say so: a restarted
  leader is not trusted until Ω says so, and what it held is re-proposed by the replica or re-learned
  through the existing catch-up.

- **The replica's ordered sequence becomes durable**, each entry appended before `Ordered` is
  indicated, and it composes the Synod core's record through a `Slot` and a `SeqSlot` so that a
  restart finds one record and one sequence with a real order between the parent's entries and the
  child's. On recovery the replica derives `slot_out` from the sequence, rebuilds its duplicate
  filter from the retained tail, and asks a peer for what was decided while it was down.

- **The request identifier gains a durable incarnation.** A replica's `cid` becomes
  `(incarnation, counter)` where the incarnation is written once per recovery, so the append path
  carries no write for the identifier and a restarted replica cannot mint an identifier its peers'
  duplicate filters have already seen.

- **A storage scope, and the handshake that checks it.** Durability is guaranteed per server id, and
  only for writes the store acknowledged. A store reports how many writes it has acknowledged; a
  recovered process announces that count to every member on session establishment; every member
  keeps the highest count it has seen from each peer, in memory always and in its own durable
  record whenever it writes anyway. **A recovered process does not vote until every member has
  answered.** A member holding a higher count than the one announced has witnessed a write the
  recovered process no longer has: the process is told, stops, raises an indication that its storage
  scope has ended, and does not act again under that id. This is the storage analogue of
  `SessionEnded`: **propagated, not absorbed**.

- **What the handshake cannot do is stated, because the analysis found it.** A lost *acknowledged*
  write is not something the protocol can tolerate, only detect. An acceptor whose `p2b` was counted
  toward a majority and whose record of that accept is gone can, in a later phase one, report the slot
  empty to a leader whose majority meets the old one only at that acceptor; and the "learner until a
  fresh instance completes" rule from *Paxos Made Live* does not close it, because the hazard is a
  promise or an accept held by a leader that has not yet completed a majority, and no instance that
  completes without the recovered process can reach that leader. So the survey's last sentence
  stands as the model: a permanent disk failure is a crash, and a process that returns without its
  acknowledged writes is that process. Detection needs a witness reachable before the recovered
  process votes, which is why the wait is for every member and not a majority, and the cost of that
  wait is stated: a member that has crashed for good while another is recovering keeps the
  recovering one a learner until reconfiguration, which is a later change.

- **Two simulator faults, first-class.** `restart_empty` returns a process with nothing in storage;
  `restart_truncated` returns it with the last `n` acknowledged writes gone and the store claiming
  to be complete. Each is recorded in the trace, and each is checked against the invariant every
  simulator capability is held to: the run cannot lose the writes without a process raising the
  event that says so — through the handshake, when a witness is reachable.

- **Write cost as an identity**, in the spirit of the message-cost change: one append per accept,
  one per applied entry, one rewrite per promise, one per ballot taken up, one per watermark
  advance, one per recovery; asserted at two membership sizes. Storage is the resource §4.3 spends,
  and an unmeasured write cadence is how the 3.6× went unnoticed on the wire.

- **The status of both modules stays *implementation*** and their space bound gains "on disk": the
  acceptor's record is bounded by the window as before, and the sequence is the data, now durable,
  with the snapshot that would bound it named as a later change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `consensus/multi-paxos-synod`: the boundary requirement ("a process that returns without its
  state is outside the model") is replaced by what a returning process may and may not do; new
  requirements for durable-before-visible acceptor and leader state, the storage-scope handshake and
  the wait before voting, detection of lost acknowledged writes, and the write-cost identity.
- `consensus/multi-paxos-replica`: the boundary requirement is replaced by the sequence surviving a
  restart; the request-identifier requirement changes scope from incarnation-volatile to
  incarnation-durable; a new requirement that recovery catches up on what was decided meanwhile.
- `simulation`: storage reports its acknowledged-write count; two new restart faults, recorded in
  the trace; the existing storage requirement gains the count's survival.
- `protocol-core`: the storage interface reports the number of writes it has acknowledged, and a
  child composed through a slot sees its parent's count.

## Impact

- `crates/recon-core/src/store.rs` — `Store::writes()`, on `MemStore`, `NoStore` and the slot
  adapters.
- `crates/recon-sim/src/sim.rs` — `restart_empty`, `restart_truncated`, the trace events for
  them, and the count surviving an honest restart.
- `crates/recon-protocols/src/multi_paxos_synod.rs` — `Meta` and `Entry` types, the writes at the
  four points above, `on_recovery`, the `Hello`/`HelloAck` exchange on `SessionEstablished`, the
  `seen` map, the learner state and `Ind::StorageScopeEnded`.
- `crates/recon-protocols/src/multi_paxos_replica.rs` — the composed record, the appended sequence,
  the incarnation, `on_recovery` and the catch-up it issues.
- Both suites; `tests/total_order_log.rs`, whose restart properties the replica can now be held to
  alongside `logged_uniform_total_order_broadcast`.
- `scripts/check-durability-tests.sh` — every new restart test registered.
- `scripts/check-safety-tests.sh` — one new mutation: a recovered acceptor that votes before every
  member has answered. The detection tests must go red under it.
- `docs/conditional-guarantees.md` — the storage scope beside the session scope: what it bounds,
  what bridges it (nothing local), and what propagates its ending.
- `docs/bounded-space.md`, `README.md`, `CLAUDE.md` — status, space and the real-world set's
  "what it still lacks" sentence, in the same commit.

**Not** in scope: the file-backed store (a driver, constraint 5, alongside transport); snapshots of
the sequence; reconfiguration, which is what lets a replaced server rejoin under a new id and what
lifts the wait on a member that is gone for good. Each is named where the module reaches its edge.
