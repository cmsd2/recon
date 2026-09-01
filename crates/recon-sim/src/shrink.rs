//! Reducing a failing scenario to a smaller one that still fails.
//!
//! This is the thing a deterministic simulator can do that a black-box fault injector cannot. A
//! run here is a function of its inputs, so a candidate reduction can be *run* and the question
//! "does it still fail?" answered rather than estimated.
//!
//! # A reduced scenario is a different run, not the same one made smaller
//!
//! Worth stating first, because it is what a reader is most likely to assume wrongly. Removing a
//! step changes when every later message is drawn from the run's generator, so the result does not
//! replay a prefix of the original: it is a **new run that also satisfies the predicate**. The seed
//! is held fixed so the reduction is reproducible, not because the stream is preserved — it is not.
//!
//! Two things follow. Every candidate must be re-run rather than reasoned about, which is what this
//! module does. And a reduction can legitimately land on a scenario that fails for a *different*
//! reason than the original. The defence is the predicate: name the property, not the symptom, and
//! the report says which predicate was used.
//!
//! # What it reduces, and in what order
//!
//! Cheapest and most informative first, then round again to a fixed point:
//!
//! 1. **The horizon**, by binary search down to the earliest that still fails — the reduction that
//!    answers *when*, which is where a hand-written probe starts.
//! 2. **Steps**, by delta-debugging rather than one-at-a-time deletion. Faults here interact: a
//!    crash matters only with the partition that isolates its quorum, and removing either alone
//!    often stops the failure where removing both would not have been tried.
//! 3. **Fault detail** — a partition with fewer groups.
//! 4. **Membership**, last, because dropping a process changes quorum arithmetic. A bug that does
//!    not survive it is not a bug about that process; one that does is a much better
//!    counterexample.

use crate::scenario::{Scenario, Step};
use crate::{Config, Sim};
use core::fmt::Write as _;
use core::time::Duration;
use recon_core::{NodeId, Protocol};

/// How large a scenario is, in the terms the search reduces.
///
/// Every reduction the search accepts leaves this no larger in any component and strictly smaller
/// in one, which is what makes the loop terminate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub nodes: usize,
    pub steps: usize,
    pub horizon: Duration,
    /// Groups across every partition step — the only fault with internal structure to simplify.
    pub groups: usize,
}

impl Size {
    fn of<C>(s: &Scenario<C>) -> Size {
        Size {
            nodes: s.nodes.len(),
            steps: s.steps.len(),
            horizon: s.horizon,
            groups: s
                .steps
                .iter()
                .map(|(_, st)| match st {
                    Step::Partition(g) => g.len(),
                    _ => 0,
                })
                .sum(),
        }
    }
}

/// What a reduction found, and what it cost.
#[derive(Debug, Clone)]
pub struct Reduction<C> {
    /// The smallest scenario found. It satisfies the predicate: it was run and checked.
    pub scenario: Scenario<C>,
    /// What the predicate was called. Recorded because a reduction can legitimately land on a
    /// different failure than the one it started from, and the reader needs to know what was
    /// actually being hunted.
    pub predicate: String,
    /// How many candidates were run, the original included.
    pub candidates: usize,
    pub before: Size,
    pub after: Size,
}

impl<C> Reduction<C> {
    /// Whether the search found anything to remove.
    pub fn reduced(&self) -> bool {
        self.before != self.after
    }

    /// A summary for a test to print when it fails.
    pub fn report(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "shrunk against predicate `{}`", self.predicate);
        let _ = writeln!(
            s,
            "  steps {} -> {}, nodes {} -> {}, horizon {:?} -> {:?}",
            self.before.steps,
            self.after.steps,
            self.before.nodes,
            self.after.nodes,
            self.before.horizon,
            self.after.horizon
        );
        let _ = writeln!(s, "  {} candidates run", self.candidates);
        if !self.reduced() {
            let _ = writeln!(s, "  nothing came out — the scenario was already minimal");
        }
        s
    }
}

impl<C: core::fmt::Debug> Reduction<C> {
    /// The reduced scenario as Rust that reconstructs it, under the report.
    pub fn to_rust(&self, name: &str) -> String {
        format!("{}\n{}", self.report(), self.scenario.to_rust(name))
    }
}

/// How precisely the horizon search narrows. Twenty runs for a one-second horizon; narrowing
/// further costs a run per halving and buys a counterexample nobody reads differently.
const HORIZON_RESOLUTION: Duration = Duration::from_micros(1);

/// Search for a smaller scenario whose run still satisfies `predicate`.
///
/// `build` is handed the candidate's configuration and membership and returns a simulator over
/// them, exactly as for [`Sim::run_scenario`] — it is called once per candidate, so it must be
/// able to construct a fresh run each time. `predicate` is evaluated on the finished run.
///
/// `predicate_name` is carried into the report. Name the property, not the symptom.
///
/// # Panics
///
/// If the original scenario does not satisfy the predicate. There is then nothing to reduce, and
/// returning something would mean returning a scenario that does not fail — which is the one
/// outcome worse than returning the original.
///
/// The predicate itself must be total: return `false` for a run that does not exhibit what you are
/// hunting, and do not assert. A predicate that panics makes the search unable to reject a
/// candidate.
pub fn shrink<P>(
    scenario: &Scenario<P::Cmd>,
    predicate_name: &str,
    build: impl Fn(Config, &[NodeId]) -> Sim<P>,
    predicate: impl Fn(&Sim<P>) -> bool,
) -> Reduction<P::Cmd>
where
    P: Protocol,
    P::Cmd: Clone,
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
    P::Meta: Clone,
    P::Entry: Clone,
{
    let mut candidates = 0;
    let mut holds = |s: &Scenario<P::Cmd>| {
        candidates += 1;
        predicate(&Sim::run_scenario(s, |c, n| build(c, n)))
    };

    assert!(
        holds(scenario),
        "shrink: the original scenario does not satisfy `{predicate_name}` — nothing to reduce"
    );

    let before = Size::of(scenario);
    let mut best = scenario.clone();
    loop {
        let mark = Size::of(&best);
        best = shrink_horizon(best, &mut holds);
        best = shrink_steps(best, &mut holds);
        best = simplify_faults(best, &mut holds);
        best = shrink_membership(best, &mut holds);
        if Size::of(&best) == mark {
            break;
        }
    }

    let after = Size::of(&best);
    Reduction { scenario: best, predicate: predicate_name.to_string(), candidates, before, after }
}

fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// Binary search for the earliest horizon that still fails.
///
/// Assumes that a predicate true at some horizon stays true at a longer one — which holds for
/// "something happened" predicates, the kind worth hunting. Where it does not, the search still
/// returns a horizon at which the predicate holds, because every accepted candidate was run; it
/// just may not be the earliest.
fn shrink_horizon<C: Clone>(
    scenario: Scenario<C>,
    holds: &mut impl FnMut(&Scenario<C>) -> bool,
) -> Scenario<C> {
    let mut lo = 0u64;
    let mut hi = nanos(scenario.horizon);
    if hi == 0 {
        return scenario;
    }
    let mut best = scenario;
    while hi - lo > nanos(HORIZON_RESOLUTION) {
        let mid = lo + (hi - lo) / 2;
        let candidate = best.with_horizon(Duration::from_nanos(mid));
        if holds(&candidate) {
            hi = mid;
            best = candidate;
        } else {
            lo = mid;
        }
    }
    best
}

/// Delta-debugging: `ddmin` over the step list.
///
/// One-at-a-time deletion is not enough here. A crash matters only together with the partition
/// that isolates its quorum, so removing either alone stops the failure and the pair is never
/// tried. `ddmin` tries complements as well as chunks, at increasing granularity, which finds it.
fn shrink_steps<C: Clone>(
    scenario: Scenario<C>,
    holds: &mut impl FnMut(&Scenario<C>) -> bool,
) -> Scenario<C> {
    let mut best = scenario;
    let mut n = 2usize;
    loop {
        let len = best.steps.len();
        if len < 2 {
            return best;
        }
        let n_now = n.min(len);
        let bounds: Vec<(usize, usize)> = (0..n_now)
            .map(|i| (len * i / n_now, len * (i + 1) / n_now))
            .filter(|(a, b)| a < b)
            .collect();

        // Reduce to a subset: keep one chunk, drop everything else.
        let mut progressed = false;
        for &(a, b) in &bounds {
            let drop: Vec<usize> = (0..len).filter(|i| *i < a || *i >= b).collect();
            if drop.is_empty() {
                continue;
            }
            let candidate = best.without_steps(&drop);
            if holds(&candidate) {
                best = candidate;
                n = 2;
                progressed = true;
                break;
            }
        }
        if progressed {
            continue;
        }

        // Reduce the complement: drop one chunk, keep everything else.
        for &(a, b) in &bounds {
            let drop: Vec<usize> = (a..b).collect();
            let candidate = best.without_steps(&drop);
            if holds(&candidate) {
                best = candidate;
                n = (n_now - 1).max(2);
                progressed = true;
                break;
            }
        }
        if progressed {
            continue;
        }

        if n_now >= len {
            return best;
        }
        n = (n_now * 2).min(len);
    }
}

/// Simplify the faults that have internal structure to simplify.
///
/// Only a partition does: everything else the simulator can be told to do is atomic, and is
/// reduced by being deleted. Merging two groups makes the partition less severe — strictly fewer
/// pairs cut — without changing when it happens or who is in the run.
fn simplify_faults<C: Clone>(
    scenario: Scenario<C>,
    holds: &mut impl FnMut(&Scenario<C>) -> bool,
) -> Scenario<C> {
    let mut best = scenario;
    let mut i = 0;
    while i < best.steps.len() {
        let Step::Partition(groups) = &best.steps[i].1 else {
            i += 1;
            continue;
        };
        if groups.len() < 2 {
            i += 1;
            continue;
        }
        let mut merged_any = false;
        'pairs: for a in 0..groups.len() {
            for b in (a + 1)..groups.len() {
                let mut next: Vec<Vec<NodeId>> = Vec::new();
                for (k, g) in groups.iter().enumerate() {
                    if k == b {
                        continue;
                    }
                    if k == a {
                        let mut joined = g.clone();
                        joined.extend(groups[b].iter().copied());
                        next.push(joined);
                    } else {
                        next.push(g.clone());
                    }
                }
                let candidate = best.with_step(i, Step::Partition(next));
                if holds(&candidate) {
                    best = candidate;
                    merged_any = true;
                    break 'pairs;
                }
            }
        }
        if !merged_any {
            i += 1;
        }
    }
    best
}

/// Drop a process, and every step that named it.
///
/// Last, and deliberately: this changes quorum arithmetic, so most bugs will not survive it. One
/// that does is a counterexample about the algorithm rather than about the run's size.
fn shrink_membership<C: Clone>(
    scenario: Scenario<C>,
    holds: &mut impl FnMut(&Scenario<C>) -> bool,
) -> Scenario<C> {
    let mut best = scenario;
    let mut i = 0;
    while i < best.nodes.len() {
        let node = best.nodes[i];
        let candidate = best.without_node(node);
        if holds(&candidate) {
            best = candidate;
        } else {
            i += 1;
        }
    }
    best
}
