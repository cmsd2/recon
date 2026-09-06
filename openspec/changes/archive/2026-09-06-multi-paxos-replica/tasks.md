## 1. The source, pinned

- [x] 1.1 Same edition as change 1 — **van Renesse, R. and Altinbuken, D. (2015), *ACM Computing
      Surveys*, 47(3)** — and this change quotes **Figure 1** (the replica) and §2.1. Verify the
      figure number against the survey before quoting it; the 2011 report numbers it differently
- [x] 1.2 Record in the module that the fourth liveness violation implemented in group 5 is Liu,
      Chand and Stoller (2019)'s, cited where it departs from Figure 1. Verify `CLAUDE.md`'s
      reference table needs no change, since both papers are already named there

## 2. The Synod modification, finished and green before the replica exists

- [x] 2.1 `MultiPaxosSynod` keeps the **identity** of the trusted process rather than a boolean:
      `trusted: Option<NodeId>`, set from `⟨ Ω, Trust | p ⟩`. Verify the existing suite still passes
      unchanged, since every current use asks only whether it is this process
- [x] 2.2 `SynodMsg::Decision { slot, command }`, and a commander that broadcasts it to every
      member of the run before raising `Ind::Decision` locally — Figure 6(a)'s `∀ρ ∈ replicas :
      send(ρ, ⟨decision, s, c⟩)`, restored now that there is a replica to address; the addressee is
      the process, colocation making the sets one. Quote the restored line and remove the module's
      note saying it was dropped. Verify every process raises `Ind::Decision`, not only the one
      whose commander counted the majority
- [x] 2.3 Verify a repeated decision for a slot names the same command: drive a later ballot to
      re-command a decided slot and assert both indications carry one command. State in the module
      that the layer above must be idempotent, and why suppressing it here is not free
- [x] 2.4 `SynodMsg::Propose { slot, command }`, and a process that is neither active nor trusted
      forwards to `trusted` — §4.4's colocation, quoted. Verify a proposal made at a passive process
      reaches the leader and is commanded under the leader's ballot
- [x] 2.5 **A forwarded proposal is not forwarded again.** A `Propose` that arrived is handled
      locally or dropped. Verify with two processes whose detectors disagree about who leads: assert
      the exchange is bounded, and that the request is dropped rather than circulating
- [x] 2.6 Verify a trusted process forwards nothing and proposes for itself, and that a process
      that is active but not trusted still commands rather than forwarding — its adopted ballot
      stands until preempted, and the ordering of the two conditions is what this pins
- [x] 2.7 The leader keeps `decided`, marked when a commander completes, and answers a `Propose`
      for a decided slot with `Decision { slot, command }` to the **asker alone**, taking the
      command from its own proposal for the slot — the leader-side half of Liu et al.'s fix,
      cited: "otherwise, it can send back the decision for that slot". Verify the answer goes to
      one process, not a fan-out, and carries the decided command
- [x] 2.8 Verify the answer is what unwedges a starved process: decide a slot, drop the decision
      to one process specifically, re-propose from it, and assert its `Ind::Decision` arrives via
      the answer. Then verify the other half of the guard: a `Propose` for a slot proposed but not
      yet decided draws no answer and starts no second commander
- [x] 2.9 Update `consensus/multi-paxos-synod`'s progress requirement in the module documentation:
      proposal *delivery* now rests on the detector, where before an inaccurate detector cost
      nothing for delivery. Verify the module's departures list says so alongside the Ω departure
- [x] 2.10 Verify a proposal forwarded to a crashed process is lost and nothing recovers it at this
      layer — the cost of colocation, tested rather than assumed. Assert the non-vacuity half: the
      detector really named the crashed process, and the forward really happened
- [x] 2.11 `./scripts/check-safety-tests.sh` still passes: the two mutations must still be caught by
      the tests registered against them, and the decision broadcast must not have made any of them
      pass for a new reason. Add any newly-detecting test to the registration
- [x] 2.12 `./scripts/check.sh` passes with the Synod suite green, before any replica code is
      written

## 3. The command, and the wire it rides

- [x] 3.1 `Command<V>` as the source's `c = ⟨κ, cid, op⟩` — the appending process, a request
      identifier, and the value. Verify it survives encoding as every wire type here does, and that
      two appends of the same value differ
- [x] 3.2 State the request identifier's scope in the module: **this incarnation**, volatile, and
      what a reused `⟨κ, cid⟩` would cause after a restart. Verify the documentation says it, in the
      same terms the ballot's scope is stated

## 4. The replica, Figure 1

- [x] 4.1 `MultiPaxosReplica<V, L>` holding `Child<MultiPaxosSynod<Command<V>, L>>` and nothing
      else of consequence, with `type Msg` the child's — this layer adds no header. Quote Figure 1
      above the implementation and state where each of its parts went, including the two that are
      absent. Verify the module compiles with one link type parameter and no broadcast child
- [x] 4.2 `slot_in`, `slot_out`, `requests`, `proposals`, `decisions`, with R1–R5 stated. Verify
      `decisions` is append-only, as the page has it and says why
- [x] 4.3 `propose()`: move requests into unused slots within the window, and hand each to the child
      as `Cmd::Propose`. Quote the function. Verify a request appended while the process is passive
      is proposed once a leader exists
- [x] 4.4 `perform()`: the already-decided check, then extend the sequence. Quote the function and
      state that `op(state)` and the client response are absent because the port is a log. Verify a
      command decided in two slots takes **one** position — drive that case deliberately rather than
      hoping a run produces it
- [x] 4.5 Verify `Position` and `Slot` diverge exactly by the duplicate count: assert positions are
      contiguous from `Position::START` while `slot_out` has run further. This is the mix-up the
      types cannot catch
- [x] 4.6 Decisions arriving out of order and more than once: hold them, extend the sequence only in
      position order, and never shorten it. Verify with decisions delivered in reverse, and assert
      the sequence extends monotonically across repeated reads
- [x] 4.7 A request whose slot was decided for a different command goes back into `requests` and is
      proposed again. Verify with two processes proposing different commands for one slot, and
      assert the loser's command still reaches the sequence

## 5. Liveness, and the fourth violation

- [x] 5.1 One periodic timer, compared against its registered `TimerId` before acting. Verify a
      process registering it does not act on the child's expiries
- [x] 5.2 Liu et al.'s fourth violation: a proposal outstanding beyond a threshold is proposed
      **again for the same slot**. Verify by dropping the decision for one slot specifically, not by
      switching on a lossy link, and assert that without the timeout `slot_out` stalls and `WINDOW`
      fills — then that with it the sequence completes, **through the leader's answer** from task
      2.7: assert the recovery arrived as a directed `Decision`, not by luck of a re-run
- [x] 5.3 Verify re-proposal is harmless where the slot is not stalled: drive re-proposals against a
      healthy run and assert the sequence is unchanged and nothing is ordered twice
- [x] 5.4 The `WINDOW` guard (R5): verify proposals stop a configured distance ahead of `slot_out`
      and resume when the sequence advances. State in the module that the reconfiguration reason for
      the window is deferred, so a reader does not take it for a pipeline cap alone
- [x] 5.5 Choose the re-proposal threshold against `detect_after` and the delivery bound — the
      design's first open question — and say in the test where the number comes from rather than
      leaving it a constant

## 6. The port, and the shared suite

- [x] 6.1 Implement `TotalOrderLog<V>`: `append`, `read` from a `Position`, and a total `classify`.
      Verify the port's `compile_fail` doctest still rejects a non-log
- [x] 6.2 `read` serves the applied prefix from this process's own copy, as the port's header
      requires. Verify a read at a process whose slot is undecided returns the shorter prefix rather
      than waiting
- [x] 6.3 Register the implementation in `tests/total_order_log.rs` so every shared property runs
      against it. Verify all of them pass
- [x] 6.4 **Check each shared property is non-vacuous against this implementation**, rather than
      assuming it carries over: the suite was written for two implementations that decide a batch
      per round, and this one decides slots independently and out of order. Verify in particular
      that `the_run_contained_overlapping_operations` still holds, and say in the suite what makes
      each property meaningful here
- [x] 6.5 Verify the crash property: a minority crashes and is **not** restarted, and the survivors
      keep ordering. Assert the crash landed before the step that depends on it

## 7. Scope, space and the boundary

- [x] 7.1 A session ending from the child is propagated, not absorbed. Verify it reaches the layer
      above and that the module states it bridges nothing
- [x] 7.2 State the space bound in the module: unbounded, a transcription, and §4.2 is what bounds
      it. Verify `docs/bounded-space.md` gains a row saying the same, and that the Synod row is
      amended for the decision broadcast's per-decision work
- [x] 7.3 Verify the send rate is flat once the work is done — `tests/common::assert_send_rate_flat!`
      — and say what keeps it flat, since the re-proposal timer is the thing that could make it not
- [x] 7.4 Verify the module documents the crash-stop boundary and what it adds to the core's: a
      returning replica would report a shortened sequence, which the ordering requirement forbids

## 8. What this dates

- [x] 8.1 `README.md`: the protocol table, the suite table and counts, and the specification tree.
      Verify the counts against `cargo test --workspace`
- [x] 8.2 `README.md`'s roadmap item 5 — advance it from "the Synod core is built" to "there is a
      log", and say that bounding is what remains before the real-world set
- [x] 8.3 `README.md`'s real-world set section: the obligation still unmet is resource use, and this
      change does not meet it. Verify the wording does not now imply it does
- [x] 8.4 `./scripts/check.sh` passes in full
- [x] 8.5 `./scripts/check-safety-tests.sh` and `./scripts/check-durability-tests.sh` both pass. The
      replica registers nothing durable, but the guards must not have been broken by the new suite
