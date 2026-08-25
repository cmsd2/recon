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

/// A process in the run.
struct Node<P> {
    protocol: P,
    crashed: bool,
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
    pub fn new(config: Config, nodes: &[NodeId], mut make: impl FnMut(NodeId) -> P) -> Self {
        let rng = ChaCha8Rng::seed_from_u64(config.seed);
        let mut map = BTreeMap::new();
        for &id in nodes {
            map.insert(id, Node { protocol: make(id), crashed: false });
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
            trace: Trace::default(),
            effects: Vec::new(),
            codec_check: None,
        }
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

    /// Stop `node` processing events and discard messages addressed to it.
    pub fn crash(&mut self, node: NodeId) {
        if let Some(n) = self.nodes.get_mut(&node) {
            n.crashed = true;
            let at = self.now;
            self.trace.push(TraceEvent::Crashed { at, node });
        }
    }

    /// Resume `node`. Its state is whatever it was when it crashed.
    pub fn restart(&mut self, node: NodeId) {
        if let Some(n) = self.nodes.get_mut(&node) {
            n.crashed = false;
            let at = self.now;
            self.trace.push(TraceEvent::Restarted { at, node });
        }
    }

    pub fn is_crashed(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.crashed).unwrap_or(false)
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

    fn crashed(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.crashed).unwrap_or(true)
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

        if self.config.loss > 0.0 && self.rng.random::<f64>() < self.config.loss {
            self.trace.push(TraceEvent::Dropped { at, from, to, msg, reason: DropReason::Lost });
            return;
        }

        let delay = self.draw_delay(from, to, &msg);
        self.schedule(at + delay, Scheduled::Deliver { from, to, msg: msg.clone() });

        if self.config.duplication > 0.0 && self.rng.random::<f64>() < self.config.duplication {
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
