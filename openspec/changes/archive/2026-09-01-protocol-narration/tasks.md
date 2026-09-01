Neither of `design.md`'s open questions changes what gets built here: the general coverage test bites
at the *second* module narrated, and per-layer spans matter only once two layers narrate the same
decision. Both are recorded there rather than resolved.

## 1. The note channel

- [x] 1.1 `Protocol::Note`, beside `Scope` and documented with the same argument: an uninhabited
      type has no values, so a protocol declaring `Infallible` cannot construct a note. Written out
      at each implementation rather than defaulted — associated type defaults are not stable, and
      `Scope` is spelled out everywhere for the same reason
- [x] 1.2 `Cx` gains a note sink and its type parameter, and `cx.note(..)` records **eagerly**, at
      the point of the call. Not an `Effect`: an effect is deferred, and a note describes something
      that has already happened — the same reason storage is not an effect
- [x] 1.3 Verify `ProtoCx<'a, P>` absorbs the parameter: the algorithms compile unchanged. The eight
      bare `Cx<` sites in `cx.rs`, `child.rs` and `core_contract.rs` are the whole edit
- [x] 1.4 Verify a protocol declaring `Note = Infallible` cannot narrate — a `compile_fail` doctest,
      as the link and detector ports already use
- [x] 1.5 `with_child`, `with_child_consuming`, `with_durable_child_consuming` and `Child` pass a
      note through untouched. **Not** by `P::Note: From<C::Note>` as planned: that was built and
      discarded, because every layer generic over a port then needed a `where` clause relating type
      parameters to express a conversion that was the identity everywhere it was instantiated. The
      child is handed the parent's own note sink, the way it is handed the source of timer
      identities — a note's vocabulary belongs to the run, not to a layer, and the ports say so.
      Verified in `core_contract.rs` that a parent does not restate a child's decision

## 2. A note in the trace

- [x] 2.1 `TraceEvent::Said { at, node, note }`, and the note parameter on `Trace`. Ten `TraceEvent<`
      and five `Trace<` mentions across the suites follow mechanically
- [x] 2.2 Verify a narrated decision reaches the trace attributed to the process and the instant, and
      that a note narrated before a send precedes it in the trace
- [x] 2.3 **Verify narrating does not change the run**: one seed run twice, once observed and once
      not, agreeing on every event but `Said`. Without this, narration is a fault injector and every
      diagnosis read from an observed run is a diagnosis of a different run
- [x] 2.4 Verify nothing a protocol can observe reveals whether anything is listening

## 3. Rendering

- [x] 3.1 `recon-sim` depends on `tracing`; `recon-core` and `recon-protocols` do not. Verify by the
      manifests
- [x] 3.2 Emit each recorded event to a subscriber **as it is recorded**, carrying `node` and the
      virtual `at` as fields rather than as an enclosing span — a span per event buys nothing, and
      the fields are what a subscriber prints. Live rather than a walk over a finished trace, because a run that hangs
      is one worth reading
- [x] 3.3 Off unless asked for, like `enable_codec_check` and `deliver_session_events`. Verify a run
      without an audience behaves exactly as it does today
- [x] 3.4 Verify the time rendered is the run's, not the wall clock's — a subscriber's own timestamp
      measures how long the simulation took, which is unrelated to the run and misleading read as
      though it were

## 4. The vocabulary, and one module narrated

- [x] 4.1 A shared `Note` enum in `recon-protocols`, and `impl From<Infallible> for Note`
- [x] 4.2 Narrate `epoch_change` at the decisions that produce **no effect**, which is what the trace
      cannot hold: a `NewEpoch` refused because the sender is not trusted or its timestamp is not
      newer, a `NACK` ignored because the candidate has already passed it, and a report sent to a new
      leader because `started_by` disagreed with it
- [x] 4.3 Do **not** narrate a decision the trace already records. A note beside
      `cx.indicate(Ind::StartEpoch { .. })` restating it adds nothing and can drift, which is the
      decay this whole design is built against. Verify by review that no note duplicates an effect

## 5. That the narration agrees with the run

The three checks are of different strengths and the tests say which, rather than implying one.

- [x] 5.1 **An action taken**: as planned this named epoch *starts*, which 4.3 forbids narrating —
      the trace already holds the indication. The property is unchanged and the subject moved to the
      note that does precede an action: every `ReachReported` is a `NACK` carrying that timestamp,
      sent to that leader
- [x] 5.2 **An action refused**: where a process narrates refusing a `NewEpoch`, no `StartEpoch`
      follows from that delivery
- [x] 5.3 **Coverage**: every *distinct* announcement reaching a process is accounted for as either
      entered or narrated as refused — distinct, because the stubborn broadcast beneath re-sends each
      one and the perfect link discards all but the first, so roughly fifteen hundred deliveries
      carry about eight announcements. A crashed and restarted process is checked as a bound rather
      than an equality, because a restart loses the link's deduplication state and the wire
      identifier cannot tell the two occasions apart. This is the check that catches narration
      falling quietly out of a clause
- [x] 5.4 Non-vacuity: assert the notes are actually there. An agreement property is satisfied by a
      protocol that narrates nothing, exactly as an absence-of-violation property is satisfied by a
      protocol that does nothing

## 6. What this dates

- [x] 6.1 `README.md`'s roadmap: mark `F` built, and say what it buys as against what `E` bought —
      the archived `scenario-shrinking` change concluded that the reduction answered *when* and *with
      how little* and said nothing about *why*, and named this as the item that answers why
- [x] 6.2 `README.md`'s `recon-sim` and `recon-core` sections, the suite table and the counts
- [x] 6.3 `./scripts/check.sh` passes in full
