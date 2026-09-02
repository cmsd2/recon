//! A run described as a value.
//!
//! Everything the simulator can be told to do imperatively — commands, crashes, partitions,
//! session breaks — can also be held in a [`Scenario`]: a configuration with its seed, a
//! membership, a list of timed [`Step`]s, and a horizon to run to. A description can be compared,
//! printed, taken apart, and above all made *smaller*, which is what [`crate::shrink()`] does with
//! it.
//!
//! This does not replace the imperative form and is not meant to. A test that provokes one named
//! condition reads better as a sequence of calls. Scenarios are for the searching kind, where the
//! failing input was discovered rather than chosen, and the useful next question is "how much of
//! this mattered?"

use crate::{Config, Sim};
use core::fmt::Write as _;
use core::time::Duration;
use recon_core::{NodeId, Protocol, Time};
use std::collections::BTreeMap;

/// One thing done to a run from outside it.
///
/// The vocabulary is exactly the simulator's own mutators, so that anything a hand-written test
/// can do to a run is something a description can say. The two opt-ins that are not faults —
/// the codec check and session-event delivery — belong to how the run is *built* rather than to
/// what happens during it, and so live in the constructor a scenario is run with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<C> {
    /// Hand a command to a process.
    Command { node: NodeId, cmd: C },
    /// Crash a process: it loses everything volatile.
    Crash(NodeId),
    /// Restart a crashed process, which takes its startup branch.
    Restart(NodeId),
    /// Suspend a process: it stops, keeping its state and everything addressed to it.
    Suspend(NodeId),
    /// Resume a suspended process.
    Resume(NodeId),
    /// Arm a process's next durable write to be the one it dies inside.
    CrashOnNextWrite(NodeId),
    /// Cut one pair off from each other, leaving every other pair alone.
    Sever(NodeId, NodeId),
    /// Restore one pair, leaving every other severing in place.
    Reconnect(NodeId, NodeId),
    /// Split the network into groups, replacing whatever was severed before.
    Partition(Vec<Vec<NodeId>>),
    /// Restore full connectivity, discarding every severing however it was made.
    Heal,
    /// End the session between a pair, losing an unknown suffix of what was in flight.
    BreakSession(NodeId, NodeId),
}

impl<C> Step<C> {
    /// Every process this step names, in the order it names them.
    pub fn nodes(&self) -> Vec<NodeId> {
        match self {
            Step::Command { node, .. }
            | Step::Crash(node)
            | Step::Restart(node)
            | Step::Suspend(node)
            | Step::Resume(node)
            | Step::CrashOnNextWrite(node) => vec![*node],
            Step::Sever(a, b) | Step::Reconnect(a, b) | Step::BreakSession(a, b) => vec![*a, *b],
            Step::Partition(groups) => groups.iter().flatten().copied().collect(),
            Step::Heal => Vec::new(),
        }
    }

    /// Whether this step mentions `node`.
    pub fn mentions(&self, node: NodeId) -> bool {
        self.nodes().contains(&node)
    }

    fn apply<P>(&self, sim: &mut Sim<P>)
    where
        P: Protocol<Cmd = C>,
        C: Clone,
        P::Cmd: Clone,
        P::Msg: Clone + PartialEq,
        P::Ind: Clone,
        P::Meta: Clone,
        P::Entry: Clone,
    {
        match self {
            Step::Command { node, cmd } => {
                // The identity is for a caller who wants to find the operation again; a scenario
                // step is replayed rather than followed, so it has no use for one.
                let _ = sim.command(*node, cmd.clone());
            }
            Step::Crash(n) => sim.crash(*n),
            Step::Restart(n) => sim.restart(*n),
            Step::Suspend(n) => sim.suspend(*n),
            Step::Resume(n) => sim.resume(*n),
            Step::CrashOnNextWrite(n) => sim.crash_on_next_write(*n),
            Step::Sever(a, b) => sim.sever(*a, *b),
            Step::Reconnect(a, b) => sim.reconnect(*a, *b),
            Step::Partition(groups) => {
                let borrowed: Vec<&[NodeId]> = groups.iter().map(|g| g.as_slice()).collect();
                sim.partition(&borrowed);
            }
            Step::Heal => sim.heal(),
            Step::BreakSession(a, b) => sim.break_session(*a, *b),
        }
    }
}

/// A whole run, as data.
///
/// Executing one twice produces the same trace, by the determinism the simulator already
/// guarantees: the configuration carries the seed, and nothing else in a run is drawn from
/// anywhere but the generator that seed starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario<C> {
    /// Network conditions and the seed. The seed lives here rather than beside it, because it is
    /// what the simulator already treats as part of a configuration.
    pub config: Config,
    /// The processes in the run.
    pub nodes: Vec<NodeId>,
    /// What happens, and when — each time measured from the start of the run, in non-decreasing
    /// order. Two steps at the same time happen in the order given, with nothing dispatched
    /// between them.
    pub steps: Vec<(Duration, Step<C>)>,
    /// How long to run after the last step.
    pub horizon: Duration,
}

impl<C> Scenario<C> {
    /// An empty run over `nodes` with no steps and no horizon.
    pub fn new(config: Config, nodes: impl IntoIterator<Item = NodeId>) -> Self {
        Scenario {
            config,
            nodes: nodes.into_iter().collect(),
            steps: Vec::new(),
            horizon: Duration::ZERO,
        }
    }

    /// Add a step at `at`, measured from the start of the run.
    ///
    /// # Panics
    ///
    /// If `at` precedes the last step already added. A description is executed in the order it
    /// is written, and the clock does not go backwards, so an out-of-order step would silently
    /// happen at the wrong moment rather than where it reads.
    pub fn at(mut self, at: Duration, step: Step<C>) -> Self {
        if let Some((last, _)) = self.steps.last() {
            assert!(at >= *last, "scenario steps must be in non-decreasing time order");
        }
        self.steps.push((at, step));
        self
    }

    /// Run until `horizon` after the start.
    pub fn horizon(mut self, horizon: Duration) -> Self {
        self.horizon = horizon;
        self
    }

    /// The time the run ends: the horizon, or the last step if that is later.
    pub fn end(&self) -> Duration {
        self.steps.last().map(|(at, _)| *at).unwrap_or(Duration::ZERO).max(self.horizon)
    }
}

impl<C: Clone> Scenario<C> {
    /// The same scenario without the steps at `drop`, given as indices into [`Scenario::steps`].
    pub(crate) fn without_steps(&self, drop: &[usize]) -> Self {
        Scenario {
            config: self.config.clone(),
            nodes: self.nodes.clone(),
            steps: self
                .steps
                .iter()
                .enumerate()
                .filter(|(i, _)| !drop.contains(i))
                .map(|(_, s)| s.clone())
                .collect(),
            horizon: self.horizon,
        }
        .repaired()
    }

    /// The same scenario without `node`, and without every step that mentions it.
    ///
    /// A partition step keeps its other groups; a group emptied by the removal goes with it.
    pub(crate) fn without_node(&self, node: NodeId) -> Self {
        let mut steps = Vec::new();
        for (at, step) in &self.steps {
            match step {
                Step::Partition(groups) => {
                    let kept: Vec<Vec<NodeId>> = groups
                        .iter()
                        .map(|g| g.iter().copied().filter(|n| *n != node).collect::<Vec<_>>())
                        .filter(|g| !g.is_empty())
                        .collect();
                    if !kept.is_empty() {
                        steps.push((*at, Step::Partition(kept)));
                    }
                }
                s if s.mentions(node) => {}
                s => steps.push((*at, s.clone())),
            }
        }
        Scenario {
            config: self.config.clone(),
            nodes: self.nodes.iter().copied().filter(|n| *n != node).collect(),
            steps,
            horizon: self.horizon,
        }
        .repaired()
    }

    /// The same scenario run only to `horizon`, dropping any step scheduled after it.
    pub(crate) fn with_horizon(&self, horizon: Duration) -> Self {
        Scenario {
            config: self.config.clone(),
            nodes: self.nodes.clone(),
            steps: self.steps.iter().filter(|(at, _)| *at <= horizon).cloned().collect(),
            horizon,
        }
        .repaired()
    }

    /// The same scenario with step `index` replaced.
    pub(crate) fn with_step(&self, index: usize, step: Step<C>) -> Self {
        let mut out = self.clone();
        out.steps[index].1 = step;
        out.repaired()
    }
}

impl<C: Clone> Scenario<C> {
    /// Drop the steps that a reduction has left dangling.
    ///
    /// A `Resume` belongs to a `Suspend` and a `Restart` to a `Crash`, and the simulator refuses
    /// each without its partner — deliberately, since resuming a crashed process would be a
    /// pause pretending to be a recovery. Deleting steps is exactly what a reduction does, so a
    /// reduction that did not repair the pairing would spend most of its candidates on runs that
    /// panic rather than on runs that answer the question.
    ///
    /// Repairing rather than rejecting: a candidate that has lost a `Suspend` is still a
    /// candidate, it is just one where the process never stopped.
    fn repaired(mut self) -> Self {
        #[derive(PartialEq, Clone, Copy)]
        enum Liveness {
            Running,
            Suspended,
            Crashed,
        }
        let mut state: BTreeMap<NodeId, Liveness> = BTreeMap::new();
        let mut kept = Vec::with_capacity(self.steps.len());
        for (at, step) in self.steps.drain(..) {
            let live = |m: &BTreeMap<NodeId, Liveness>, n: &NodeId| {
                *m.get(n).unwrap_or(&Liveness::Running)
            };
            let keep = match &step {
                Step::Suspend(n) => live(&state, n) == Liveness::Running,
                Step::Resume(n) => live(&state, n) == Liveness::Suspended,
                Step::Restart(n) => live(&state, n) == Liveness::Crashed,
                _ => true,
            };
            if !keep {
                continue;
            }
            match &step {
                Step::Suspend(n) => {
                    state.insert(*n, Liveness::Suspended);
                }
                Step::Resume(n) | Step::Restart(n) => {
                    state.insert(*n, Liveness::Running);
                }
                Step::Crash(n) => {
                    state.insert(*n, Liveness::Crashed);
                }
                _ => {}
            }
            kept.push((at, step));
        }
        self.steps = kept;
        self
    }

    /// Whether every `Resume` has a `Suspend` and every `Restart` a `Crash`, in order.
    ///
    /// A scenario written by hand can be wrong; one produced by a reduction cannot, because every
    /// reduction repairs. Exposed so a test can say which it is holding.
    pub fn is_well_formed(&self) -> bool
    where
        C: PartialEq,
    {
        self.clone().repaired().steps == self.steps
    }
}

impl<P> Sim<P>
where
    P: Protocol,
    P::Cmd: Clone,
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
    P::Meta: Clone,
    P::Entry: Clone,
{
    /// Execute a description, and return the run it produced.
    ///
    /// `build` is handed the scenario's configuration and membership and returns a simulator over
    /// them — normally `Sim::new(config, nodes, make)`, plus whichever opt-ins the protocol needs
    /// (`deliver_session_events`, `enable_codec_check`). It takes both rather than closing over
    /// them because the shrinker will hand it *smaller* ones, and a builder that ignored its
    /// arguments would quietly keep running the original.
    ///
    /// The clock is advanced to each step's time and the step applied, exactly as a hand-written
    /// test would: `run_until(at)` then the call. Two steps at the same time are applied with
    /// nothing dispatched between them, which is what makes "command, then break the session
    /// carrying it" expressible.
    pub fn run_scenario(
        scenario: &Scenario<P::Cmd>,
        build: impl FnOnce(Config, &[NodeId]) -> Sim<P>,
    ) -> Sim<P> {
        let mut sim = build(scenario.config.clone(), &scenario.nodes);
        for (at, step) in &scenario.steps {
            sim.run_until(Time::from_offset(*at));
            step.apply(&mut sim);
        }
        sim.run_until(Time::from_offset(scenario.horizon));
        sim
    }
}

impl<C: core::fmt::Debug> Scenario<C> {
    /// Render as Rust that reconstructs this scenario, as a function named `name`.
    ///
    /// The end of a reduction should be something to paste, not something to transcribe. The
    /// command is rendered with its `Debug`, which is valid Rust for the derived implementations
    /// this repository's commands all use, provided their variants are in scope where the output
    /// is pasted.
    pub fn to_rust(&self, name: &str) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "fn {name}() -> Scenario<Cmd> {{");
        let _ = writeln!(s, "    Scenario {{");
        let _ = writeln!(s, "        config: {},", render_config(&self.config, 8));
        let _ = writeln!(s, "        nodes: {},", render_nodes(&self.nodes));
        if self.steps.is_empty() {
            let _ = writeln!(s, "        steps: vec![],");
        } else {
            let _ = writeln!(s, "        steps: vec![");
            for (at, step) in &self.steps {
                let _ =
                    writeln!(s, "            ({}, {}),", render_duration(*at), render_step(step));
            }
            let _ = writeln!(s, "        ],");
        }
        let _ = writeln!(s, "        horizon: {},", render_duration(self.horizon));
        let _ = writeln!(s, "    }}");
        let _ = writeln!(s, "}}");
        s
    }
}

/// `Duration::from_nanos(n)` — exact for every value the simulator can hold, where
/// `from_millis` would round a latency drawn in nanoseconds into a different run.
fn render_duration(d: Duration) -> String {
    format!("Duration::from_nanos({})", d.as_nanos())
}

fn render_node(n: NodeId) -> String {
    format!("NodeId({})", n.0)
}

fn render_nodes(nodes: &[NodeId]) -> String {
    let inner: Vec<String> = nodes.iter().map(|n| render_node(*n)).collect();
    format!("vec![{}]", inner.join(", "))
}

fn render_step<C: core::fmt::Debug>(step: &Step<C>) -> String {
    match step {
        Step::Command { node, cmd } => {
            format!("Step::Command {{ node: {}, cmd: {cmd:?} }}", render_node(*node))
        }
        Step::Crash(n) => format!("Step::Crash({})", render_node(*n)),
        Step::Restart(n) => format!("Step::Restart({})", render_node(*n)),
        Step::Suspend(n) => format!("Step::Suspend({})", render_node(*n)),
        Step::Resume(n) => format!("Step::Resume({})", render_node(*n)),
        Step::CrashOnNextWrite(n) => format!("Step::CrashOnNextWrite({})", render_node(*n)),
        Step::Sever(a, b) => {
            format!("Step::Sever({}, {})", render_node(*a), render_node(*b))
        }
        Step::Reconnect(a, b) => {
            format!("Step::Reconnect({}, {})", render_node(*a), render_node(*b))
        }
        Step::BreakSession(a, b) => {
            format!("Step::BreakSession({}, {})", render_node(*a), render_node(*b))
        }
        Step::Heal => "Step::Heal".to_string(),
        Step::Partition(groups) => {
            let inner: Vec<String> = groups.iter().map(|g| render_nodes(g)).collect();
            format!("Step::Partition(vec![{}])", inner.join(", "))
        }
    }
}

/// Every field, rather than the builder calls that would have produced them: `synchronous` and
/// `sessions` overwrite the fault knobs, so builder order is not recoverable from a value and a
/// rendering that guessed at it could produce a different run.
fn render_config(c: &Config, indent: usize) -> String {
    let pad = " ".repeat(indent + 4);
    let close = " ".repeat(indent);
    let mut s = String::from("Config {\n");
    let _ = writeln!(s, "{pad}seed: {},", c.seed);
    let _ = writeln!(s, "{pad}loss: {:?},", c.loss);
    let _ = writeln!(s, "{pad}duplication: {:?},", c.duplication);
    let _ = writeln!(s, "{pad}reorder: {:?},", c.reorder);
    let _ = writeln!(s, "{pad}latency_min: {},", render_duration(c.latency_min));
    let _ = writeln!(s, "{pad}latency_max: {},", render_duration(c.latency_max));
    let _ = writeln!(s, "{pad}reorder_delay: {},", render_duration(c.reorder_delay));
    match c.synchronous {
        None => {
            let _ = writeln!(s, "{pad}synchronous: None,");
        }
        Some(d) => {
            let _ = writeln!(s, "{pad}synchronous: Some({}),", render_duration(d));
        }
    }
    let _ = writeln!(s, "{pad}sessions: {},", c.sessions);
    let _ = writeln!(s, "{pad}reconnect_interval: {},", render_duration(c.reconnect_interval));
    let _ = writeln!(s, "{pad}max_steps: {},", c.max_steps);
    let _ = write!(s, "{close}}}");
    s
}
