## Why

When a seeded property fails here, what you get is a seed. Twenty-three test files search or assert
over `for seed in 0..40`, and a failure from one of them says "seed 17: two processes decided
differently: `[3, 5]`" and nothing else. From there the work is by hand: write a throwaway test,
print some state, read it, narrow it, delete it.

A deterministic simulator can do better than that, and this is the thing it can do that Jepsen
cannot — Jepsen hands you a long history and cannot reliably reproduce it, so it cannot minimise.
Here a run is a function of its inputs, so a failing run can be re-run with one fault removed, or a
shorter horizon, or one process fewer, and the question "does it still fail?" has a reliable answer.
Repeat until nothing more can come out, and what is left is a counterexample somebody can read.

**A correction, since this change dates the claim that motivated it.** `README.md` says of shrinking
that "every diagnosis in these notes was reached by hand-writing a throwaway probe … a shrinker
would have handed those over", and names three. Checked against what actually happened, it would
have handed over none of them:

| Diagnosis | What was actually needed |
|---|---|
| the epoch that climbed to 647,309 | to **see** `ets` — the run was already one crash, nothing to minimise |
| the send rate growing 12.6k → 76.6k | nothing was failing; it was a measurement, not a counterexample |
| the leader trusted by everyone that announced nothing | bisection across the **stack** — Paxos, then epoch-change, then Ω, then ◇P — not across a schedule |

All three were visibility problems. That is the roadmap's item `F`, and this change corrects the
sentence rather than leaving a claim the repository's own history contradicts. Shrinking is still
worth having, for a narrower and more honest reason: it answers *when* and *with how little*, which
is where a hand-written probe starts rather than where it ends.

## What Changes

- **A scenario is data.** `Scenario` — a config with its seed, a membership, a list of timed steps
  (commands and faults), and a horizon — plus `Sim::run_scenario` to execute one. Everything the
  simulator can currently be told to do imperatively becomes something that can be held in a value,
  compared, printed and taken apart.
- **A shrinker**, `shrink(scenario, predicate)`, which repeatedly proposes a smaller scenario, runs
  it, and keeps it if the predicate still holds. It shortens the horizon, deletes steps, simplifies
  a partition, and drops a process from the membership, to a fixed point.
- **The result prints as runnable Rust**, so the end of a shrink is a test you can paste rather than
  a description you must transcribe.
- **The shrinker is tested against a bug that was real.** One of the defects this project has
  already found and fixed is reintroduced behind a test-only switch, and the shrinker is required to
  reduce it. A shrinker demonstrated only on a toy is a shrinker nobody has tested.

Deliberately not in scope: converting the twenty-three existing suites. They are written
imperatively and are clearer that way — a test that provokes one named condition is not improved by
becoming a data structure. Scenarios are for the searching kind, where the failing input is
discovered rather than chosen.

## Capabilities

### Modified Capabilities

- `simulation`: a run can be described as a value and executed from it, and a failing scenario can
  be reduced to a smaller one that still fails

## Impact

`recon-sim` gains `Scenario`, `Step`, `Sim::run_scenario` and `shrink`, and a suite for them.
`P::Cmd` acquires a `Clone` bound where scenarios are used, since a step holds a command and
shrinking re-runs it. No protocol changes; no existing suite changes. `README.md`'s roadmap is
corrected where it overstates what this buys.
