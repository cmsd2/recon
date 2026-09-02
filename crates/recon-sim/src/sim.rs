//! The deterministic execution environment.
//!
//! Runs a set of processes in one thread with a virtual clock and a seeded generator. It *is*
//! the fair-loss link layer: messages may be lost, duplicated, delayed and reordered, and the
//! protocols above are responsible for recovering from that.

use crate::config::Config;
use crate::narrate::{Render, render};
use crate::trace::{DropReason, NotBegun, OpId, ProtoTrace, ProtoTraceEvent, Trace, TraceEvent};
use core::time::Duration;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use recon_core::error::CodecError;
use recon_core::{
    Cx, Effect, MemStore, NoNotes, NodeId, NoteSink, Position, ProtoCx, ProtoEffect, Protocol,
    SessionEvent, Store, Time, TimerId, WriteKind,
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
        /// Names this operation in the trace. Carried so that whether it was handled or discarded,
        /// the record says which operation it was.
        op: OpId,
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
    /// Which pairs cannot reach each other, normalised so `(a, b)` and `(b, a)` are one entry.
    ///
    /// A set of pairs rather than a grouping, because a grouping makes reachability an equivalence
    /// relation and real networks do not: `A` may reach `B` and `B` reach `C` while `A` cannot reach
    /// `C`. [`Sim::partition`] is the special case in which the severed pairs happen to be exactly
    /// those spanning two groups.
    severed: BTreeSet<(NodeId, NodeId)>,
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
    trace: ProtoTrace<P>,
    /// One source of operation identities for the whole run, as `next_timer` is for timers.
    next_op: u64,
    /// What the current handler narrated, flushed into the trace when it returns. Empty, and never
    /// written to, unless the run was asked to record notes.
    notes: Vec<P::Note>,
    /// Whether anything is listening. Off by default, so an ordinary run pays nothing.
    record_notes: bool,
    /// One source of timer identities for the whole run: two layers of one process must never
    /// be handed the same handle, or each would accept the other's expiry.
    next_timer: u64,
    effects: Vec<ProtoEffect<P>>,
    codec_check: Option<CodecCheck<P::Msg>>,
    /// Renders each event as it is recorded. Absent unless the run was asked for it, exactly as
    /// the codec check is.
    render: Option<Render<P>>,
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
            severed: BTreeSet::new(),
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
            next_op: 0,
            notes: Vec::new(),
            record_notes: false,
            effects: Vec::new(),
            codec_check: None,
            render: None,
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
    pub fn trace(&self) -> &ProtoTrace<P> {
        &self.trace
    }

    /// Record what protocols narrate, so the trace holds their decisions beside what happened.
    ///
    /// Off by default: a run pays nothing for an audience it does not have, and the protocol's own
    /// code is the same either way — it calls `Cx::note` regardless, and the sink discards. That is
    /// what makes narrating unable to change the run.
    pub fn record_notes(&mut self) {
        self.record_notes = true;
    }

    /// Record one event: render it if anything is listening, then keep it.
    ///
    /// Rendered *as* it is recorded rather than by a later walk over the trace, because a run that
    /// fails to terminate is one of the things worth reading.
    fn record(&mut self, event: ProtoTraceEvent<P>) {
        if let Some(render) = self.render {
            render(&event);
        }
        self.trace.push(event);
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

    /// Hand `cmd` to `node` at the current time, and take an identity naming that operation.
    ///
    /// The identity is a return value rather than a parameter, so a caller with no interest in it
    /// carries on as before. It names the operation in the trace: [`Trace::invoked_at`] says when
    /// the process handled it, and [`Trace::why_not_begun`] says why it never did.
    pub fn command(&mut self, node: NodeId, cmd: P::Cmd) -> OpId {
        self.command_at(node, Duration::ZERO, cmd)
    }

    /// Hand `cmd` to `node` after `after` has elapsed, and take an identity naming that operation.
    pub fn command_at(&mut self, node: NodeId, after: Duration, cmd: P::Cmd) -> OpId {
        let op = OpId(self.next_op);
        self.next_op += 1;
        self.schedule(self.now + after, Scheduled::Command { node, cmd, op });
        op
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
        self.record(TraceEvent::Crashed { at, node });
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
            self.record(TraceEvent::Suspended { at, node });
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
        self.record(TraceEvent::Resumed { at, node });
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
        self.record(TraceEvent::Restarted { at, node });

        // What survived, handed back as an event rather than through the constructor: the
        // algorithms that need it re-announce their log and re-send what was pending, and those
        // are effects, which a constructor cannot emit.
        // Exactly one branch, as the book has it: something in storage means recovery, nothing
        // means this incarnation is starting afresh and takes the first-start path instead.
        let survived = self.storage.get(&node).map(|s| !s.is_empty()).unwrap_or(false);
        self.record(TraceEvent::Recovered { at, node, had_state: survived });
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
    ///
    /// The special case of [`Sim::sever`] in which the severed pairs are exactly those spanning two
    /// groups — so reachability *is* transitive here, and a process in a group reaches every other
    /// member. That is the easy case, and it is not the only one; see `sever`.
    ///
    /// Replaces whatever was severed before, so calling this after `sever` discards that severing.
    pub fn partition(&mut self, groups: &[&[NodeId]]) {
        let groups: Vec<BTreeSet<NodeId>> =
            groups.iter().map(|g| g.iter().copied().collect()).collect();
        let members: Vec<NodeId> = groups.iter().flatten().copied().collect();
        self.severed.clear();
        for (i, a) in members.iter().enumerate() {
            for b in members.iter().skip(i + 1) {
                if !groups.iter().any(|g| g.contains(a) && g.contains(b)) {
                    self.severed.insert(pair(*a, *b));
                }
            }
        }
        if self.config.is_session_based() {
            self.end_severed_sessions();
        }
    }

    /// Cut `a` and `b` off from each other, in both directions, leaving every other pair alone.
    ///
    /// This is what a grouping cannot express. Severing one pair of three processes leaves a
    /// **bridge**: `A` reaches `B` and `B` reaches `C`, but `A` does not reach `C`. All three are
    /// correct, none of them is wrong about what it can see, and there is no group any of them
    /// belongs to — which is the case every layer above that depends on processes agreeing about
    /// who is reachable has never been asked about.
    ///
    /// Severing is symmetric. A link that works one way and not the other is a different fault and
    /// a harder question for the session model, which treats a session as a property of a pair; see
    /// this change's `design.md`.
    pub fn sever(&mut self, a: NodeId, b: NodeId) {
        if a == b {
            return;
        }
        self.severed.insert(pair(a, b));
        if self.config.is_session_based() {
            self.end_severed_sessions();
        }
    }

    /// Restore connectivity between `a` and `b`, leaving every other severing in place.
    pub fn reconnect(&mut self, a: NodeId, b: NodeId) {
        self.severed.remove(&pair(a, b));
    }

    /// Whether `a` and `b` can currently reach each other.
    ///
    /// For asserting the topology a test built rather than assuming it: severing one pair of *four*
    /// processes is not a bridge, and a test that thinks it is would be testing nothing.
    pub fn reachable(&self, a: NodeId, b: NodeId) -> bool {
        self.connected(a, b)
    }

    /// Restore full connectivity, discarding every severing however it was made.
    pub fn heal(&mut self) {
        self.severed.clear();
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
    /// Dispatch everything scheduled for the current instant, and nothing later. The clock does not
    /// move.
    ///
    /// For sequencing a test by *events* rather than by durations: a command is scheduled, not run,
    /// so `command(...)` followed by `break_session(...)` breaks a session with nothing in flight.
    /// `step_now()` between them runs the handler, whose sends then sit in the queue at their
    /// latency — in flight, and the break finds them. The older idiom, `run_for(1 ms)` with a
    /// comment, depends on the latency being longer than the millisecond.
    pub fn step_now(&mut self) {
        let now = self.now;
        while self.steps < self.config.max_steps {
            let Some((&key, _)) = self.queue.iter().next() else { break };
            if key.0 > now {
                break;
            }
            let item = self.queue.remove(&key).expect("key just observed");
            self.steps += 1;
            self.dispatch(item);
        }
    }

    /// Dispatch the next scheduled event, moving the clock to it. `false` when there is nothing
    /// left to dispatch, or the step budget is spent.
    ///
    /// For a test searching for a state that one event creates and the next may destroy — "exactly
    /// one process has decided" — stepping by event cannot skip it, where `run_for(1 ms)` can when
    /// two events fall inside the millisecond.
    pub fn step(&mut self) -> bool {
        if self.steps >= self.config.max_steps {
            return false;
        }
        let Some((&key, _)) = self.queue.iter().next() else { return false };
        let item = self.queue.remove(&key).expect("key just observed");
        self.now = key.0;
        self.steps += 1;
        self.dispatch(item);
        true
    }

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
            Scheduled::Command { node, cmd, op } => {
                // Discarded rather than held when the process is not running, and recorded either
                // way. A command is a call from the layer above, on this process — a stalled
                // process's layer above is stalled with it, so there is nothing to delay. What was
                // wrong before was the silence, not the discarding.
                if let Some(why) = self.why_not_begun(node) {
                    let at = self.now;
                    self.record(TraceEvent::NotInvoked { at, node, op, cmd, why });
                    return;
                }
                let at = self.now;
                self.record(TraceEvent::Invoked { at, node, op, cmd: cmd.clone() });
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
                self.record(TraceEvent::TimerFired { at, node, id });
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
                    self.record(TraceEvent::Dropped {
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
                self.record(TraceEvent::Delivered { at, from, to, msg: msg.clone() });
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
    /// Why an operation given to `node` cannot begin, or `None` if it can.
    fn why_not_begun(&self, node: NodeId) -> Option<NotBegun> {
        match self.nodes.get(&node) {
            None => Some(NotBegun::NotAProcess),
            Some(n) => match n.liveness {
                Liveness::Running => None,
                Liveness::Suspended => Some(NotBegun::Stalled),
                Liveness::Crashed => Some(NotBegun::Crashed),
            },
        }
    }

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
        let mut notes = core::mem::take(&mut self.notes);
        notes.clear();

        {
            let Some(n) = self.nodes.get_mut(&node) else {
                self.effects = effects;
                self.notes = notes;
                return;
            };
            let inner = self.storage.entry(node).or_default();
            let mut store =
                FaultyStore { inner, writes: &mut writes, doomed, keep, died: &mut died };
            // The protocol's code is the same either way — it calls `cx.note` regardless — which is
            // what makes narrating unable to change the run.
            let mut discard = NoNotes;
            let listener: &mut dyn NoteSink<P::Note> =
                if self.record_notes { &mut notes } else { &mut discard };
            let mut cx = Cx::new(
                &mut effects,
                self.now,
                &mut self.rng,
                &mut store,
                &mut self.next_timer,
                listener,
            );
            f(&mut n.protocol, &mut cx);
        }

        let at = self.now;
        // Before the writes and the effects: a note marks the decision, and those are what it led
        // to. A handler that narrated and then wrote reads in that order.
        for note in notes.drain(..) {
            self.record(TraceEvent::Said { at, node, note });
        }
        self.notes = notes;
        if !writes.is_empty() {
            self.doomed.remove(&node);
        }
        for kind in writes {
            self.record(TraceEvent::Wrote { at, node, kind });
        }

        if died {
            // Everything the handler went on to do is discarded — a crash loses volatile state
            // anyway, so nothing decided on the strength of that write can escape.
            effects.clear();
            self.effects = effects;
            self.record(TraceEvent::DiedWriting { at, node });
            self.crash(node);
            return;
        }

        for effect in effects.drain(..) {
            match effect {
                Effect::Send { to, msg } => self.transmit(node, to, msg),
                Effect::Indicate(ind) => {
                    let at = self.now;
                    self.record(TraceEvent::Indicated { at, node, ind });
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
        self.record(TraceEvent::Sent { at, from, to, msg: msg.clone() });

        if self.stopped(from) {
            self.record(TraceEvent::Dropped {
                at,
                from,
                to,
                msg,
                reason: DropReason::SenderCrashed,
            });
            return;
        }

        if !self.connected(from, to) {
            self.record(TraceEvent::Dropped { at, from, to, msg, reason: DropReason::Partitioned });
            return;
        }

        if self.config.is_session_based() {
            if !self.ensure_session(from, to) {
                // `connected` was checked above, so a refusal here is either a crashed recipient
                // or the instant of an ending — not a partition, and the trace must not say so.
                let reason = if self.crashed(to) {
                    DropReason::RecipientCrashed
                } else {
                    DropReason::NoSession
                };
                self.record(TraceEvent::Dropped { at, from, to, msg, reason });
                return;
            }
            let deliver_at = self.session_delivery_time(from, to);
            self.schedule(deliver_at, Scheduled::Deliver { from, to, msg });
            return;
        }

        let synchronous = self.config.is_synchronous();

        if !synchronous && self.config.loss > 0.0 && self.rng.random::<f64>() < self.config.loss {
            self.record(TraceEvent::Dropped { at, from, to, msg, reason: DropReason::Lost });
            return;
        }

        let delay = self.draw_delay(from, to, &msg);
        self.schedule(at + delay, Scheduled::Deliver { from, to, msg: msg.clone() });

        if !synchronous
            && self.config.duplication > 0.0
            && self.rng.random::<f64>() < self.config.duplication
        {
            self.record(TraceEvent::Duplicated { at, from, to, msg: msg.clone() });
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
            self.record(TraceEvent::Reordered { at, from, to, msg: msg.clone() });
            delay += self.config.reorder_delay;
        }
        delay
    }

    fn connected(&self, a: NodeId, b: NodeId) -> bool {
        a == b || !self.severed.contains(&pair(a, b))
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
    P::Cmd: Clone,
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

impl<P> Sim<P>
where
    P: Protocol,
    P::Msg: Clone + PartialEq + core::fmt::Debug,
    P::Ind: Clone + core::fmt::Debug,
    P::Note: core::fmt::Debug,
    P::Cmd: Clone + core::fmt::Debug,
    P::Meta: Clone,
    P::Entry: Clone,
{
    /// Emit every recorded event to whatever `tracing` subscriber is installed, as it is recorded.
    ///
    /// Off by default, like the codec check: a run pays nothing for an audience it does not have.
    /// Turning it on does not change the run — the events are the same ones the trace already
    /// holds, in the same order, and nothing a protocol can observe is affected.
    ///
    /// Pair it with [`Sim::record_notes`] to see what the protocols *said* as well as what happened
    /// to them; without it the rendering shows the run, which is what the trace showed before this
    /// existed.
    pub fn enable_tracing(&mut self) {
        self.render = Some(render::<P>);
    }
}

fn pair(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a <= b { (a, b) } else { (b, a) }
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
                self.record(TraceEvent::SuffixLost { at, from, to, msg });
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

        self.record(TraceEvent::SessionEnded { at, a: key.0, b: key.1, epoch, reason });

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
        self.record(TraceEvent::SessionOpened { at, a: key.0, b: key.1, epoch });

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
    P::Cmd: Clone,
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
