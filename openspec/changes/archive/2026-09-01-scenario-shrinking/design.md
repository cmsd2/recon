## Context

See `proposal.md` — Why. The constraint that makes this possible is the one the whole project rests
on: a run is a function of its seed and configuration, so a candidate reduction can be *run* and the
question "does it still fail?" answered rather than estimated. The constraint that makes it awkward
is that the twenty-three suites which would benefit are written imperatively, and a `Sim` cannot be
asked what was done to it.

## Goals / Non-Goals

Goals: express a run as data; reduce a failing one; report the result as something runnable; and
demonstrate the reduction on a defect this project actually had. Non-goals: converting the existing
suites, generating scenarios randomly, and any form of state-space exploration — this reduces a
scenario somebody already has, it does not go looking for one.

## Decisions

### The reduced scenario is a different run, not the same one made smaller

Worth stating first because it is the thing most likely to be misread. Removing a step changes when
every later message is drawn from the generator, so the reduced scenario does not replay a prefix of
the original — it is a **new run that also satisfies the predicate**. The seed is held fixed for
reproducibility, not because the RNG stream is preserved; it is not.

That is normal for shrinking and it is why every candidate must be re-run rather than reasoned about.
It also means a reduction can, legitimately, produce a scenario that fails *for a different reason*
than the original. The defence is the predicate: make it name the property, not a symptom, and the
report says which predicate was used.

### Reductions, in the order they are tried

Cheapest and most informative first, then to a fixed point:

```
1. horizon        binary search down to the earliest that still fails
                  → answers "when", which is where a hand-written probe starts
2. steps          delete one; then delete contiguous runs, halving
                  → answers "with how little"
3. fault detail   3 partitions → 2; a set of severs → fewer; drop a peer from a group
4. membership     5 processes → 4 → 3
                  → tried last: it changes quorum arithmetic, so a bug may not survive,
                    and a survivor is a much better counterexample
```

Delta-debugging rather than one-at-a-time deletion for step 2, because faults here interact — a
crash matters only with the partition that isolates its quorum — and removing either alone often
stops the failure while removing both would not have been tried.

### A reduction repairs the pairing it breaks — found the hard way

Not anticipated when this was written, and it cost a failing test to find. A `Resume` belongs to a
`Suspend` and a `Restart` to a `Crash`, and the simulator refuses each without its partner — rightly,
since resuming a crashed process would be a pause pretending to be a recovery. Deleting steps is
precisely what a reduction does, so a naive reduction spends most of its candidates on runs that
panic rather than on runs that answer the question.

Every reduction therefore **repairs** rather than rejects: a walk over the steps tracking each
process's liveness drops the ones now dangling. A candidate that has lost its `Suspend` is still a
candidate — it is one where that process never stopped. `Scenario::is_well_formed` exposes the same
walk so a test can say which kind of scenario it is holding.

The alternative — loosening `resume`/`restart` to no-op on the wrong liveness — was rejected. That
check is deliberate and documented; a shrinker is not a reason to weaken it.

### The predicate is a function of the finished run

`impl Fn(&Sim<P>) -> bool`, evaluated after the scenario has run to its horizon. Not a callback
during the run, which would make the reduction depend on when it fired.

A predicate must be **total and honest**: true means "this run exhibits what I am hunting". A
predicate that panics rather than returning false makes the search unable to reject a candidate, so
the runner catches nothing and the contract is stated instead — return `false`, do not assert.

### Termination, and the shrinker's own determinism

Every reduction strictly decreases a well-founded measure — steps, horizon, membership — so the loop
terminates. Candidates are proposed in a fixed order with no randomness, so the same input yields the
same output, which is what makes a reduction reportable. A shrinker somebody cannot reproduce is a
shrinker whose answer nobody can check.

### The report is Rust, not prose

The end of a reduction is a printed `Scenario` literal that reconstructs it. The alternative — a
description to transcribe — is where the effort this change exists to remove would come back.

## Risks / Trade-offs

- **It only helps tests written as scenarios, and none are.** → Deliberate: the imperative suites
  provoke one named condition each and are clearer that way. Scenarios are for the searching kind,
  where the failing input is discovered. If nothing adopts it, that is the signal that this was the
  wrong item.
- **A reduction can find a different bug than the one you started with.** → Inherent, mitigated by
  predicates that name properties rather than symptoms, and by the report saying which was used.
  This happened twice while building the demonstration, and is the most useful thing the change
  turned up. A predicate comparing the two halves of a run returned a 17 ms scenario that the
  *sound* stack satisfied too, because a run that short is all startup; skipping the first quarter
  returned an 80 ms one that failed the same way, because an epoch consensus is supposed to send
  more as it works through `READ`, `WRITE` and `DECIDED`. Only "the last quarter against the third,
  in a run long enough to have gone idle" names the property the module claims. **The shrinker did
  not mislead — it exposed a predicate that named a symptom**, which is a service, but the cost of
  getting the predicate right was comparable to the cost of the probe it was meant to replace.
- **Each candidate is a full run.** → Runs here are milliseconds of real time; a reduction is
  hundreds of them. Worth measuring on the demonstration in group 4 rather than assuming.
- **`P::Cmd: Clone` is a new bound** wherever scenarios are used, since a step holds a command and
  re-running clones it. Every `Cmd` in the repository already derives `Clone`; the bound sits on the
  scenario API rather than on `Protocol`, so nothing else acquires it.

## Migration Plan

1. `Scenario`, `Step`, `Sim::run_scenario`, and the equivalence test against the imperative API.
2. The shrinker, with its own suite — including that it can fail to reduce and says so.
3. The rendering.
4. The demonstration against a reintroduced real defect.
5. Docs, including the roadmap correction.

## Open Questions

- **Whether `F` should come first.** The proposal's table is the evidence: all three diagnoses the
  roadmap cites for shrinking were visibility problems, and tracing is what would have solved them.
  This change is worth having on its own terms — it answers *when* and *with how little* — but if
  only one gets built, the history says tracing. Recorded here rather than argued in the proposal,
  because the answer is the user's. The demonstration in
  `crates/recon-protocols/tests/shrinking_a_real_defect.rs` settles the narrow version of it and
  goes in `F`'s favour: the reduction turned nine faults and five processes into one command and one
  process — and the one-process result was genuinely new — but it said nothing about *why*, and
  reaching the cause was still reading the code.
- **Whether the two want joining.** The natural product of a failed property is a *report*: the
  minimal scenario, plus a narrated run of it. That is `E` and `F` together, and neither half is
  obviously useful without the other.
