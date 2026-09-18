## 1. The source, quoted, and the boundary restated

- [x] 1.1 Quote §4.3 in `multi_paxos_synod.rs` above the durable state, from the 2015 survey
      (page 42:18), and verify the section title and page against the PDF before quoting
- [x] 1.2 Replace the module's "boundary this module does not cross" section with the new boundary:
      a process returning with its acknowledged writes is slow; one returning without them is
      detected and stops; reconfiguration is the way back. Verify `cargo doc -D warnings` passes and
      the section names the schedule that defeats the *Paxos Made Live* rule
- [x] 1.3 Do the same in `multi_paxos_replica.rs`, and state that the sequence is durable, grows,
      and is bounded only by a snapshot named as a later change. Verify the status/space line reads
      "implementation; bounded by the window, on disk; the sequence exempt"

## 2. The store's count

- [x] 2.1 Add `Store::writes(&self) -> u64` to `recon-core`, implemented on `MemStore`
      (incremented per `set` and per `append`), `NoStore` (zero), and both slot adapters
      (delegating to the parent). Verify with a contract test in `core_contract.rs` for each of the
      three scenarios in the protocol-core delta
- [x] 2.2 Make the count part of the simulator's per-node storage so it survives `Sim::restart`.
      Verify with a `simulation.rs` test: write `k` times, crash, restart, read `k`
- [x] 2.3 Verify `crash_on_next_write` counts the interrupted write iff it took effect, across
      seeds, in `simulation.rs`

## 3. Two restart faults

- [x] 3.1 Add `Sim::restart_empty(node)` and `Sim::restart_truncated(node, n)`, and extend
      `TraceEvent::Recovered` with what was lost. Verify each of the four scenarios in the
      simulation delta with a `simulation.rs` test, including that a truncated store reads as a
      valid earlier state
- [x] 3.2 Verify `./scripts/check-durability-tests.sh` still passes unchanged: the feature and the
      knobs coexist and the feature's list is untouched by this step
- [x] 3.3 Add the storage scope to `docs/conditional-guarantees.md` beside the session scope: what
      it bounds, that nothing local bridges it, what propagates its ending, and what the simulator
      can and cannot express about it. Verify the table of scopes gains a row

## 4. Measure before writing anything

- [x] 4.1 Write the write-cost identity test for the synod suite — per-role writes as a function of
      entries and leadership changes, at three and five members — against the current code, and
      verify it fails with every count at zero. This is the test the rest of the change is built to
      turn green
- [x] 4.2 Write the replica's identity test the same way (one append per applied entry, one rewrite
      per recovery, net of the core's), and verify it fails the same way

## 5. The acceptor's durable state

- [x] 5.1 Declare `Meta { promise, round, collected, seen }` and `Entry::Accepted { slot, ballot,
      command }` on `MultiPaxosSynod`. Verify the crate builds with the replica still passing
      `NoStore` (it will not compile once the replica composes durably; that is step 7)
- [x] 5.2 Write the promise before `p1b`, in the handler's own text. Verify with the
      dying-inside-the-write test for `p1a` across seeds: never a `p1b` without a promise
- [x] 5.3 Append the accept before `p2b`. Verify with the same test for `p2a`, and with the
      one-append-per-accept count from 4.1
- [x] 5.4 Write `collected` when it advances, before discarding below it. Verify a recovered
      acceptor's `p1b` carries the recovered watermark and skips below it (the existing skip test,
      run through a restart)
- [x] 5.5 `on_recovery`: restore `Meta`, fold the accept records last-writer-wins, drop below
      `collected`. Verify with "a recovered acceptor answers phase one with what it accepted", in
      the schedule where the restarted acceptor is the only intersection of the two majorities, and
      verify that test goes red under `lose-storage-on-restart`

## 6. The leader's durable round

- [x] 6.1 Write `round` before the first `p1a` of a scout. Verify with "a recovered leader mints
      above everything it used before" and with the no-two-proposals-at-one-ballot-and-slot
      assertion the checker already runs, through a leader restart that Ω re-trusts
- [x] 6.2 State the incarnation scope of proposals, scouts, commanders and `decided` in the module.
      Verify a restarted leader that had outstanding commanders decides nothing under its old
      ballot and the replica re-proposes (the existing "a command that loses its slot and comes
      back" test, with a restart rather than a preemption)

## 7. The replica's durable sequence, composed

- [x] 7.1 Declare the replica's `Meta { incarnation, synod: <synod Meta> }` and
      `Entry = Applied | Synod(<synod Entry>)`, compose the core through `slot!` and a `SeqSlot`,
      and switch `run_durable` on. Verify `core_contract.rs`'s child-sees-parent's-count scenario
      against this pair
- [x] 7.2 Append `Applied` before indicating `Ordered`. Verify with the dying-inside-the-write test
      for `perform`: never indicated without being written
- [x] 7.3 `on_recovery`: restore, fold, derive `slot_out` and `slot_in`, rebuild `performed` from
      the retained tail, bump and write `incarnation`. Verify "a recovered replica's read extends
      what it served before" and "appends something new", each red under `lose-storage-on-restart`
- [x] 7.4 Issue `CatchUp` after the core reports its handshake complete. Verify "a recovered replica
      catches up on what it missed" against a process that never failed
- [x] 7.5 Change `cid` to `(incarnation, counter)`. Verify "a recovered replica's new append is not a
      duplicate", and verify it is red when the incarnation is held at zero (a one-line local
      mutation, not a registered one)

## 8. The handshake, the wait, and the detection

- [x] 8.1 Add `SynodMsg::Hello { writes }` and `HelloAck { seen }`; send `Hello` on recovery and on
      `SessionEstablished`; keep `seen` in memory from `Hello`s and in `Meta` when `Meta` is written.
      Verify `seen` survives a witness's own restart at least as far as its last write
- [x] 8.2 Add the `recovering` set: no `p1a`/`p2a` answered and no scout started while non-empty;
      `Ind::Recovered` raised when it empties. Verify "does not vote before the answers arrive" by
      hand-driving a `p2a` between the restart and the last `HelloAck`
- [x] 8.3 On a `HelloAck` above its own count, stop: raise `Ind::StorageScopeEnded` once, answer
      nothing after. Verify the truncated-store and empty-store detection scenarios, each with the
      witness partitioned until after the restart so the backlog cannot mask it
- [x] 8.4 Verify "a member gone for good keeps a recovered process a learner": crash one for good,
      restart another, assert it never votes and the survivors still decide at five members
- [x] 8.5 Add `synod-vote-before-answered` to `Cargo.toml` and `scripts/check-safety-tests.sh`, and
      register the detection tests under it. Verify the guard passes, and verify one registered test
      shows two proposals chosen for one slot under the mutation
- [x] 8.6 Propagate `StorageScopeEnded` through the replica and verify the replica orders nothing
      after it

## 9. Cost, and the guards

- [x] 9.1 Turn 4.1 and 4.2 green and verify the identity holds exactly at three and five members,
      and that an idle window writes nothing
- [x] 9.2 Register every restart test from steps 5–8 in `scripts/check-durability-tests.sh` and
      verify the guard passes: each goes red with storage wiped, and the guard names no unregistered
      detector
- [x] 9.3 Run `./scripts/check-safety-tests.sh` and verify the existing three mutations' detector
      counts; treat any change as a finding to explain in the commit
- [x] 9.4 Verify `assert_send_rate_flat!` still holds for both modules with the handshake in place,
      and that the message-cost identity is unchanged (the `Hello` pair is per establishment, not
      per entry, and the identity counts by kind)
- [x] 9.5 Run the shared `total_order_log.rs` suite's restart properties against the replica, which
      it could not be held to before. Verify they pass, and register the durable ones

## 10. Documents, in the same commit

- [x] 10.1 `docs/bounded-space.md`: both Multi-Paxos rows gain "on disk" and the sequence's
      exemption says it is durable. Verify the rows match the modules' status lines
- [x] 10.2 `README.md`: the fail-recovery table rows for both modules, the real-world set section
      (durability met; what remains is snapshots and reconfiguration), the guard section for the
      fourth mutation, the test-count table and total. Verify the counts against `cargo test`
- [x] 10.3 `CLAUDE.md`: the real-world set's "what it still lacks" sentence. Verify no other
      sentence in it still says Multi-Paxos is crash-stop
- [x] 10.4 `./scripts/check.sh`, then `./scripts/check-durability-tests.sh` and
      `./scripts/check-safety-tests.sh`, all green before the commit
