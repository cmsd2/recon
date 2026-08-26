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
use recon_core::{
    Cx, Effect, MemStore, NodeId, Position, ProtoCx, ProtoEffect, Protocol, SessionEvent, Store,
    Time, WriteKind,
};
use std::collections::{BTreeMap, BTreeSet};

/// Round-trips one message through the wire codec, when codec checking is enabled.
type CodecCheck<M> = fn(&M) -> Result<M, CodecError>;

/// Something scheduled to happen at a point in virtual time.
enum Scheduled<P: Protocol> {
    Deliver {
        from: NodeId,
        to: NodeId,
        msg: P::Msg,
    },
    Timer {
        node: NodeId,
        token: P::Timer,
    },
    Command {
        node: NodeId,
        cmd: P::Cmd,
    },
    ScopeEvent {
        node: NodeId,
        scope: P::Scope,
    },
    /// Retry establishing every session that is not up. A deployed link keeps trying on its own
    /// rather than waiting for the layers above to transmit, so the model does too.
    Reconnect,
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
    /// The current epoch of the session between each pair, keyed by the pair in sorted order.
    /// Absent means no session has been established yet.
    sessions: BTreeMap<(NodeId, NodeId), u64>,
    /// The next epoch to hand out for each pair, so epochs increase across re-establishment.
    next_epoch: BTreeMap<(NodeId, NodeId), u64>,
    /// The last time anything was delivered from one process to another, so that a message is
    /// never delivered before one sent earlier the same way. This is what makes a session FIFO.
    last_delivery: BTreeMap<(NodeId, NodeId), Time>,
    /// Turns a session ending into whatever the protocol calls a scope. Absent unless the
    /// protocol opted in, exactly as the codec check does.
    session_scope: Option<fn(SessionEvent) -> P::Scope>,
    /// What each process can read: everything it has written, durable or not. A protocol reads
    /// its own writes back at once, which is what makes the interface synchronous.
    storage: BTreeMap<NodeId, MemStore<P::Meta, P::Entry>>,
    /// Processes whose next write is fatal — dying mid-`fsync`.
    doomed: BTreeSet<NodeId>,
    trace: Trace<P::Msg, P::Ind, P::Timer>,
    effects: Vec<ProtoEffect<P>>,
    codec_check: Option<CodecCheck<P::Msg>>,
}

impl<P> Sim<P>
where
    P: Protocol,
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
    P::Timer: Clone,
    P::Meta: Clone,
    P::Entry: Clone,
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
        let mut sim = Sim {
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
            sessions: BTreeMap::new(),
            next_epoch: BTreeMap::new(),
            last_delivery: BTreeMap::new(),
            session_scope: None,
            storage: BTreeMap::new(),
            doomed: BTreeSet::new(),
            trace: Trace::default(),
            effects: Vec::new(),
            codec_check: None,
        };
        if sim.config.is_session_based() {
            // The link starts trying immediately and keeps trying, so a session comes up as soon
            // as one is possible rather than when something above happens to transmit.
            sim.schedule(Time::ZERO, Scheduled::Reconnect);
        }
        // A first start, for every process: nothing has been written down, so this is the branch
        // the book takes with ⟨ Init ⟩ rather than ⟨ Recovery ⟩.
        let ids: Vec<NodeId> = sim.nodes.keys().copied().collect();
        for node in ids {
            sim.run_handler(node, |p, cx| p.on_init(cx));
        }
        sim
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

    /// Borrow what a process has written down. `None` if it has written nothing.
    pub fn storage(&self, node: NodeId) -> Option<&MemStore<P::Meta, P::Entry>> {
        self.storage.get(&node)
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
    /// Arm the next write by `node` to be the one it dies inside.
    ///
    /// Whether that write landed is decided by the seed, and the process cannot tell: what it
    /// reads on recovering is the only evidence.
    pub fn crash_on_next_write(&mut self, node: NodeId) {
        self.doomed.insert(node);
    }

    pub fn crash(&mut self, node: NodeId) {
        let fresh = (self.make)(node);
        if let Some(n) = self.nodes.get_mut(&node) {
            n.protocol = fresh;
            n.liveness = Liveness::Crashed;
        } else {
            return;
        }
        self.discard_timers_of(node);
        if self.config.is_session_based() {
            self.end_sessions_of(node);
        }
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

        // What survived, handed back as an event rather than through the constructor: the
        // algorithms that need it re-announce their log and re-send what was pending, and those
        // are effects, which a constructor cannot emit.
        // Exactly one branch, as the book has it: something in storage means recovery, nothing
        // means this incarnation is starting afresh and takes the first-start path instead.
        let survived = self.storage.get(&node).map(|s| !s.is_empty()).unwrap_or(false);
        self.trace.push(TraceEvent::Recovered { at, node, had_state: survived });
        if survived {
            self.run_handler(node, |p, cx| p.on_recovery(cx));
        } else {
            self.run_handler(node, |p, cx| p.on_init(cx));
        }

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
        if self.config.is_session_based() {
            self.end_severed_sessions();
        }
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
            Scheduled::Reconnect => {
                self.reconnect_sweep();
                let at = self.now + self.config.reconnect_interval;
                self.schedule(at, Scheduled::Reconnect);
            }
            Scheduled::ScopeEvent { node, scope } => {
                if self.crashed(node) {
                    return;
                }
                self.run_handler(node, |p, cx| p.on_scope_event(scope, cx));
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
    fn run_handler(&mut self, node: NodeId, f: impl FnOnce(&mut P, &mut ProtoCx<'_, P>)) {
        let mut effects = core::mem::take(&mut self.effects);
        effects.clear();

        // Drawn before the handler runs, so the store need not borrow the generator the context
        // already holds.
        // Armed until a write actually happens: a handler that writes nothing is not the one.
        let doomed = self.doomed.contains(&node);
        let keep = doomed && self.rng.random::<bool>();
        let mut writes: Vec<WriteKind> = Vec::new();
        let mut died = false;

        {
            let Some(n) = self.nodes.get_mut(&node) else {
                self.effects = effects;
                return;
            };
            let inner = self.storage.entry(node).or_default();
            let mut store =
                FaultyStore { inner, writes: &mut writes, doomed, keep, died: &mut died };
            let mut cx = Cx::new(&mut effects, self.now, &mut self.rng, &mut store);
            f(&mut n.protocol, &mut cx);
        }

        let at = self.now;
        if !writes.is_empty() {
            self.doomed.remove(&node);
        }
        for kind in writes {
            self.trace.push(TraceEvent::Wrote { at, node, kind });
        }

        if died {
            // Everything the handler went on to do is discarded — a crash loses volatile state
            // anyway, so nothing decided on the strength of that write can escape.
            effects.clear();
            self.effects = effects;
            self.trace.push(TraceEvent::DiedWriting { at, node });
            self.crash(node);
            return;
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

        if self.config.is_session_based() {
            if !self.ensure_session(from, to) {
                let reason = if self.crashed(to) {
                    DropReason::RecipientCrashed
                } else {
                    DropReason::Partitioned
                };
                self.trace.push(TraceEvent::Dropped { at, from, to, msg, reason });
                return;
            }
            let deliver_at = self.session_delivery_time(from, to);
            self.schedule(deliver_at, Scheduled::Deliver { from, to, msg });
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
    P::Meta: Clone,
    P::Entry: Clone,
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

fn pair(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a <= b { (a, b) } else { (b, a) }
}

impl<P> Sim<P>
where
    P: Protocol,
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
    P::Timer: Clone,
    P::Meta: Clone,
    P::Entry: Clone,
{
    /// The current epoch of the session between `a` and `b`, if one is established.
    pub fn session_epoch(&self, a: NodeId, b: NodeId) -> Option<u64> {
        self.sessions.get(&pair(a, b)).copied()
    }

    /// End the session between `a` and `b`, discarding an unknown suffix of what was in flight.
    ///
    /// A new session opens at a higher epoch the next time either sends and the pair is able to
    /// communicate.
    pub fn break_session(&mut self, a: NodeId, b: NodeId) {
        self.end_session(a, b, DropReason::SessionEnded);
    }

    /// Whether a session currently exists between `a` and `b`.
    pub fn has_session(&self, a: NodeId, b: NodeId) -> bool {
        self.sessions.contains_key(&pair(a, b))
    }

    fn end_session(&mut self, a: NodeId, b: NodeId, reason: DropReason) {
        let key = pair(a, b);
        let Some(epoch) = self.sessions.remove(&key) else {
            return;
        };
        let at = self.now;

        // Discard an unknown suffix of what was in flight, in either direction. The cut is drawn
        // from the run's generator, so it varies with the seed and can be everything or nothing.
        let mut inflight: Vec<(Time, u64)> = self
            .queue
            .iter()
            .filter(|(_, s)| {
                matches!(s, Scheduled::Deliver { from, to, .. } if pair(*from, *to) == key)
            })
            .map(|(k, _)| *k)
            .collect();
        inflight.sort();
        let keep = self.rng.random_range(0..=inflight.len());
        for k in inflight.into_iter().skip(keep) {
            if let Some(Scheduled::Deliver { from, to, msg }) = self.queue.remove(&k) {
                self.trace.push(TraceEvent::SuffixLost { at, from, to, msg });
            }
        }

        // Ordering restarts with the next session.
        self.last_delivery.remove(&(key.0, key.1));
        self.last_delivery.remove(&(key.1, key.0));

        self.trace.push(TraceEvent::SessionEnded { at, a: key.0, b: key.1, epoch, reason });

        // Both endpoints are told, if they are alive to hear it. The epoch named is the one that
        // ended: at the moment of failure the next is not a fact, and may never become one.
        if let Some(f) = self.session_scope {
            for (node, peer) in [(key.0, key.1), (key.1, key.0)] {
                if !self.crashed(node) {
                    let scope = f(SessionEvent::Ended { peer, epoch });
                    self.schedule(at, Scheduled::ScopeEvent { node, scope });
                }
            }
        }
    }

    /// End every session involving `node`.
    fn end_sessions_of(&mut self, node: NodeId) {
        let peers: Vec<NodeId> = self
            .sessions
            .keys()
            .filter_map(|(a, b)| {
                if *a == node {
                    Some(*b)
                } else if *b == node {
                    Some(*a)
                } else {
                    None
                }
            })
            .collect();
        for peer in peers {
            self.end_session(node, peer, DropReason::SessionEnded);
        }
    }

    /// End every session that the current partitioning has severed.
    fn end_severed_sessions(&mut self) {
        let severed: Vec<(NodeId, NodeId)> =
            self.sessions.keys().copied().filter(|(a, b)| !self.connected(*a, *b)).collect();
        for (a, b) in severed {
            self.end_session(a, b, DropReason::Partitioned);
        }
    }

    /// Establish a session if the pair can communicate and none exists.
    fn ensure_session(&mut self, a: NodeId, b: NodeId) -> bool {
        let key = pair(a, b);
        if self.sessions.contains_key(&key) {
            return true;
        }
        if !self.connected(a, b) || self.crashed(a) || self.crashed(b) {
            return false;
        }
        // Epochs are per pair and only ever increase, so a re-established session is
        // distinguishable from the one it replaces.
        let epoch = self.next_epoch.get(&key).copied().unwrap_or(1);
        self.next_epoch.insert(key, epoch + 1);
        self.sessions.insert(key, epoch);
        let at = self.now;
        self.trace.push(TraceEvent::SessionOpened { at, a: key.0, b: key.1, epoch });

        // Both endpoints are told. This is the actionable event: the peer is reachable, so
        // anything sent in response arrives.
        if let Some(f) = self.session_scope {
            for (node, peer) in [(key.0, key.1), (key.1, key.0)] {
                if !self.crashed(node) {
                    let scope = f(SessionEvent::Established { peer, epoch });
                    self.schedule(at, Scheduled::ScopeEvent { node, scope });
                }
            }
        }
        true
    }

    /// Try to establish every session that is not up.
    fn reconnect_sweep(&mut self) {
        let nodes: Vec<NodeId> = self.nodes.keys().copied().collect();
        for (i, a) in nodes.iter().enumerate() {
            for b in nodes.iter().skip(i + 1) {
                self.ensure_session(*a, *b);
            }
        }
    }

    /// Delivery time under a session: never before something sent earlier the same way.
    fn session_delivery_time(&mut self, from: NodeId, to: NodeId) -> Time {
        let lo = self.config.latency_min.as_nanos() as u64;
        let hi = self.config.latency_max.as_nanos() as u64;
        let base = if hi > lo { self.rng.random_range(lo..=hi) } else { lo };
        let earliest = self.now + Duration::from_nanos(base);

        let key = (from, to);
        let at = match self.last_delivery.get(&key) {
            Some(prev) if *prev >= earliest => *prev + Duration::from_nanos(1),
            _ => earliest,
        };
        self.last_delivery.insert(key, at);
        at
    }
}

impl<P> Sim<P>
where
    P: Protocol,
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
    P::Timer: Clone,
    P::Meta: Clone,
    P::Entry: Clone,
    P::Scope: From<SessionEvent>,
{
    /// Deliver session events to the protocol as scope events.
    ///
    /// Opt-in, like the codec check: a protocol that declares no scopes cannot receive one, and
    /// the bound lives only on this method so ordinary runs need nothing.
    pub fn deliver_session_events(&mut self) {
        self.session_scope = Some(P::Scope::from);
    }
}

/// The store a protocol writes through: records what happened, and can kill the process.
///
/// When `doomed`, the first write applies or does not by a coin drawn before the handler ran, and
/// the process is then killed.
struct FaultyStore<'a, Me, En> {
    inner: &'a mut MemStore<Me, En>,
    writes: &'a mut Vec<WriteKind>,
    doomed: bool,
    keep: bool,
    died: &'a mut bool,
}

impl<Me, En> FaultyStore<'_, Me, En> {
    /// Whether the write takes effect. Recorded either way: it was attempted.
    fn allow(&mut self, kind: WriteKind) -> bool {
        self.writes.push(kind);
        if !self.doomed {
            return true;
        }
        if *self.died {
            // The process is already gone; the handler is still running only because a synchronous
            // call cannot be interrupted. Nothing more it does reaches the disk.
            return false;
        }
        *self.died = true;
        self.keep
    }
}

impl<Me, En> Store<Me, En> for FaultyStore<'_, Me, En> {
    fn get(&self) -> Option<&Me> {
        self.inner.get()
    }

    fn set(&mut self, meta: Me) {
        if self.allow(WriteKind::Set) {
            self.inner.set(meta);
        }
    }

    fn append(&mut self, entry: En) -> Position {
        if self.allow(WriteKind::Append) {
            return self.inner.append(entry);
        }
        self.inner.end()
    }

    fn read_from(&self, from: Position) -> Vec<&En> {
        self.inner.read_from(from)
    }

    fn end(&self) -> Position {
        self.inner.end()
    }
}
