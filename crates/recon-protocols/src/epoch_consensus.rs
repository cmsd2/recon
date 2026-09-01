//! Read/write epoch consensus — the quorum core, and where Paxos's safety argument lives.
//!
//! **Status: implementation. Space: bounded by membership.**
//!
//! Cachin, Guerraoui & Rodrigues, Module 5.6 and Algorithm 5.6 ("Read/Write Epoch Consensus"),
//! quoted from the book:
//!
//! ```text
//! Algorithm 5.6: Read/Write Epoch Consensus
//! Implements: EpochConsensus, instance ep, with timestamp ets and leader ℓ.
//! Uses:
//!     PerfectPointToPointLinks, instance pl;
//!     BestEffortBroadcast, instance beb.
//!
//! upon event ⟨ ep, Init | state ⟩ do
//!     (valts, val) := state; tmpval := ⊥; states := [⊥]^N; accepted := 0;
//!
//! upon event ⟨ ep, Propose | v ⟩ do                       // only leader ℓ
//!     tmpval := v;
//!     trigger ⟨ beb, Broadcast | [READ] ⟩;
//!
//! upon event ⟨ beb, Deliver | ℓ, [READ] ⟩ do
//!     trigger ⟨ pl, Send | ℓ, [STATE, valts, val] ⟩;
//!
//! upon event ⟨ pl, Deliver | q, [STATE, ts, v] ⟩ do       // only leader ℓ
//!     states[q] := (ts, v);
//!
//! upon #(states) > N/2 do                                 // only leader ℓ
//!     (ts, v) := highest(states);
//!     if v ≠ ⊥ then tmpval := v;
//!     states := [⊥]^N;
//!     trigger ⟨ beb, Broadcast | [WRITE, tmpval] ⟩;
//!
//! upon event ⟨ beb, Deliver | ℓ, [WRITE, v] ⟩ do
//!     (valts, val) := (ets, v);
//!     trigger ⟨ pl, Send | ℓ, [ACCEPT] ⟩;
//!
//! upon event ⟨ pl, Deliver | q, [ACCEPT] ⟩ do             // only leader ℓ
//!     accepted := accepted + 1;
//!
//! upon accepted > N/2 do                                  // only leader ℓ
//!     accepted := 0;
//!     trigger ⟨ beb, Broadcast | [DECIDED, tmpval] ⟩;
//!
//! upon event ⟨ beb, Deliver | ℓ, [DECIDED, v] ⟩ do
//!     trigger ⟨ ep, Decide | v ⟩;
//!
//! upon event ⟨ ep, Abort ⟩ do
//!     trigger ⟨ ep, Aborted | (valts, val) ⟩;
//!     halt;                                               // stop operating when aborted
//! ```
//!
//! # Why two majorities are the whole algorithm
//!
//! The leader reads from a majority and writes to a majority, and any two majorities of `Π`
//! intersect. So if some epoch decided `v` — meaning a majority accepted it — then every later
//! epoch's read reaches at least one process that accepted `v`, and `highest(states)` returns it.
//! `if v ≠ ⊥ then tmpval := v` is the line that makes the later leader adopt it instead of its own
//! proposal. That is the entire reason two epochs cannot decide differently, and every other part
//! of Paxos exists to arrange for it.
//!
//! **`highest` means highest *timestamp*, not highest value.** It picks the state written in the
//! most recent epoch, which is the one that may already have been decided.
//!
//! # `halt` is a safety property, not tidiness
//!
//! `upon event ⟨ ep, Abort ⟩ … halt;  // stop operating when aborted`. An instance that kept
//! answering after being abandoned would be a second leader for its epoch under another name: it
//! could still collect a quorum and decide, while the epoch that replaced it decided something
//! else. [`EpochConsensus::aborted`] is checked at the top of every handler, and the suite delivers
//! a message to an aborted instance and asserts that nothing at all comes out.
//!
//! # Departure: directed replies travel by directed broadcast
//!
//! As in [`crate::epoch_change`], the book's `pl` child is absorbed into the broadcast's directed
//! [`beb::Cmd::SendTo`] — one addressed process, one wire message, which is what `pl, Send` does.
//! `STATE` and `ACCEPT` both travel that way. One child and one wire variant fewer, and the
//! guarantee is unchanged.
//!
//! ```text
//! EPC1 [always]  Validity — a decided value was proposed in this epoch, or was the highest-
//!                timestamped value some process had already accepted
//! EPC2 [always]  Uniform agreement — no two processes decide differently in one epoch
//! EPC3 [always]  Integrity — a process decides at most once
//! EPC4 [always]  Lock-in — a value decided in an earlier epoch is what a later one decides
//! EPC5 [always]  Abort behaviour — an abandoned instance reports its state and then is silent
//! ```

use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use crate::perfect_link as pl;

/// `(valts, val)` — what a process has accepted, and when.
///
/// `val` is `None` for the book's `⊥`: nothing accepted yet, at timestamp zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State<V> {
    pub valts: u64,
    pub val: Option<V>,
}

impl<V> Default for State<V> {
    /// The state a first epoch begins from: nothing accepted.
    fn default() -> Self {
        State { valts: 0, val: None }
    }
}

/// What this layer puts on the wire, beneath the broadcast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpochMsg<V> {
    /// `[READ]` — the leader asking what everyone holds.
    Read,
    /// `[STATE, valts, val]` — a follower's answer, addressed to the leader.
    StateIs { valts: u64, val: Option<V> },
    /// `[WRITE, v]` — the leader telling everyone what to accept.
    Write { val: V },
    /// `[ACCEPT]` — a follower's acknowledgement, addressed to the leader.
    Accept,
    /// `[DECIDED, v]` — the leader announcing the decision.
    Decided { val: V },
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<V> {
    /// `⟨ ep, Propose | v ⟩`. Acted on only by this epoch's leader.
    Propose(V),
    /// `⟨ ep, Abort ⟩`. Answered by [`Ind::Aborted`], after which the instance is silent.
    Abort,
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<V> {
    /// `⟨ ep, Decide | v ⟩`.
    Decide(V),
    /// `⟨ ep, Aborted | (valts, val) ⟩` — the state this instance held when it was abandoned.
    Aborted(State<V>),
}

/// What the broadcast beneath puts on the wire for this layer's messages.
pub type BebMsg<V> = pl::Wire<EpochMsg<V>>;

/// Abortable consensus within one epoch.
#[derive(Debug)]
pub struct EpochConsensus<V> {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    /// `ets` — this instance's epoch timestamp.
    ets: u64,
    /// `ℓ` — this epoch's leader.
    leader: NodeId,
    /// `(valts, val)`.
    state: State<V>,
    /// `tmpval` — the value the leader is trying to write.
    tmpval: Option<V>,
    /// `states` — what the leader has read back, by process.
    states: BTreeMap<NodeId, State<V>>,
    /// `accepted` — how many have acknowledged the write.
    accepted: usize,
    /// Whether the write has already been sent, so a second majority of `STATE` cannot resend it.
    written: bool,
    /// Whether the decision has been announced, so a second majority of `ACCEPT` cannot re-announce.
    announced: bool,
    /// `halt`. Every handler returns immediately once this is set.
    aborted: bool,
    beb: BestEffortBroadcast<EpochMsg<V>>,
    inbox: Vec<beb::Ind<EpochMsg<V>>>,
}

impl<V: Clone> EpochConsensus<V> {
    /// `⟨ ep, Init | state ⟩` — an instance for epoch `ets` led by `leader`, beginning from `state`.
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
        EpochConsensus {
            me,
            peers: peers.clone(),
            ets,
            leader,
            state,
            tmpval: None,
            states: BTreeMap::new(),
            accepted: 0,
            written: false,
            announced: false,
            aborted: false,
            beb: BestEffortBroadcast::new(me, peers, retransmit),
            inbox: Vec::new(),
        }
    }

    /// This epoch's timestamp.
    pub fn timestamp(&self) -> u64 {
        self.ets
    }

    /// Whether this instance has been abandoned and is therefore silent.
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    /// What this process has accepted, and when.
    pub fn state(&self) -> &State<V> {
        &self.state
    }

    /// `N/2` — the threshold both majorities are measured against.
    fn majority(&self) -> usize {
        self.peers.len() / 2
    }

    fn is_leader(&self) -> bool {
        self.me == self.leader
    }

    fn broadcast(&mut self, msg: EpochMsg<V>, cx: &mut ProtoCx<'_, Self>) {
        self.through_beb(cx, |b, ccx| b.on_cmd(beb::Cmd::Broadcast(msg), ccx));
    }

    fn send_to(&mut self, to: NodeId, msg: EpochMsg<V>, cx: &mut ProtoCx<'_, Self>) {
        self.through_beb(cx, |b, ccx| b.on_cmd(beb::Cmd::SendTo { to, msg }, ccx));
    }

    /// `highest(states)` — the state with the greatest timestamp among those read.
    fn highest(&self) -> Option<State<V>> {
        self.states.values().max_by_key(|s| s.valts).cloned()
    }

    fn on_epoch_msg(&mut self, from: NodeId, msg: EpochMsg<V>, cx: &mut ProtoCx<'_, Self>) {
        // `halt` — an abandoned instance answers nothing, which is what stops it from becoming a
        // second leader for an epoch that has moved on.
        if self.aborted {
            return;
        }
        match msg {
            // `upon event ⟨ beb, Deliver | ℓ, [READ] ⟩`
            EpochMsg::Read if from == self.leader => {
                let reply =
                    EpochMsg::StateIs { valts: self.state.valts, val: self.state.val.clone() };
                self.send_to(from, reply, cx);
            }
            // `upon event ⟨ pl, Deliver | q, [STATE, ts, v] ⟩`  // only leader
            EpochMsg::StateIs { valts, val } if self.is_leader() => {
                self.states.insert(from, State { valts, val });
                self.maybe_write(cx);
            }
            // `upon event ⟨ beb, Deliver | ℓ, [WRITE, v] ⟩`
            EpochMsg::Write { val } if from == self.leader => {
                self.state = State { valts: self.ets, val: Some(val) };
                self.send_to(from, EpochMsg::Accept, cx);
            }
            // `upon event ⟨ pl, Deliver | q, [ACCEPT] ⟩`  // only leader
            EpochMsg::Accept if self.is_leader() => {
                self.accepted += 1;
                self.maybe_decide(cx);
            }
            // `upon event ⟨ beb, Deliver | ℓ, [DECIDED, v] ⟩`
            EpochMsg::Decided { val } if from == self.leader => {
                cx.indicate(Ind::Decide(val));
            }
            // A message from someone who is not this epoch's leader, or a leader-only message at a
            // follower. Neither is addressed to this process's role; the book's guards drop them.
            _ => {}
        }
    }

    /// `upon #(states) > N/2 do … trigger ⟨ beb, Broadcast | [WRITE, tmpval] ⟩`.
    fn maybe_write(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.written || self.states.len() <= self.majority() {
            return;
        }
        // `(ts, v) := highest(states); if v ≠ ⊥ then tmpval := v;`
        //
        // This is the line the whole algorithm turns on. A value already accepted in a higher
        // epoch displaces this leader's own proposal, which is what stops two epochs deciding
        // differently. The book prints `≠`; an OCR of the page renders it as `=`, which would
        // invert the meaning and break safety outright.
        if let Some(highest) = self.highest()
            && highest.val.is_some()
        {
            self.tmpval = highest.val;
        }
        self.states.clear();
        self.written = true;
        if let Some(val) = self.tmpval.clone() {
            self.broadcast(EpochMsg::Write { val }, cx);
        }
    }

    /// `upon accepted > N/2 do … trigger ⟨ beb, Broadcast | [DECIDED, tmpval] ⟩`.
    fn maybe_decide(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.announced || self.accepted <= self.majority() {
            return;
        }
        self.accepted = 0;
        self.announced = true;
        if let Some(val) = self.tmpval.clone() {
            self.broadcast(EpochMsg::Decided { val }, cx);
        }
    }

    fn through_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut BestEffortBroadcast<EpochMsg<V>>,
            &mut ProtoCx<'_, BestEffortBroadcast<EpochMsg<V>>>,
        ),
    ) {
        let mut inbox = core::mem::take(&mut self.inbox);
        {
            let b = &mut self.beb;
            cx.with_child_consuming(core::convert::identity, &mut inbox, |ccx| f(b, ccx));
        }
        for ind in inbox.drain(..) {
            match ind {
                beb::Ind::Deliver { from, msg } => self.on_epoch_msg(from, msg, cx),
                beb::Ind::SessionEnded { .. } | beb::Ind::SessionEstablished { .. } => {}
            }
        }
        self.inbox = inbox;
    }
}

impl<V: Clone> Protocol for EpochConsensus<V> {
    type Cmd = Cmd<V>;
    type Ind = Ind<V>;
    type Msg = BebMsg<V>;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably. `logged_epoch_consensus` is the variant that does.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        if self.aborted {
            return;
        }
        match cmd {
            // `upon event ⟨ ep, Propose | v ⟩ do tmpval := v; … // only leader ℓ`
            Cmd::Propose(v) => {
                if self.is_leader() {
                    self.tmpval = Some(v);
                    self.broadcast(EpochMsg::Read, cx);
                }
            }
            // `upon event ⟨ ep, Abort ⟩ do trigger ⟨ ep, Aborted | (valts, val) ⟩; halt;`
            Cmd::Abort => {
                self.aborted = true;
                cx.indicate(Ind::Aborted(self.state.clone()));
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: BebMsg<V>, cx: &mut ProtoCx<'_, Self>) {
        if self.aborted {
            return;
        }
        self.through_beb(cx, |b, ccx| b.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        if self.aborted {
            return;
        }
        self.through_beb(cx, |b, ccx| b.on_timer(id, ccx));
    }
}
