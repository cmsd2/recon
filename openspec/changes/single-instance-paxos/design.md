## Context

See `proposal.md` — Why. Five things about the present state shape the approach.

**Everything here rests on a perfect failure detector, and Paxos must not.** `flooding_consensus`
needs `P`, and its suite demonstrates what that costs: one false suspicion splits the decision, for
ever. Paxos is fail-noisy — it tolerates a detector that lies — and the whole value of building it
is lost if it is only ever run where the detector behaves. The gossip work in the change before this
one made exactly that mistake in miniature, defaulting to a perfect link and hiding the property the
abstraction existed for. The parallel is close enough to name.

**The book decomposes Paxos into three abstractions.** Not one algorithm: an epoch-change (5.5), an
abortable epoch consensus (5.6), and a layer tying them together (5.7). Following that decomposition
is not deference — it is what makes each piece separately testable, and the abortable epoch
consensus is where the whole safety argument lives.

**Leader-driven consensus rebuilds its child while running.** Algorithm 5.7 starts a *new* epoch
consensus instance per epoch, initialised with the state the previous one returned when it aborted.
Nothing else here does that; every other layer constructs its children once. This is the design's
one genuinely novel composition question.

**The fail-recovery half has a stated shape already.** Algorithm 5.9's header reads
`Uses: StubbornPointToPointLinks, instance sl; StubbornBestEffortBroadcast, instance sbeb`, and both
exist here. Its `store(valts, val)` / `retrieve(valts, val)` map onto `Cx::storage` and
`on_recovery`, which the logged link and logged uniform reliable broadcast already use.

**This is eight modules.** The last change added two and was the largest so far. The sequencing
below exists so that stopping halfway leaves something coherent rather than a half-built stack.

## Goals / Non-Goals

**Goals:**

- Each of the three abstractions readable against its page and testable on its own.
- Safety demonstrated where it is hard — under a detector that lies — rather than where it is easy.
- The fail-recovery half sharing the fail-noisy half's algorithms rather than forking them.

**Non-Goals:**

- Multi-Paxos, a replicated log, or a second instance. One instance, one decision.
- Optimising the message count. Cachin's presentation is not the fewest-round version and this
  transcribes his.
- An eventually perfect failure detector as its own module. Ω is derived from the perfect detector
  already here; see Decisions.
- Bounded space. These are single-instance protocols whose state is bounded by membership and by the
  number of epochs, and the epoch count is bounded by leadership changes rather than by messages —
  so the rule in `docs/bounded-space.md` is met without a retention window.

## Decisions

### Ω comes from the perfect detector, and the departure is stated

Algorithm 2.8 derives Ω from an *eventually* perfect detector:
`upon leader ≠ maxrank(Π \ suspected) do leader := maxrank(Π \ suspected); trigger ⟨Ω, Trust | leader⟩`. This repository has a *perfect* detector, which is strictly stronger — it never
suspects a correct process — so the same construction is correct, and gives an Ω that happens to be
accurate from the start rather than eventually.

That is a departure worth stating loudly, because it is also a trap: an Ω that is never wrong makes
every test of Paxos's distinguishing property vacuous. The detector's accuracy is therefore
*withdrawn* in the tests that matter, using the same mechanism `perfect_failure_detector`'s own
suite uses — the synchrony assumption is removed and it begins accusing correct processes.

*Alternative considered — implement ◇P (Algorithm 2.7) first.* Faithful, and it gives the repository
the fail-noisy detector it lacks. It is a second protocol with its own suite before Paxos begins,
and Ω over `P` with the accuracy withdrawn produces the same inaccuracy for testing purposes.
Rejected as a prerequisite; recorded as the natural next change if a genuinely eventual detector is
wanted for its own sake.

### The epoch consensus child is a field that is replaced, not a registry

Leader-driven consensus holds one `EpochConsensus` as a concrete typed field. On `StartEpoch` it
aborts the current one, waits for the state it returns, and constructs a replacement from that state
with the new timestamp and leader.

This keeps `CLAUDE.md`'s rule — parents own children as concrete typed fields — while allowing what
the algorithm needs. What it is emphatically *not* is a map from timestamp to instance: the algorithm
has one live instance at a time, and holding several would be the string-keyed dynamism this
repository's post-mortem blames for the first attempt.

*Alternative considered — one long-lived instance parameterised by the current epoch.* Fewer
constructions, and it destroys the property that makes the algorithm safe: an aborted instance must
be silent afterwards, and an instance that is merely re-parameterised has no clean point at which it
stops answering for its old epoch. The book's `Abort`/`Aborted` handshake exists precisely to make
that boundary observable, and it is easier to get right with a fresh instance.

### The abort handshake is asynchronous, and the layer above must wait

`Abort` is a request and `Aborted` is its answer, carrying the state. Leader-driven consensus does
not construct the next instance until the previous one has answered.

This is stated because the obvious implementation — abort and immediately replace — loses the state
and with it the safety property. The window between the two is short and the temptation to collapse
it is real.

### The logged half reuses the fail-noisy modules' algorithms, not their code

Algorithms 5.8–5.11 are 5.5–5.7 with storage added, and it is tempting to make the logged versions a
flag on the volatile ones. They are separate modules here, matching the split this repository
already made between `perfect_link` and `logged_link`.

*Alternative considered — one module with a durability parameter.* It would halve the code. It would
also make the guarantee conditional on a runtime setting rather than on the type, and the failure
mode — a deployment that believes it is durable and is not — is exactly the silent kind this project
takes mechanical measures against elsewhere.

### A durable parent composes durable children through a slot — `recon_core::Slot`

**Not anticipated when this change was planned, and discovered by trying to write Algorithm 5.10.**
`Cx::with_child` and `Cx::with_child_consuming` hand a child `NoStore`, and `store.rs` said in as
many words that "scoping one store into two is a design nothing yet needs". Algorithm 5.10 needs it:
it keeps `(ets, ℓ, decision)` of its own, composes two children that each keep a record, and its
`Recovery` reads its children's records by name — `retrieve(startts, start) of instance lec` and
`retrieve(epochdecision) of instance lep.ets`.

`Slot { read: fn(&Parent) -> Option<&Child>, write: fn(Option<&Parent>, Child) -> Parent }` names
the part of a parent's record that belongs to a child, and
`Cx::with_durable_child_consuming(msg, collected, slot, f)` hands the child a store backed by it.
The child's `set` becomes a read-modify-write of the parent's record: **one write, not two**, so a
crash cannot land between a parent's record and its child's. `fn` pointers rather than closures, for
the same reason the composition mappers are: a slot names a fixed place in a type.

*Alternative considered — a keyed store, with sub-stores addressed by a child index.* It would
handle the sequence as well as the metadata, and it would reach into `Store`, `MemStore`, the
simulator and the trace. Nothing needs the sequence half: `logged_uniform_reliable_broadcast` is the
only protocol here that appends and nothing composes over it, so building it would be the framework
before its second consumer. The child's `Entry` is therefore uninhabited — a child that appends
cannot be composed, and the *signature* says so rather than a comment. `Slot`'s documentation
records the shape the sequence half would take.

*Alternative considered — build 5.10 over the volatile 5.5 and 5.6.* It compiles today and discards
exactly what the logged halves buy.

### One slot for a child that is replaced every epoch

The book gives each `lep.ts` its own record. There is one slot here, holding whichever instance is
live, and that is not a loss: the only instance ever read back is `lep.ets`, and `ets` is in the
same record. A crash can land between the parent's `store(ets, ℓ)` and the new instance's own `Init`
write, and both outcomes are safe — before it, recovery reads the previous epoch's `epochdecision`
against the new `ets`, which lock-in makes the same value; after it, recovery reads a fresh record
and has simply not decided yet.

### Safety is asserted where two leaders actually coexist, with a non-vacuity half

Every agreement assertion in the fail-noisy suite is paired with a check that the run genuinely
contained disputed leadership: more than one process observed acting as leader, in overlapping
epochs. Without it, an agreement assertion passes on a run where one leader was never challenged,
which is the shape `tests/method.rs` exists to reject.

## Risks / Trade-offs

- **The whole change may be tested where the detector behaves, proving nothing.** This is the
  specific failure the previous change made with a perfect link, and the specification names it as
  the headline obligation. → The disputed-leadership scenario is a requirement, not a nice-to-have,
  and it carries a non-vacuity clause. Reviewing it is the first thing to do if this lands and later
  looks thin.

- **Eight modules is a lot to hold, and the middle of it is the least useful place to stop.** →
  Sequenced so the fail-noisy three land together and are a coherent, tested Paxos on their own. If
  the change stops there it has delivered the algorithm; the logged half is an addition, not a
  completion.

- **The abort handshake is a state machine with a window in it.** A message arriving for an instance
  that has been asked to abort but has not answered has to be handled, and getting it wrong is a
  safety bug rather than a liveness one. → The specification requires an abandoned instance to be
  silent, and the suite pins it by delivering a message to an aborted instance and asserting nothing
  is sent.

- **The logged half's durable-before-visible obligation is easy to state and easy to violate.** The
  write must precede the send *in the handler's own text*. → `CLAUDE.md` already states this and
  `logged_link` already demonstrates it; the same `crash_on_next_write` fault is available and the
  specification requires a test that spends it.

## Migration Plan

Additive throughout. Nothing existing changes behaviour.

1. Ω over the perfect detector, with its own suite — including that it is deterministic in the
   suspected set, and that withdrawing accuracy makes it disagree.
2. Epoch-change over Ω, with its timestamp and settling properties.
3. Read/write epoch consensus, alone, driven directly: the quorum core, the abort handshake, and the
   silence of an abandoned instance. This is where the safety argument lives and it is tested before
   anything composes over it.
4. Leader-driven consensus over 2 and 3, with agreement under crashes, then agreement under a lying
   detector with its non-vacuity half. **A coherent, tested Paxos ends here.**
5. Logged epoch-change: the timestamp durable and recovered.
6. Logged read/write epoch consensus: the accepted value durable before it is revealed, recovery
   restoring it, and a test spending `crash_on_next_write`.
7. Logged leader-driven consensus, and the run that combines crashes, recoveries and a lying
   detector at once.

Rollback is per-step. Steps 5 to 7 can be abandoned without touching 1 to 4.

## Open Questions

- ~~Whether the epoch timestamp should be a plain counter or a `(round, rank)` pair.~~ **Answered by
  the page while reading it.** Algorithm 5.5 initialises `ts := rank(self)` and advances it by
  `ts := ts + N`, so each process draws from its own residue class mod `N` and no two processes can
  ever mint the same timestamp without coordinating. A plain counter would not have that property.
- Whether the logged modules should share the fail-noisy ones' message types or define their own.
  Sharing is tempting and the logged link did not do it. Answerable when step 5 is written.
