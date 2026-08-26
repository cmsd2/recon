## 1. The primitive in the core

- [x] 1.1 Add a `Durable` associated type to `Protocol` and a store effect carrying it; verify a
      protocol declaring no durable state cannot construct a store effect, as a compile-fail test
      alongside the existing scope one
- [x] 1.2 Add a recovery entry point that receives retrieved durable state and can emit effects;
      verify from the core suite that a recovered protocol re-indicates without any message having
      arrived, and that a first start is distinguishable from a recovery
- [x] 1.3 Update every exhaustive match over the effect enum in `recon-core` and its tests; verify
      the workspace builds and `core_contract` passes
- [x] 1.4 Verify the existing protocols are unaffected: each declares no durable state, and the
      whole suite passes unchanged

## 2. The primitive in the simulator

- [x] 2.1 Give each process storage that survives `crash` and `restart` and is part of the seeded
      state; verify what was written is retrieved and what was held in memory is not
- [x] 2.2 Make a write take time; verify a write outstanding at the moment of a crash is
      distinguishable in the trace from one that had completed
- [x] 2.3 Implement the interrupted write as all-or-nothing chosen by the seed; verify across a
      seed range that both outcomes occur, that a completed write always survives, and that no
      partially written value is ever retrieved
- [x] 2.4 Honour the ordering rule — every store durable before any later send leaves the process;
      verify by crashing between the two and asserting the write survived and the message was never
      sent
- [x] 2.5 Record storage activity in the trace; verify a durability property can be asserted from
      the trace alone, without reading protocol state
- [x] 2.6 Verify a run involving writes, crashes and recoveries reproduces exactly from its seed

## 3. Logged perfect links

- [x] 3.1 Transcribe Module 2.4 and Algorithm 2.3 over the existing stubborn link, quoting the
      pseudocode and stating the status and space bound; verify a message from a surviving sender
      is log-delivered despite loss
- [x] 3.2 Verify the log is durable before the layer above is notified, by crashing between the two
      and asserting the message is in the retrieved set
- [x] 3.3 Verify no duplication **across a restart**: log-deliver, crash, restart, let the sender's
      retransmission arrive again, and assert it is not log-delivered twice
- [x] 3.4 Verify that contrast is real by running the same schedule against the existing perfect
      link, whose record is volatile, and showing it does deliver twice
- [x] 3.5 Verify recovery re-announces the retrieved log with no message having arrived
- [x] 3.6 Verify no creation, and that the durable set grows with distinct messages log-delivered —
      the stated bound, asserted rather than assumed
- [x] 3.7 Verify the weakened reliable delivery is stated honestly: a sender crashing before the
      message reaches anyone requires no delivery, and a schedule exists in which none occurs

## 4. Stubborn broadcast

- [x] 4.1 Implement best-effort broadcast over stubborn links that does not deduplicate; verify
      delivery repeats without bound over a long run
- [x] 4.2 Verify a process crashed at the moment of a broadcast delivers it after restarting — the
      reason this rung exists
- [x] 4.3 Verify no creation, and that this layer's own state does not grow with messages received

## 5. Logged uniform reliable broadcast

- [x] 5.1 Transcribe Module 3.6 and Algorithm 3.8 over stubborn broadcast, quoting the pseudocode
      and stating the status and space bound; verify a five-process run with no faults log-delivers
      everywhere
- [x] 5.2 Store `pending` and `delivered` and **not** `ack`; verify from the trace that no
      acknowledgement is ever written, and that a recovered process holds no acknowledgements
- [x] 5.3 Verify acknowledgements are rebuilt by re-broadcasting on recovery, and that a message
      pending before a crash is log-delivered after it
- [x] 5.4 Verify validity and uniform agreement with a minority crashing and recovering repeatedly
- [x] 5.5 Verify uniform agreement when a process log-delivers and then crashes for ever, and when
      it log-delivers, crashes and recovers — asserting it does not log-deliver a second time
- [x] 5.6 Verify the majority boundary at an **even** membership as well as an odd one, for the
      reason recorded in the majority-ack change: with an odd `N` the off-by-one is invisible
- [x] 5.7 Verify that without a majority the layer blocks rather than diverges, and that progress
      resumes when enough processes recover
- [x] 5.8 Verify every absence-of-violation assertion is paired with a minimum log-delivery count

## 6. Recording it

- [ ] 6.1 Update `docs/conditional-guarantees.md`: stable storage is no longer an unimplemented
      entry in its table, and its reading that an incarnation boundary needs no event applies to
      the ending and not the beginning
- [ ] 6.2 Update `docs/bounded-space.md`: the audit gains rungs whose unbounded state is on disk,
      and the bounded delivered-cursor is named as the mechanism that would fix them
- [ ] 6.3 Add the three rungs to the README with their status and space bounds, note what the
      fail-recovery model changes about indications, and refresh the test counts; verify the links
      resolve
- [ ] 6.4 Record as notes in the change: whether the effect-plus-ordering-rule design survived
      contact with three algorithms, whether the crash-during-write fault found anything, and
      whether two independent consumers were enough to shape the primitive or a third would have
      changed it
