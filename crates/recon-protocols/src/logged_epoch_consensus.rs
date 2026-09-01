//! Read/write epoch consensus that survives a restart.
//!
//! **Status: implementation. Space: bounded by membership, plus what the stubborn children hold
//! outstanding — which nothing here retires. See the departure on `Stop`.**
//!
//! Cachin, Guerraoui & Rodrigues, Module 5.7 (`LoggedEpochConsensus`) and Algorithm 5.9 ("Logged
//! Read/Write Epoch Consensus"), quoted from the book:
//!
//! ```text
//! Algorithm 5.9: Logged Read/Write Epoch Consensus
//! Implements: EpochConsensus, instance lep, with timestamp ets and leader ℓ.
//! Uses:
//!     StubbornPointToPointLinks, instance sl;
//!     StubbornBestEffortBroadcast, instance sbeb;
//!
//! upon event ⟨ lep, Init | state ⟩ do
//!     (valts, val) := state;
//!     store(valts, val);
//!     tmpval := ⊥;
//!     states := [⊥]^N;
//!     accepted := 0;
//!
//! upon event ⟨ lep, Recovery ⟩ do
//!     retrieve(valts, val);
//!
//! upon event ⟨ lep, Propose | v ⟩ do                       // only leader ℓ
//!     tmpval := v;
//!     trigger ⟨ sbeb, Broadcast | [READ] ⟩;
//!
//! upon event ⟨ sbeb, Deliver | ℓ, [READ] ⟩ do
//!     trigger ⟨ sl, Send | ℓ, [STATE, valts, val] ⟩;
//!
//! upon event ⟨ sl, Deliver | q, [STATE, ts, v] ⟩ do        // only leader ℓ
//!     states[q] := (ts, v);
//!
//! upon #(states) > N/2 do                                  // only leader ℓ
//!     (ts, v) := highest(states);
//!     if v ≠ ⊥ then tmpval := v;
//!     states := [⊥]^N;
//!     trigger ⟨ sbeb, Broadcast | [WRITE, tmpval] ⟩;
//!
//! upon event ⟨ sbeb, Deliver | ℓ, [WRITE, v] ⟩ do
//!     (valts, val) := (ets, v);
//!     store(valts, val);
//!     trigger ⟨ sl, Send | ℓ, [ACCEPT] ⟩;
//!
//! upon event ⟨ sl, Deliver | q, [ACCEPT] ⟩ do              // only leader ℓ
//!     accepted := accepted + 1;
//!
//! upon accepted > N/2 do                                   // only leader ℓ
//!     accepted := 0;
//!     trigger ⟨ sbeb, Broadcast | [DECIDED, tmpval] ⟩;
//!
//! upon event ⟨ sbeb, Deliver | ℓ, [DECIDED, v] ⟩ do
//!     epochdecision := v;
//!     store(epochdecision);
//!     trigger ⟨ lep, Decide | epochdecision ⟩;
//!
//! upon event ⟨ lep, Abort ⟩ do
//!     trigger ⟨ lep, Aborted | (valts, val) ⟩;
//!     halt;                                                // stop operating when aborted
//! ```
//!
//! The safety argument is [`crate::epoch_consensus`]'s and is not restated here: two majorities
//! intersect, so a later epoch's read reaches a process that accepted whatever an earlier epoch
//! decided, and `if v ≠ ⊥ then tmpval := v` makes the later leader adopt it. What this module adds
//! is that the argument still holds when the processes holding that intersection go down and come
//! back.
//!
//! # Two `store` calls, one metadata value
//!
//! The book writes `store(valts, val)` and `store(epochdecision)` as separate calls. `Cx::storage`
//! offers one rewritten metadata value and an appended sequence, so both land in one [`Durable`]
//! that is rewritten each time. Nothing accumulates: an epoch accepts at most one value and decides
//! at most one, so the record is a fixed size and `Entry` is uninhabited.
//!
//! # Durable before visible, twice, and both in the handler's own text
//!
//! `store(valts, val); trigger ⟨ sl, Send | ℓ, [ACCEPT] ⟩` — the acceptance is a **promise to a
//! quorum**. A process that told the leader it had accepted `v` at `ets`, and then came back with
//! no record of it, would answer a later epoch's read with an empty state; the later leader would
//! find nothing in the intersection and be free to write something else, after `v` had already been
//! decided. That is `EPC4` failing, and it fails silently.
//!
//! `store(epochdecision); trigger ⟨ lep, Decide | v ⟩` — same shape one step later. The layer above
//! reads `epochdecision` back on recovery ([`LoggedEpochConsensus::epoch_decision`]) and that is how
//! Algorithm 5.10 knows a process had decided before it went down.
//!
//! Both orders are written here, in these handlers, and not left to a driver to arrange by
//! buffering effects until the handler returns. `Cx` supports eager sinks, so buffering is not
//! something this code may assume.
//!
//! # Departure: repeats are idempotent, and the standing conditions fire once
//!
//! [`crate::epoch_consensus`] runs over perfect links, which deliver each message once. This one
//! runs over stubborn ones, which must not deduplicate — repeating for ever is what reaches a
//! process that was down when the message was sent. So every handler here sees its message many
//! times, and the book's counters do not survive that:
//!
//! - `accepted := accepted + 1` counts *messages*, and one process's ACCEPT arrives for ever. The
//!   count would pass `N/2` on its own with a single acceptance in the whole run. It is a set of
//!   the processes that have accepted, so a repeat adds nothing.
//! - `upon #(states) > N/2` and `upon accepted > N/2` are standing conditions the book re-arms by
//!   clearing what they count. Clearing is not enough when the messages come back: `written` and
//!   `announced` make each fire once, as they already do in [`crate::epoch_consensus`].
//! - `store` is a rewrite of the same value on a repeat, which is idempotent, but it is still a
//!   write. `WRITE` is applied only when it changes something, so the write count stays one per
//!   acceptance and a test can check that rather than take it on trust.
//! - **A follower answers `READ` once and `WRITE` once.** The book answers every delivery. Over a
//!   stubborn link the answer is itself retransmitted until this instance ends, so a second answer
//!   to a redelivered `READ` is a second stubborn transmission carrying the same content — and
//!   since redeliveries never stop, neither would the transmissions. Measured before this guard: the
//!   send rate grew linearly in time, 12.6k → 76.6k per 400 ms across five windows, with nothing
//!   faulty. One answer is enough for the same reason retransmission exists at all: a leader that
//!   crashed and came back re-proposes, and what reaches its new incarnation is the follower's
//!   *original* reply, still going. A follower that crashes forgets it answered and answers again,
//!   which is correct — its link forgot the transmission too.
//!
//! # Departure: messages carry the epoch they belong to
//!
//! As in [`crate::epoch_consensus`]: instances are addressed `lep.ets` in the book and by nothing at
//! all on a real wire, so a `WRITE` from epoch 7 arriving after epoch 11 began would be accepted and
//! recorded at timestamp 11 — an acceptance that never happened. The stamp is in [`Tagged`], inside
//! the instance, because the epoch is the instance's own identity.
//!
//! # Departure: nothing calls `Stop`
//!
//! As in [`crate::logged_epoch_change`]. The stubborn children retransmit until retired and nothing
//! retires them, so space grows with the number of distinct messages an epoch sends rather than
//! with the membership. Bounded in practice by the epoch ending, which is what `Abort` is for.
//!
//! ```text
//! EPC1 [always]  Validity — a decided value was proposed in this epoch, or was the highest-
//!                timestamped value some process had already accepted
//! EPC2 [always]  Uniform agreement — no two processes decide differently in one epoch
//! EPC3 [always]  Integrity — a process decides at most once
//! EPC4 [always]  Lock-in — a value decided in an earlier epoch is what a later one decides, and
//!                **this holds across a crash**: what a process accepted is read back on recovery
//! EPC5 [always]  Abort behaviour — an abandoned instance reports its state and then is silent
//! ```

use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::stubborn_broadcast::{self as sbeb, BroadcastId, StubbornBroadcast};
use crate::stubborn_link::{self as sl, SendId, StubbornLink};

/// `(valts, val)` — what a process has accepted, and when.
///
/// `val` is `None` for the book's `⊥`: nothing accepted yet, at timestamp zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State<V> {
    pub valts: u64,
    pub val: Option<V>,
}

impl<V> Default for State<V> {
    fn default() -> Self {
        State { valts: 0, val: None }
    }
}

/// Everything this instance keeps durably, as one rewritten value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Durable<V> {
    /// `(valts, val)`.
    pub state: State<V>,
    /// `epochdecision`, once this epoch has decided.
    pub decision: Option<V>,
}

/// What travels by `sbeb` — the leader speaking to everyone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Announce<V> {
    /// `[READ]`.
    Read,
    /// `[WRITE, v]`.
    Write { val: V },
    /// `[DECIDED, v]`.
    Decided { val: V },
}

/// What travels by `sl` — a follower answering the leader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply<V> {
    /// `[STATE, valts, val]`.
    StateIs { valts: u64, val: Option<V> },
    /// `[ACCEPT]`.
    Accept,
}

/// A message stamped with the epoch it belongs to.
///
/// The stamp lives here rather than in the layer above because the epoch is this instance's own
/// identity — it stamps what it sends and drops what is not addressed to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tagged<M> {
    pub ets: u64,
    pub msg: M,
}

/// The wire, multiplexing the two children the book names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<V> {
    /// `sbeb` — the leader's announcements.
    Announce(Tagged<Announce<V>>),
    /// `sl` — the followers' replies, each to one process.
    Reply(Tagged<Reply<V>>),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<V> {
    /// `⟨ lep, Propose | v ⟩`. Acted on only by this epoch's leader.
    Propose(V),
    /// `⟨ lep, Abort ⟩`.
    Abort,
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<V> {
    /// `⟨ lep, Decide | v ⟩`. Raised only after the decision is durable.
    Decide(V),
    /// `⟨ lep, Aborted | (valts, val) ⟩` — the state the replacement instance begins from.
    Aborted(State<V>),
}

/// Abortable consensus within one epoch, whose acceptances survive a restart.
#[derive(Debug)]
pub struct LoggedEpochConsensus<V: Clone> {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    /// `ets` — this instance's epoch timestamp.
    ets: u64,
    /// `ℓ` — this epoch's leader.
    leader: NodeId,
    /// `(valts, val)` and `epochdecision` — durable, and mirrored here.
    durable: Durable<V>,
    /// `tmpval` — the value the leader is trying to write. Volatile, as in the book.
    tmpval: Option<V>,
    /// `states` — what the leader has read back, by process. A map, so a repeat replaces.
    states: BTreeMap<NodeId, State<V>>,
    /// `accepted` — **which** processes have acknowledged, not how many messages said so.
    accepted: BTreeSet<NodeId>,
    /// Whether the write has been sent, so a repeat cannot resend it.
    written: bool,
    /// Whether the decision has been announced, so a repeat cannot re-announce it.
    announced: bool,
    /// Whether the decision has been reported upward, so a repeated `DECIDED` decides once.
    decided: bool,
    /// Whether this follower has answered the leader's `READ`. One stubborn reply is enough.
    state_sent: bool,
    /// Whether this follower has answered the leader's `WRITE`. Likewise.
    accept_sent: bool,
    /// Test-only: answer *every* redelivery, which is what this module did before the two flags
    /// above were added. See [`LoggedEpochConsensus::with_reply_per_redelivery_defect`].
    reply_per_redelivery: bool,
    /// `halt`. Every handler returns immediately once this is set.
    aborted: bool,
    /// Names the next stubborn transmission. Volatile, and so is what it keys.
    next_send: u64,
    /// Names the next stubborn broadcast. Volatile, and so is what it keys.
    next_broadcast: u64,
    sbeb: Child<StubbornBroadcast<Tagged<Announce<V>>>>,
    sl: Child<StubbornLink<Tagged<Reply<V>>>>,
}

impl<V: Clone> LoggedEpochConsensus<V> {
    /// `⟨ lep, Init | state ⟩` — an instance for epoch `ets` led by `leader`, beginning from
    /// `state`.
    ///
    /// The book's `store(valts, val)` in `Init` happens on the first event this instance handles,
    /// because a constructor has no context to write through. [`Protocol::on_init`] is where it
    /// lands, and it lands before anything is sent.
    pub fn new(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        ets: u64,
        leader: NodeId,
        state: State<V>,
        retransmit: core::time::Duration,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        LoggedEpochConsensus {
            me,
            peers: peers.clone(),
            ets,
            leader,
            durable: Durable { state, decision: None },
            tmpval: None,
            states: BTreeMap::new(),
            accepted: BTreeSet::new(),
            written: false,
            announced: false,
            decided: false,
            state_sent: false,
            accept_sent: false,
            reply_per_redelivery: false,
            aborted: false,
            next_send: 0,
            next_broadcast: 0,
            sbeb: Child::new(StubbornBroadcast::new(me, peers.clone(), retransmit)),
            sl: Child::new(StubbornLink::new(retransmit)),
        }
    }

    /// `ets`.
    pub fn timestamp(&self) -> u64 {
        self.ets
    }

    /// Whether this instance has been abandoned.
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    /// **Put a fixed defect back.** Answer every redelivered `READ` and `WRITE` on a fresh
    /// stubborn transmission, as this module did before `state_sent` and `accept_sent` existed.
    ///
    /// The consequence is not a wrong decision: it is work that grows with how long the run has
    /// been going rather than with membership — 12.6k, 28.6k, 44.6k, 60.6k, 76.6k sends in
    /// successive 400 ms windows, because each answer joins a stubborn set that is never emptied.
    /// `the_send_rate_does_not_grow_after_the_epoch_has_decided` is the test that holds it fixed.
    ///
    /// It exists so that the scenario shrinker can be demonstrated against a defect this project
    /// actually had rather than against a toy; `shrinking_a_real_defect.rs` is the only caller, and
    /// a test asserts that reintroducing it does break the bound. Nothing else may call it: a
    /// process built this way violates the module's own stated space bound on purpose.
    #[doc(hidden)]
    pub fn with_reply_per_redelivery_defect(mut self) -> Self {
        self.reply_per_redelivery = true;
        self
    }

    /// `(valts, val)` — what this process has accepted.
    pub fn state(&self) -> &State<V> {
        &self.durable.state
    }

    /// `epochdecision` — what this epoch decided, if this process saw it decide.
    ///
    /// Read by the layer above after a recovery: `retrieve(epochdecision) of instance lep.ets` is
    /// how Algorithm 5.10 learns that a process had decided before it went down.
    pub fn epoch_decision(&self) -> Option<&V> {
        self.durable.decision.as_ref()
    }

    /// `N/2` — the threshold both majorities are measured against.
    fn majority(&self) -> usize {
        self.peers.len() / 2
    }

    fn is_leader(&self) -> bool {
        self.me == self.leader
    }

    fn broadcast(&mut self, msg: Announce<V>, cx: &mut ProtoCx<'_, Self>) {
        let msg = Tagged { ets: self.ets, msg };
        let id = BroadcastId(self.next_broadcast);
        self.next_broadcast += 1;
        self.through_sbeb(cx, |b, ccx| b.on_cmd(sbeb::Cmd::Broadcast { id, msg }, ccx));
    }

    fn send_to(&mut self, to: NodeId, msg: Reply<V>, cx: &mut ProtoCx<'_, Self>) {
        let msg = Tagged { ets: self.ets, msg };
        let id = SendId(self.next_send);
        self.next_send += 1;
        self.through_sl(cx, |l, ccx| l.on_cmd(sl::Cmd::Send { id, to, msg }, ccx));
    }

    /// `highest(states)` — the state with the greatest timestamp among those read.
    fn highest(&self) -> Option<State<V>> {
        self.states.values().max_by_key(|s| s.valts).cloned()
    }

    fn on_announce(&mut self, from: NodeId, msg: Announce<V>, cx: &mut ProtoCx<'_, Self>) {
        if from != self.leader {
            return;
        }
        match msg {
            // `upon event ⟨ sbeb, Deliver | ℓ, [READ] ⟩`. Idempotent: the reply says what this
            // process has accepted, which a repeat does not change.
            Announce::Read => {
                if self.state_sent && !self.reply_per_redelivery {
                    return;
                }
                self.state_sent = true;
                let reply = Reply::StateIs {
                    valts: self.durable.state.valts,
                    val: self.durable.state.val.clone(),
                };
                self.send_to(from, reply, cx);
            }
            // `upon event ⟨ sbeb, Deliver | ℓ, [WRITE, v] ⟩ do (valts, val) := (ets, v);
            //  store(valts, val); trigger ⟨ sl, Send | ℓ, [ACCEPT] ⟩`
            //
            // **The write precedes the acceptance, here in this handler.** The ACCEPT is a promise
            // to a quorum, and a promise with no record behind it is how `EPC4` fails silently.
            Announce::Write { val } => {
                if self.accept_sent && !self.reply_per_redelivery {
                    return;
                }
                if self.durable.state.valts != self.ets {
                    self.durable.state = State { valts: self.ets, val: Some(val) };
                    cx.storage().set(self.durable.clone());
                }
                self.accept_sent = true;
                self.send_to(from, Reply::Accept, cx);
            }
            // `upon event ⟨ sbeb, Deliver | ℓ, [DECIDED, v] ⟩ do epochdecision := v;
            //  store(epochdecision); trigger ⟨ lep, Decide | epochdecision ⟩`
            Announce::Decided { val } => {
                if self.decided {
                    return;
                }
                self.decided = true;
                self.durable.decision = Some(val.clone());
                cx.storage().set(self.durable.clone());
                cx.indicate(Ind::Decide(val));
            }
        }
    }

    fn on_reply(&mut self, from: NodeId, msg: Reply<V>, cx: &mut ProtoCx<'_, Self>) {
        if !self.is_leader() {
            return;
        }
        match msg {
            // `upon event ⟨ sl, Deliver | q, [STATE, ts, v] ⟩ do states[q] := (ts, v)`
            Reply::StateIs { valts, val } => {
                self.states.insert(from, State { valts, val });
                self.maybe_write(cx);
            }
            // `upon event ⟨ sl, Deliver | q, [ACCEPT] ⟩ do accepted := accepted + 1`, counted by
            // process rather than by message. See the module's note on repeats.
            Reply::Accept => {
                self.accepted.insert(from);
                self.maybe_decide(cx);
            }
        }
    }

    /// `upon #(states) > N/2 do … trigger ⟨ sbeb, Broadcast | [WRITE, tmpval] ⟩`.
    fn maybe_write(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.written || self.states.len() <= self.majority() {
            return;
        }
        // `(ts, v) := highest(states); if v ≠ ⊥ then tmpval := v;` — the line the whole algorithm
        // turns on, and the one that makes a later epoch adopt what an earlier one may have decided.
        if let Some(highest) = self.highest()
            && highest.val.is_some()
        {
            self.tmpval = highest.val;
        }
        self.states.clear();
        self.written = true;
        if let Some(val) = self.tmpval.clone() {
            self.broadcast(Announce::Write { val }, cx);
        }
    }

    /// `upon accepted > N/2 do … trigger ⟨ sbeb, Broadcast | [DECIDED, tmpval] ⟩`.
    fn maybe_decide(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.announced || self.accepted.len() <= self.majority() {
            return;
        }
        self.accepted.clear();
        self.announced = true;
        if let Some(val) = self.tmpval.clone() {
            self.broadcast(Announce::Decided { val }, cx);
        }
    }

    fn through_sbeb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut StubbornBroadcast<Tagged<Announce<V>>>,
            &mut ProtoCx<'_, StubbornBroadcast<Tagged<Announce<V>>>>,
        ),
    ) {
        let mut inds = self.sbeb.run(cx, Wire::Announce, f);
        for sbeb::Ind::Deliver { from, msg } in inds.drain(..) {
            self.on_announce(from, msg.msg, cx);
        }
        self.sbeb.reclaim(inds);
    }

    fn through_sl(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut StubbornLink<Tagged<Reply<V>>>,
            &mut ProtoCx<'_, StubbornLink<Tagged<Reply<V>>>>,
        ),
    ) {
        let mut inds = self.sl.run(cx, Wire::Reply, f);
        for sl::Ind::Deliver { from, msg } in inds.drain(..) {
            self.on_reply(from, msg.msg, cx);
        }
        self.sl.reclaim(inds);
    }
}

impl<V: Clone> Protocol for LoggedEpochConsensus<V> {
    type Cmd = Cmd<V>;
    type Ind = Ind<V>;
    type Msg = Wire<V>;
    type Scope = core::convert::Infallible;
    type Meta = Durable<V>;
    /// An epoch accepts at most one value and decides at most one. Nothing accumulates.
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        if self.aborted {
            return;
        }
        match cmd {
            // `upon event ⟨ lep, Propose | v ⟩ do tmpval := v; … // only leader ℓ`
            Cmd::Propose(v) => {
                if self.is_leader() {
                    self.tmpval = Some(v);
                    self.broadcast(Announce::Read, cx);
                }
            }
            // `upon event ⟨ lep, Abort ⟩ do trigger ⟨ lep, Aborted | (valts, val) ⟩; halt;`
            Cmd::Abort => {
                self.aborted = true;
                cx.indicate(Ind::Aborted(self.durable.state.clone()));
            }
        }
    }

    /// `such that ts = ets`, applied at the door.
    ///
    /// Unlike [`crate::epoch_consensus`], the link beneath keeps no duplicate set for a foreign
    /// message to poison — it deduplicates nothing at all. The guard is here for the safety reason
    /// alone: an acceptance recorded at the wrong timestamp is an acceptance that never happened.
    fn on_msg(&mut self, from: NodeId, msg: Wire<V>, cx: &mut ProtoCx<'_, Self>) {
        if self.aborted {
            return;
        }
        match msg {
            Wire::Announce(m) if m.ets == self.ets => {
                self.through_sbeb(cx, |b, ccx| b.on_msg(from, m, ccx))
            }
            Wire::Reply(m) if m.ets == self.ets => {
                self.through_sl(cx, |l, ccx| l.on_msg(from, m, ccx))
            }
            _ => {}
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        if self.aborted {
            return;
        }
        self.through_sbeb(cx, |b, ccx| b.on_timer(id, ccx));
        self.through_sl(cx, |l, ccx| l.on_timer(id, ccx));
    }

    /// `upon event ⟨ lep, Init | state ⟩ do (valts, val) := state; store(valts, val); …`
    ///
    /// The state came in through the constructor; this is where it becomes durable, before this
    /// instance answers anything.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        cx.storage().set(self.durable.clone());
    }

    /// `upon event ⟨ lep, Recovery ⟩ do retrieve(valts, val)`.
    ///
    /// `epochdecision` comes back with it, because they share one metadata value. Nothing is
    /// re-indicated: a process that decided before it went down told the layer above at the time,
    /// and it is that layer's own record — not a second `Decide` from here — that restores it. See
    /// [`LoggedEpochConsensus::epoch_decision`].
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if let Some(durable) = cx.storage().get().cloned() {
            self.decided = durable.decision.is_some();
            self.durable = durable;
        }
    }
}
