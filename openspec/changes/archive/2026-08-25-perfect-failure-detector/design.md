## Context

See `proposal.md` — Why. The governing constraints are `docs/postmortem.md` §5 and `CLAUDE.md`;
the conventions the code already follows are listed there and are not restated.

Four rungs exist, all asynchronous: nothing built so far assumes anything about how long a message
takes. This change breaks that, necessarily — a perfect failure detector is the abstraction that
encapsulates a synchronous system's timing assumptions, and there is no way to detect a crash
accurately without one. Most of the design work is therefore about where that assumption lives and
how visible it is, not about the detector, which is small.

## Goals / Non-Goals

**Goals:**

- A detector whose two properties are testable, including the case where its assumption is
  withdrawn and accuracy is expected to fail.
- A synchronous mode in the simulator that constrains *timing* only, leaving crashes and
  partitions to behave exactly as before.
- The timing assumption stated where a reader meets the detector, not buried in a config value.

**Non-Goals:**

- The eventually perfect failure detector of the partially synchronous model. It is the more
  practical abstraction and it is a later rung.
- Uniform reliable broadcast, which follows and is what motivated this.
- Implementing the `Scope` associated type from `docs/scope-annotated-modules.md`. The detector is
  the clearest case for it yet, and it still waits for a second consumer.

## Decisions

### 1. The synchronous mode constrains timing, not failure

Configuring a run synchronous fixes delivery at or within a bound and disables loss and
duplication. It does **not** disable crashes or partitions.

*Why:* a failure detector with nothing to detect is untestable. The synchronous model in the book
is one where processes still crash and messages still take bounded time; it is timing that is
known, not reliability of processes. Partitions are the interesting edge — under a partition the
detector will accuse a live process, and that is correct behaviour for P given its assumption has
been violated, not a defect. The specification says so explicitly.

### 2. The bound is readable from the configuration

A test configures the network's bound and the detector's timeout from the same value rather than
guessing a timeout that happens to work.

*Why:* a detector whose timeout is tuned by trial against a network it cannot interrogate is a
flaky test waiting to happen, and the flakiness would look like a protocol defect. It also makes
the dependency between the two visible: the detector is correct *because* the network promised
something.

### 3. Heartbeats, not the simulator's knowledge of who crashed

The detector sends heartbeats and times out. It does not ask the simulator which processes have
crashed, even though the simulator knows.

*Why:* the simulator is the network, not an oracle. A detector that consulted it would be
correct by construction and would test nothing — and the same protocol must be able to run over a
real network later, where no such oracle exists. This is the same discipline that keeps protocols
sans-IO.

*Alternative considered:* an oracle detector as a test fixture, so that uniform reliable broadcast
could be developed before the real detector works. Rejected for now — it is the kind of scaffolding
that outlives its purpose — but it is worth reconsidering if the detector proves slow to get right,
since URB's correctness does not depend on *how* P is implemented.

### 4. The detector is a peer, not a layer

Uniform reliable broadcast will own both a best-effort broadcast and a detector. The detector is
therefore not composed beneath anything in this change; it stands alone and is tested alone.

*Consequence for the next change:* URB is the first protocol to own two children, which is the
case the previous change's notes named as the one that would reopen the macro question. Two
children mean two helper methods with near-identical bodies; if they read as duplicates, that is
the second instance the deferred decision was waiting for.

### 5. Crash indications are raised once and are permanent

The detector's state is the set of processes already reported. A process is reported once and
never un-reported.

*Why:* strong completeness demands permanence, and a detector that re-reported would push
deduplication onto every consumer. Algorithm 3.4 removes from `correct` on each `⟨P, Crash | p⟩`;
repeated indications would be harmless there but not everywhere.

## Risks / Trade-offs

**A timing assumption leaks into layers that should not have one** → uniform reliable broadcast
will depend on P, and therefore on synchrony, while its own specification says nothing about
timing. Mitigation: state the dependency in URB's specification when it is written, as the book
does when it distinguishes fail-stop from fail-silent algorithms. The honest reading is that
Algorithm 3.4's guarantees are conditional on the detector's, which are conditional on the network's.

**The synchronous mode becomes the default in tests because it is easier to reason about** → and
the asynchronous behaviour that every other rung is tested under quietly stops being exercised.
Mitigation: the existing suites are not migrated, and the mode is opt-in per run.

**Timing tests that pass by luck** → a detection test with a generous margin can pass without the
detector working, and a tight one can fail spuriously. Mitigation: assert both directions — that
a crashed process *is* detected within a stated multiple of the bound, and that a correct one is
*never* detected however long the run continues.

**Scope creep into the eventually perfect detector** → the partially synchronous version is more
useful and more interesting, and it is a different rung. Mitigation: it is a stated non-goal.

## Open Questions

- **Whether the detector needs its own wire message or can share the broadcast's.** Heartbeats are
  a distinct concern and probably want their own type, but if URB ends up multiplexing both over
  one link, that is a composition question this change need not settle.
- **What the detection bound should be as a multiple of the delivery bound.** A matter of
  calibration; it changes no requirement and no task.
