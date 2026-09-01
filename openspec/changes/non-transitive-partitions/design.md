## Context

`connected(a, b)` (sim.rs:665) asks whether `a` and `b` share a group in `partitions:
Option<Vec<BTreeSet<NodeId>>>`. That representation cannot express a bridge, because group membership
is an equivalence relation and reachability under a bridge is not transitive.

## Goals / Non-Goals

Goals: express a non-transitive partition; keep every existing call site working; find out what the
stack does under one, and pin whatever that turns out to be. Non-goals: asymmetric (one-way)
severing — see below; changing any protocol; a general topology generator.

## Decisions

### Severed pairs, with `partition` as sugar over them

`severed: BTreeSet<(NodeId, NodeId)>`, normalised so `(a, b)` and `(b, a)` are one entry, and

```rust
fn connected(&self, a: NodeId, b: NodeId) -> bool { !self.severed.contains(&pair(a, b)) }
```

`partition(groups)` severs every pair spanning two groups and is otherwise unchanged; `heal()` clears
the set. The thirty-odd existing call sites keep their meaning exactly, which matters because several
of them are load-bearing — `flooding_consensus`'s split-by-false-suspicion, the majority-ack resend
tests, `leader_driven_consensus`'s no-majority half.

*Alternative — keep groups and add a separate bridge knob.* Two representations of one concept, and
`connected` would have to consult both. The pair set subsumes groups; groups do not subsume it.

### Symmetric only, and asymmetric is a separate question

A severed pair cuts both directions. One-way severing — `A` can send to `B` but not receive — is also
real, and the implementation would be nearly free, since `connected` is already evaluated
directionally at its call site in `transmit(from, to)`.

It is left out because of what it would mean for a **session**. A session is a property of a pair,
and this repository's session model rests on that: one epoch per pair, ended and re-established as a
unit. A link that works one way and not the other is not a session that has ended, and deciding what
it *is* would be a change to `docs/conditional-guarantees.md`'s model rather than a knob on the
simulator. Worth doing; not here.

The interesting failure it would add is a different one anyway — a process whose heartbeats arrive
but whose peers' do not, so it suspects everyone while nobody suspects it. That belongs with the
detector work.

### What the bridge is expected to do, and why the tests say "measure" rather than "assert"

The honest chain, and the reason this fault is worth having:

```
bridge                              A never hears from C, and C is correct
  │
  ├─▶ ◇P2 lapses                    "eventually no correct process is suspected" — false at A and C
  │     [Δ ≤ cap ∧ Δ stable]        the network's condition, not the implementation's fault
  │
  ├─▶ ELD1 lapses                   Ω inherits it; A trusts B, B and C trust C
  │     [detector settles]
  │
  ├─▶ UC4 lapses                    termination is [majority correct ∧ detector settles]
  │     [majority ∧ settles]
  │
  └─▶ UC2 must NOT lapse            agreement is [always], and a bridge is the schedule that
                                    breaks it if the annotations are wrong
```

The last line is the test worth writing. The others are *observations* — the suite records what
happens rather than asserting a predicted answer, because there is a real possibility the stack makes
progress anyway: `B` reaches both, so `{B, C}` is a majority and both trust `C`, and `A` is simply
left behind. If that is what happens it is a good outcome and the test should say so; if leadership
never settles it is also a good outcome, being exactly what `[detector settles]` warns of. Predicting
which in advance and asserting it would be writing the answer before doing the experiment.

### Non-vacuity: assert the topology is actually non-transitive

A bridge test over a topology that happens to be transitive tests nothing, and the mistake is easy —
`sever(A, C)` on three processes is a bridge; on four it may not be. So the suite asserts the shape
from `reachable`: some pair reachable, some pair not, and a third process reaching both. Same
discipline as everywhere else here.

## Risks / Trade-offs

- **Thirty call sites depend on `partition` keeping its meaning.** → Its signature and semantics are
  unchanged and it is implemented over the new primitive; the existing suites are the guard, and they
  must pass untouched.
- **The bridge may expose a liveness gap in epoch-change**, as the healed partition did last change.
  → That would be the point. It is recorded and scoped as its own decision rather than fixed inside
  this change, exactly as that one was.
- **`Config::sessions` runs interact with severing mid-run.** → `end_severed_sessions` already reads
  `connected`, so a severed pair loses its session as a partitioned one does today; the existing
  session suites cover the behaviour and must pass unchanged.

## Migration Plan

1. `severed` replaces `partitions`; `connected` reads it; `partition` and `heal` reimplemented over
   it. Every existing suite passes untouched — that is the whole of this step's evidence.
2. `sever`, `reconnect`, `reachable`, and simulator tests for them including the bridge's shape.
3. The stack under a bridge: Ω, epoch-change, Paxos. Record what happens.
4. Docs.

## Open Questions

- **Does the bridge stall the stack or route around it?** `{B, C}` is a majority and both trust `C`,
  so progress is possible in principle. The experiment decides, and step 3 is the experiment.
- **If it stalls in a way epoch-change could repair**, is that a fix here or a follow-on? The healed
  partition last change was scoped as a follow-on decision and that precedent seems right.
