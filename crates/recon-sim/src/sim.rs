//! The deterministic execution environment.
//!
//! Runs a set of processes in one thread with a virtual clock and a seeded generator. It *is*
//! the fair-loss link layer: messages may be lost, duplicated, delayed and reordered, and the
//! protocols above are responsible for recovering from that.

use crate::config::Config;
use crate::trace::{DropReason, Trace, TraceEvent};
use core::time::Duration;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use recon_core::error::CodecError;
use recon_core::{Cx, Effect, NodeId, Protocol, Time};
use std::collections::{BTreeMap, BTreeSet};

/// Round-trips one message through the wire codec, when codec checking is enabled.
type CodecCheck<M> = fn(&M) -> Result<M, CodecError>;

/// Something scheduled to happen at a point in virtual time.
enum Scheduled<P: Protocol> {
    Deliver { from: NodeId, to: NodeId, msg: P::Msg },
    Timer { node: NodeId, token: P::Timer },
    Command { node: NodeId, cmd: P::Cmd },
}

/// Whether a process is handling events, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Liveness {
    Running,
    /// Stopped with its state preserved — a pause, not a failure.
    Suspended,
    /// Stopped having lost everything volatile.
    Crashed,
}

/// A process in the run.
struct Node<P> {
    protocol: P,
    liveness: Liveness,
}

/// A deterministic run of `P` across several processes.
///
/// Ordering is total and reproducible: the queue is keyed by `(time, sequence)`, so events at
/// the same virtual instant are processed in the order they were scheduled, on every run.
pub struct Sim<P: Protocol> {
    now: Time,
    rng: ChaCha8Rng,
    config: Config,
    seq: u64,
    steps: u64,
    queue: BTreeMap<(Time, u64), Scheduled<P>>,
    nodes: BTreeMap<NodeId, Node<P>>,
    partitions: Option<Vec<BTreeSet<NodeId>>>,
    /// Timers that came due while their process was suspended. A suspension preserves state, and
    /// a pending timer is state, so these are re-armed on resume rather than discarded. A crash
    /// destroys them instead — see `discard_timers_of`.
    deferred: Vec<(NodeId, P::Timer)>,
    /// Rebuilds a process after a crash, since a crash loses volatile state.
    make: Box<dyn FnMut(NodeId) -> P>,
    trace: Trace<P::Msg, P::Ind, P::Timer>,
    effects: Vec<Effect<P::Msg, P::Ind, P::Timer>>,
    codec_check: Option<CodecCheck<P::Msg>>,
}

impl<P> Sim<P>
where
    P: Protocol,
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
    P::Timer: Clone,
{
    /// Build a run over `nodes`, constructing each process with `make`.
    pub fn new(
        config: Config,
        nodes: &[NodeId],
        mut make: impl FnMut(NodeId) -> P + 'static,
    ) -> Self {
        let rng = ChaCha8Rng::seed_from_u64(config.seed);
        let mut map = BTreeMap::new();
        for &id in nodes {
            map.insert(id, Node { protocol: make(id), liveness: Liveness::Running });
        }
        Sim {
            now: Time::ZERO,
            rng,
            config,
            seq: 0,
            steps: 0,
            queue: BTreeMap::new(),
            nodes: map,
            partitions: None,
            deferred: Vec::new(),
            make: Box::new(make),
            trace: Trace::default(),
            effects: Vec::new(),
            codec_check: None,
        }
    }

    /// The upper bound on delivery, when the run is synchronous.
    ///
    /// A protocol whose correctness rests on this bound should be configured from it rather than
    /// from a timeout that happens to work.
    pub fn delivery_bound(&self) -> Option<Duration> {
        self.config.delivery_bound()
    }

    /// The current virtual time.
    pub fn now(&self) -> Time {
        self.now
    }

    /// The record of what has happened so far.
    pub fn trace(&self) -> &Trace<P::Msg, P::Ind, P::Timer> {
        &self.trace
    }

    /// Borrow a process, for inspecting state a trace cannot show.
    pub fn protocol(&self, node: NodeId) -> Option<&P> {
        self.nodes.get(&node).map(|n| &n.protocol)
    }

    /// The processes in this run, in a stable order.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().copied()
    }

    /// Hand `cmd` to `node` at the current time.
    pub fn command(&mut self, node: NodeId, cmd: P::Cmd) {
        self.schedule(self.now, Scheduled::Command { node, cmd });
    }

    /// Hand `cmd` to `node` after `after` has elapsed.
    pub fn command_at(&mut self, node: NodeId, after: Duration, cmd: P::Cmd) {
        self.schedule(self.now + after, Scheduled::Command { node, cmd });
    }

    /// Crash `node`: it stops handling events and loses everything volatile.
    ///
    /// Its protocol state is replaced with a freshly initialised one and its pending timers are
    /// discarded, so a restart resumes having forgotten what it delivered. This is what a real
    /// process gets. For a pause that preserves state, use [`Sim::suspend`].
    pub fn crash(&mut self, node: NodeId) {
        let fresh = (self.make)(node);
        if let Some(n) = self.nodes.get_mut(&node) {
            n.protocol = fresh;
            n.liveness = Liveness::Crashed;
        } else {
            return;
        }
        self.discard_timers_of(node);
        let at = self.now;
        self.trace.push(TraceEvent::Crashed { at, node });
    }

    /// Suspend `node`: it stops handling events but keeps its state and its timers.
    ///
    /// Not what a crash does. Use it to model a process that is merely unreachable or stalled.
    pub fn suspend(&mut self, node: NodeId) {
        if let Some(n) = self.nodes.get_mut(&node) {
            n.liveness = Liveness::Suspended;
            let at = self.now;
            self.trace.push(TraceEvent::Suspended { at, node });
        }
    }

    /// Resume `node`, whether it crashed or was suspended.
    ///
    /// Timers that came due while it was suspended fire now. A crashed process has none, since
    /// the crash discarded them along with the rest of its volatile state.
    pub fn restart(&mut self, node: NodeId) {
        if !self.nodes.contains_key(&node) {
            return;
        }
        self.nodes.get_mut(&node).expect("just checked").liveness = Liveness::Running;
        let at = self.now;
        self.trace.push(TraceEvent::Restarted { at, node });

        let due: Vec<P::Timer> = {
            let mut keep = Vec::new();
            let mut due = Vec::new();
            for (n, token) in self.deferred.drain(..) {
                if n == node {
                    due.push(token);
                } else {
                    keep.push((n, token));
                }
            }
            self.deferred = keep;
            due
        };
        for token in due {
            self.schedule(at, Scheduled::Timer { node, token });
        }
    }

    /// Whether `node` is currently stopped, for either reason.
    pub fn is_stopped(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.liveness != Liveness::Running).unwrap_or(false)
    }

    /// Timers are volatile state, so a crash takes them with it — including any held while the
    /// process was suspended.
    fn discard_timers_of(&mut self, node: NodeId) {
        self.deferred.retain(|(n, _)| *n != node);
        let doomed: Vec<(Time, u64)> = self
            .queue
            .iter()
            .filter(|(_, s)| matches!(s, Scheduled::Timer { node: n, .. } if *n == node))
            .map(|(k, _)| *k)
            .collect();
        for k in doomed {
            self.queue.remove(&k);
        }
    }

    /// Split the network into groups. Messages between groups are not delivered.
    pub fn partition(&mut self, groups: &[&[NodeId]]) {
        self.partitions =
            Some(groups.iter().map(|g| g.iter().copied().collect::<BTreeSet<_>>()).collect());
    }

    /// Remove any partition, restoring full connectivity.
    pub fn heal(&mut self) {
        self.partitions = None;
    }

    /// Process every event scheduled at or before `until`.
    pub fn run_until(&mut self, until: Time) {
        while self.steps < self.config.max_steps {
            let Some((&key, _)) = self.queue.iter().next() else { break };
            if key.0 > until {
                break;
            }
            let item = self.queue.remove(&key).expect("key just observed");
            self.now = key.0;
            self.steps += 1;
            self.dispatch(item);
        }
        if self.now < until {
            self.now = until;
        }
    }

    /// Process every event scheduled within `d` of now.
    pub fn run_for(&mut self, d: Duration) {
        self.run_until(self.now + d);
    }

    // ---------------------------------------------------------------- internals

    fn schedule(&mut self, at: Time, item: Scheduled<P>) {
        let key = (at, self.seq);
        self.seq += 1;
        self.queue.insert(key, item);
    }

    fn dispatch(&mut self, item: Scheduled<P>) {
        match item {
            Scheduled::Command { node, cmd } => {
                if self.crashed(node) {
                    return;
                }
                self.run_handler(node, |p, cx| p.on_cmd(cmd, cx));
            }
            Scheduled::Timer { node, token } => {
                if self.suspended(node) {
                    // Held, not dropped: the process still exists and will want this.
                    self.deferred.push((node, token));
                    return;
                }
                if self.crashed(node) {
                    return;
                }
                let at = self.now;
                self.trace.push(TraceEvent::TimerFired { at, node, token: token.clone() });
                self.run_handler(node, |p, cx| p.on_timer(token, cx));
            }
            Scheduled::Deliver { from, to, msg } => {
                if self.crashed(to) {
                    let at = self.now;
                    self.trace.push(TraceEvent::Dropped {
                        at,
                        from,
                        to,
                        msg,
                        reason: DropReason::RecipientCrashed,
                    });
                    return;
                }
                let msg = match self.check_codec(&msg) {
                    Ok(m) => m,
                    Err(e) => panic!("codec check failed for a message from {from} to {to}: {e}"),
                };
                let at = self.now;
                self.trace.push(TraceEvent::Delivered { at, from, to, msg: msg.clone() });
                self.run_handler(to, |p, cx| p.on_msg(from, msg, cx));
            }
        }
    }

    fn suspended(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.liveness == Liveness::Suspended).unwrap_or(false)
    }

    fn crashed(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.liveness != Liveness::Running).unwrap_or(true)
    }

    /// Run one handler and interpret everything it emits.
    fn run_handler(
        &mut self,
        node: NodeId,
        f: impl FnOnce(&mut P, &mut Cx<'_, P::Msg, P::Ind, P::Timer>),
    ) {
        let mut effects = core::mem::take(&mut self.effects);
        effects.clear();

        {
            let Some(n) = self.nodes.get_mut(&node) else {
                self.effects = effects;
                return;
            };
            let mut cx = Cx::new(&mut effects, self.now, &mut self.rng);
            f(&mut n.protocol, &mut cx);
        }

        for effect in effects.drain(..) {
            match effect {
                Effect::Send { to, msg } => self.transmit(node, to, msg),
                Effect::Indicate(ind) => {
                    let at = self.now;
                    self.trace.push(TraceEvent::Indicated { at, node, ind });
                }
                Effect::SetTimer { after, token } => {
                    self.schedule(self.now + after, Scheduled::Timer { node, token });
                }
            }
        }

        self.effects = effects;
    }

    /// Apply the network model to one outgoing message.
    fn transmit(&mut self, from: NodeId, to: NodeId, msg: P::Msg) {
        let at = self.now;
        self.trace.push(TraceEvent::Sent { at, from, to, msg: msg.clone() });

        if self.crashed(from) {
            self.trace.push(TraceEvent::Dropped {
                at,
                from,
                to,
                msg,
                reason: DropReason::SenderCrashed,
            });
            return;
        }

        if !self.connected(from, to) {
            self.trace.push(TraceEvent::Dropped {
                at,
                from,
                to,
                msg,
                reason: DropReason::Partitioned,
            });
            return;
        }

        let synchronous = self.config.is_synchronous();

        if !synchronous && self.config.loss > 0.0 && self.rng.random::<f64>() < self.config.loss {
            self.trace.push(TraceEvent::Dropped { at, from, to, msg, reason: DropReason::Lost });
            return;
        }

        let delay = self.draw_delay(from, to, &msg);
        self.schedule(at + delay, Scheduled::Deliver { from, to, msg: msg.clone() });

        if !synchronous
            && self.config.duplication > 0.0
            && self.rng.random::<f64>() < self.config.duplication
        {
            self.trace.push(TraceEvent::Duplicated { at, from, to, msg: msg.clone() });
            let second = self.draw_delay(from, to, &msg);
            self.schedule(at + second, Scheduled::Deliver { from, to, msg });
        }
    }

    fn draw_delay(&mut self, from: NodeId, to: NodeId, msg: &P::Msg) -> Duration {
        let lo = self.config.latency_min.as_nanos() as u64;
        let hi = self.config.latency_max.as_nanos() as u64;
        let base = if hi > lo { self.rng.random_range(lo..=hi) } else { lo };

        let mut delay = Duration::from_nanos(base);
        if let Some(bound) = self.config.synchronous {
            // A reordering spike would exceed the bound, which is the one thing this mode
            // promises not to do.
            return delay.min(bound);
        }
        if self.config.reorder > 0.0 && self.rng.random::<f64>() < self.config.reorder {
            let at = self.now;
            self.trace.push(TraceEvent::Reordered { at, from, to, msg: msg.clone() });
            delay += self.config.reorder_delay;
        }
        delay
    }

    fn connected(&self, a: NodeId, b: NodeId) -> bool {
        match &self.partitions {
            None => true,
            Some(groups) => groups.iter().any(|g| g.contains(&a) && g.contains(&b)),
        }
    }

    fn check_codec(&self, msg: &P::Msg) -> Result<P::Msg, CodecError> {
        match self.codec_check {
            None => Ok(msg.clone()),
            Some(f) => f(msg),
        }
    }
}

impl<P> Sim<P>
where
    P: Protocol,
    P::Msg: Clone + PartialEq + serde::Serialize + serde::de::DeserializeOwned,
    P::Ind: Clone,
    P::Timer: Clone,
{
    /// Round-trip every delivered message through the wire codec.
    ///
    /// Off by default: the simulator moves typed values, so a codec defect cannot be mistaken
    /// for a protocol defect. Turn it on to check that messages actually survive encoding,
    /// without paying for it on every run.
    pub fn enable_codec_check(&mut self) {
        self.codec_check = Some(crate::codec::round_trip);
    }
}
