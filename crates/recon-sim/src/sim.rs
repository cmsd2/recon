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
    Time, TimerId, WriteKind,
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
        id: TimerId,
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
    /// Everything that came due for a process while it was suspended, in the order it came due:
    /// timers, deliveries carried by a live session, and scope events.
    ///
    /// A suspension is a stall, not a failure, so nothing addressed to a suspended process is
    /// dropped — dropping a delivery while its session stays up would lose a message with no
    /// `SessionEnded` to say so, which is the one thing `docs/conditional-guarantees.md` forbids
    /// of every layer and therefore of the simulator too. These are re-dispatched by `resume`.
    /// A crash destroys them instead — see `discard_pending_of`.
    deferred: Vec<(NodeId, Scheduled<P>)>,
    /// Rebuilds a process after a crash, since a crash loses volatile state.
    make: Box<dyn FnMut(NodeId) -> P>,
    /// The current epoch of the session between each pair, keyed by the pair in sorted order.
    /// Absent means no session has been established yet.
    sessions: BTreeMap<(NodeId, NodeId), u64>,
    /// The next epoch to hand out for each pair, so epochs increase across re-establishment.
    next_epoch: BTreeMap<(NodeId, NodeId), u64>,
    /// When each pair's last session ended, so that none re-opens in the instant it closed.
    ended_at: BTreeMap<(NodeId, NodeId), Time>,
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
    trace: Trace<P::Msg, P::Ind>,
    /// One source of timer identities for the whole run: two layers of one process must never
    /// be handed the same handle, or each would accept the other's expiry.
    next_timer: u64,
    effects: Vec<ProtoEffect<P>>,
    codec_check: Option<CodecCheck<P::Msg>>,
}

impl<P> Sim<P>
where
    P: Protocol,
    P::Msg: Clone + PartialEq,
    P::Ind: Clone,
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
            next_timer: 0,
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
            ended_at: BTreeMap::new(),
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
    pub fn trace(&self) -> &Trace<P::Msg, P::Ind> {
        &self.trace
    }

    /// Borrow a process, for inspecting state a trace cannot show.
    pub fn protocol(&self, node: NodeId) -> Option<&P> {
        self.nodes.get(&node).map(|n| &n.protocol)
    }

    /// [`Sim::protocol`] for a process the test knows is running. Panics naming the process
    /// otherwise, which is more use in a failure than `unwrap`'s line number.
    pub fn at(&self, node: NodeId) -> &P {
        self.protocol(node).unwrap_or_else(|| panic!("{node} is not running"))
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

    /// Arm the next write by `node` to be the one it dies inside.
    ///
    /// Whether that write landed is decided by the seed, and the process cannot tell: what it
    /// reads on recovering is the only evidence.
    pub fn crash_on_next_write(&mut self, node: NodeId) {
        self.doomed.insert(node);
    }

    /// Crash `node`: it stops handling events and loses everything volatile.
    ///
    /// Its protocol state is replaced with a freshly initialised one, and its pending timers and
    /// anything held for it are discarded, so a restart resumes having forgotten what it
    /// delivered. This is what a real process gets. For a stall that preserves state, use
    /// [`Sim::suspend`].
    pub fn crash(&mut self, node: NodeId) {
        let fresh = (self.make)(node);
        if let Some(n) = self.nodes.get_mut(&node) {
            n.protocol = fresh;
            n.liveness = Liveness::Crashed;
        } else {
            return;
        }
        self.discard_pending_of(node);
        if self.config.is_session_based() {
            self.end_sessions_of(node);
        }
        let at = self.now;
        self.trace.push(TraceEvent::Crashed { at, node });
    }

    /// Suspend `node`: it stops handling events but keeps its state, its timers, and everything
    /// addressed to it while it is away.
    ///
    /// Not what a crash does. This is a *stall* — the process is descheduled and comes back
    /// having missed nothing, because the timers, deliveries and scope events that came due
    /// meanwhile were held rather than dropped. Losing them would be losing a message inside a
    /// session that never ended, which is the one thing this model forbids of a layer.
    ///
    /// For unreachability use [`Sim::partition`], and for failure [`Sim::crash`]. Resume with
    /// [`Sim::resume`]; [`Sim::restart`] is for a crashed process and re-runs its startup branch.
    pub fn suspend(&mut self, node: NodeId) {
        if let Some(n) = self.nodes.get_mut(&node) {
            n.liveness = Liveness::Suspended;
            let at = self.now;
            self.trace.push(TraceEvent::Suspended { at, node });
        }
    }

    /// Resume a *suspended* `node`: everything held while it was away is dispatched now.
    ///
    /// No startup branch runs. Nothing was lost, so there is nothing to recover and nothing to
    /// initialise — replaying `on_init` or `on_recovery` over intact volatile state would be
    /// telling a process it restarted when it did not. That is what separates this from
    /// [`Sim::restart`].
    ///
    /// Note what a resumed process is *not* told: that time passed. Its clock ran while it could
    /// not read it, so anything that measures silence — a failure detector — comes back with
    /// stale evidence and a timer due immediately. That is what a stall does to a real process,
    /// and it is why the synchronous model excludes one.
    pub fn resume(&mut self, node: NodeId) {
        match self.nodes.get_mut(&node) {
            Some(n) if n.liveness == Liveness::Suspended => n.liveness = Liveness::Running,
            Some(_) => panic!("resume({node}): not suspended — restart() is for a crash"),
            None => return,
        }
        let at = self.now;
        self.trace.push(TraceEvent::Resumed { at, node });
        self.release_deferred(node);
    }

    /// Restart a *crashed* `node`, which takes its startup branch.
    ///
    /// A crash discarded its volatile state, its timers, and anything that was on its way, so
    /// there is nothing held to release. What survived is in storage, and is handed back through
    /// `on_recovery`. For a suspension use [`Sim::resume`].
    pub fn restart(&mut self, node: NodeId) {
        match self.nodes.get_mut(&node) {
            Some(n) if n.liveness == Liveness::Crashed => n.liveness = Liveness::Running,
            Some(_) => panic!("restart({node}): not crashed — resume() is for a suspension"),
            None => return,
        }
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
    }

    /// Re-dispatch everything held while `node` was suspended, in the order it came due.
    fn release_deferred(&mut self, node: NodeId) {
        let at = self.now;
        let mut keep = Vec::new();
        let mut due = Vec::new();
        for (n, item) in self.deferred.drain(..) {
            if n == node {
                due.push(item);
            } else {
                keep.push((n, item));
            }
        }
        self.deferred = keep;
        for item in due {
            self.schedule(at, item);
        }
    }

    /// Whether `node` is currently stopped, for either reason.
    pub fn is_stopped(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.liveness != Liveness::Running).unwrap_or(false)
    }

    /// Timers and undelivered messages are lost with the incarnation that was waiting for them,
    /// so a crash takes them — including everything held while the process was suspended.
    fn discard_pending_of(&mut self, node: NodeId) {
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
                if self.stopped(node) {
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
                if self.suspended(node) {
                    // A local notification, not a network message: held for the same reason a
                    // timer is. A stalled process has not stopped existing.
                    self.deferred.push((node, Scheduled::ScopeEvent { node, scope }));
                    return;
                }
                if self.stopped(node) {
                    return;
                }
                self.run_handler(node, |p, cx| p.on_scope_event(scope, cx));
            }
            Scheduled::Timer { node, id } => {
                if self.suspended(node) {
                    // Held, not dropped: the process still exists and will want this.
                    self.deferred.push((node, Scheduled::Timer { node, id }));
                    return;
                }
                if self.stopped(node) {
                    return;
                }
                let at = self.now;
                self.trace.push(TraceEvent::TimerFired { at, node, id });
                self.run_handler(node, |p, cx| p.on_timer(id, cx));
            }
            Scheduled::Deliver { from, to, msg } => {
                if self.suspended(to) {
                    // Held. Dropping it would lose a message inside a session that is still up,
                    // with no `SessionEnded` raised — the model's own invariant, broken by the
                    // model. The recipient sees it when it resumes, as it would after a stall.
                    self.deferred.push((to, Scheduled::Deliver { from, to, msg }));
                    return;
                }
                if self.stopped(to) {
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

    /// Stopped with its state intact — a stall, from which everything held is replayed.
    fn suspended(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.liveness == Liveness::Suspended).unwrap_or(false)
    }

    /// Stopped having lost everything volatile, or never here at all.
    ///
    /// Distinct from [`Sim::stopped`], and the distinction matters: what is owed to a crashed
    /// process is nothing, and what is owed to a suspended one is everything, later.
    fn crashed(&self, node: NodeId) -> bool {
        self.nodes.get(&node).map(|n| n.liveness == Liveness::Crashed).unwrap_or(true)
    }

    /// Not handling events, for either reason.
    fn stopped(&self, node: NodeId) -> bool {
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
            let mut cx =
                Cx::new(&mut effects, self.now, &mut self.rng, &mut store, &mut self.next_timer);
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
                Effect::SetTimer { after, id } => {
                    self.schedule(self.now + after, Scheduled::Timer { node, id });
                }
            }
        }

        self.effects = effects;
    }

    /// Apply the network model to one outgoing message.
    fn transmit(&mut self, from: NodeId, to: NodeId, msg: P::Msg) {
        let at = self.now;
        self.trace.push(TraceEvent::Sent { at, from, to, msg: msg.clone() });

        if self.stopped(from) {
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
        for k in inflight.iter().copied().skip(keep) {
            if let Some(Scheduled::Deliver { from, to, msg }) = self.queue.remove(&k) {
                self.trace.push(TraceEvent::SuffixLost { at, from, to, msg });
            }
        }

        // What survived is flushed now, before the ending is announced. A transport delivers
        // nothing on a connection after it has surfaced the close, and a scope boundary that
        // arrivals can trail is not a boundary: the layer above resends on `Established`, so a
        // straggler from the old epoch would arrive behind the new epoch's traffic under an
        // identifier nothing distinguishes. Re-scheduled in their existing order, so the session
        // is FIFO right up to its last message.
        for k in inflight.into_iter().take(keep) {
            if let Some(item) = self.queue.remove(&k) {
                self.schedule(at, item);
            }
        }

        // Ordering restarts with the next session, and the next one cannot be this instant.
        self.last_delivery.remove(&(key.0, key.1));
        self.last_delivery.remove(&(key.1, key.0));
        self.ended_at.insert(key, at);

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
    ///
    /// A process needs no session with itself, and giving it one would announce `Established`
    /// twice per node — the pair loop visits `(a, b)` and `(b, a)` — so a layer that resends on
    /// re-establishment would resend to itself twice per round.
    fn ensure_session(&mut self, a: NodeId, b: NodeId) -> bool {
        if a == b {
            return true;
        }
        let key = pair(a, b);
        if self.sessions.contains_key(&key) {
            return true;
        }
        // Strictly crashed: a suspended process still exists, and the scope event it is owed is
        // held for it rather than skipped.
        if !self.connected(a, b) || self.crashed(a) || self.crashed(b) {
            return false;
        }
        // Not in the instant the last one ended. Everything that ending owes the two endpoints —
        // the flushed prefix, then `Ended` — is scheduled at that instant, and a successor
        // established among them would reach a layer as `Established` before the `Ended` it
        // replaces. Reconnection takes time; the sweep opens the next one.
        if self.ended_at.get(&key) == Some(&self.now) {
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
    P::Meta: Clone,
    P::Entry: Clone,
    P::Scope: From<SessionEvent>,
{
    /// Deliver session events to the protocol as scope events.
    ///
    /// Opt-in, like the codec check: a protocol that declares no scopes cannot receive one, and
    /// the bound lives only on this method so ordinary runs need nothing.
    ///
    /// **Forgetting this is silent, and it disables everything the session layers do.** Sessions
    /// still open, end and lose their suffixes; no layer is ever told, so every resend clause is
    /// dead, every `[session]` tag is unearned, and nothing fails to say so. A session-based run
    /// of a protocol whose `Scope` is inhabited should call it, and
    /// `forgetting_deliver_session_events_silently_disables_the_whole_bridge` is what that costs.
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
