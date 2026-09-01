//! Leader-driven consensus — Paxos.
//!
//! **Status: implementation. Space: bounded by membership.**
//!
//! Cachin, Guerraoui & Rodrigues, Algorithm 5.7 ("Leader-Driven Consensus"), quoted from the book:
//!
//! ```text
//! Algorithm 5.7: Leader-Driven Consensus
//! Implements: UniformConsensus, instance uc.
//! Uses:
//!     EpochChange, instance ec;
//!     EpochConsensus (multiple instances).
//!
//! upon event ⟨ uc, Init ⟩ do
//!     val := ⊥; proposed := FALSE; decided := FALSE;
//!     Obtain the leader ℓ0 of the initial epoch with timestamp 0 from epoch-change instance ec;
//!     Initialize a new instance ep.0 of epoch consensus with timestamp 0, leader ℓ0, state (0, ⊥);
//!     (ets, ℓ) := (0, ℓ0);
//!     (newts, newℓ) := (0, ⊥);
//!
//! upon event ⟨ uc, Propose | v ⟩ do
//!     val := v;
//!
//! upon event ⟨ ec, StartEpoch | newts′, newℓ′ ⟩ do
//!     (newts, newℓ) := (newts′, newℓ′);
//!     trigger ⟨ ep.ets, Abort ⟩;
//!
//! upon event ⟨ ep.ts, Aborted | state ⟩ such that ts = ets do
//!     (ets, ℓ) := (newts, newℓ);
//!     proposed := FALSE;
//!     Initialize a new instance ep.ets of epoch consensus with timestamp ets, leader ℓ, and
//!         state state;
//!
//! upon ℓ = self ∧ val ≠ ⊥ ∧ proposed = FALSE do
//!     proposed := TRUE;
//!     trigger ⟨ ep.ets, Propose | val ⟩;
//!
//! upon event ⟨ ep.ts, Decide | v ⟩ such that ts = ets do
//!     if decided = FALSE then
//!         decided := TRUE;
//!         trigger ⟨ uc, Decide | v ⟩;
//! ```
//!
//! # The child is replaced while running, and the state is what carries across
//!
//! Every other layer in this repository constructs its children once. This one does not: each epoch
//! gets a **new** epoch-consensus instance, seeded with the state the previous one returned when it
//! was aborted. `CLAUDE.md`'s rule still holds — the field is one concrete type, replaced, not a map
//! from timestamp to instance resolved while running — but it is the first layer here whose child is
//! rebuilt at all.
//!
//! **The abort handshake is asynchronous and the wait is load-bearing.** `Abort` is a request;
//! `Aborted` is its answer, carrying `(valts, val)`. The replacement is not constructed until that
//! answer arrives, because the state it carries is what stops the new epoch contradicting the old
//! one. Aborting and immediately replacing — which is the obvious implementation — loses it, and
//! with it the property the whole algorithm exists to have.
//!
//! # Addition: epoch-consensus traffic is tagged with its epoch
//!
//! The book writes `ep.ts` and `such that ts = ets`, so instances are addressed by timestamp and a
//! message for one never reaches another. Nothing in this codebase's wire does that for free, and
//! the consequence of omitting it is a **safety** failure rather than a lost message: a `WRITE` from
//! epoch 7 arriving after epoch 11 has begun would be accepted and recorded at timestamp 11,
//! inventing an acceptance that never happened.
//!
//! The stamp lives in [`ep::Tagged`], inside the child, rather than in this layer's wire. The epoch
//! is the instance's own identity, so the instance stamps what it sends and drops what is not
//! addressed to it — and this layer cannot forget to. An earlier draft put it here, and the type
//! system objected for an unrelated reason, which turned out to be pointing at the better place.
//!
//! ```text
//! UC1 [always]      Validity — a decided value was proposed by some process
//! UC2 [always]      Uniform agreement — no two processes decide differently, **including while the
//!                   leader detector is wrong and two processes each believe they lead**
//! UC3 [always]      Integrity — a process decides at most once
//! UC4 [conditional] Termination — every correct process decides, provided a majority is correct
//!                   and the leader detector eventually settles
//! ```
//!
//! `UC4` is conditional on both, and saying so is the point. A majority that never forms, or a
//! detector that never settles, leaves this waiting — which is what FLP requires of it and what
//! `flooding_consensus` pretends away by assuming a perfect detector.

use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::epoch_change::{self as ec, EpochChange};
use crate::epoch_consensus::{self as ep, EpochConsensus, State};

/// The wire, multiplexing the epoch-change child and whichever epoch-consensus instance is live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<E, C> {
    /// The epoch-change child's traffic.
    Change(E),
    /// The live epoch-consensus instance's traffic.
    ///
    /// The epoch tag that makes `ep.ts` addressable lives inside `C` rather than here — see
    /// [`ep::Tagged`]. It belongs to the instance, so the instance stamps it and drops what is not
    /// its own, and a parent cannot forget to.
    Consensus(C),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<V> {
    /// `⟨ uc, Propose | v ⟩`.
    Propose(V),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<V> {
    /// `⟨ uc, Decide | v ⟩`. Raised at most once.
    Decide(V),
}

/// Paxos: uniform consensus over an epoch-change and a sequence of abortable epoch consensuses.
pub struct LeaderDrivenConsensus<V> {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    retransmit: core::time::Duration,
    /// `val` — what this process wants decided, once the layer above has said.
    val: Option<V>,
    /// `proposed` — whether this process has proposed in the current epoch.
    proposed: bool,
    /// `decided` — whether it has reported a decision. Reported at most once.
    decided: bool,
    /// `(ets, ℓ)` — the epoch now live, and who leads it.
    ets: u64,
    leader: NodeId,
    /// `(newts, newℓ)` — the epoch waiting for the current one to finish aborting.
    pending: Option<(u64, NodeId)>,
    ec: EpochChange,
    /// `ep.ets`. One instance, replaced on each epoch change — never a map.
    ep: EpochConsensus<V>,
    ec_inbox: Vec<ec::Ind>,
    ep_inbox: Vec<ep::Ind<V>>,
}

impl<V: Clone> LeaderDrivenConsensus<V> {
    /// Paxos among `peers`.
    pub fn new(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        retransmit: core::time::Duration,
        heartbeat: core::time::Duration,
        detect_after: core::time::Duration,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        // "Obtain the leader ℓ0 of the initial epoch with timestamp 0 from epoch-change" — the same
        // `maxrank(Π)` the leader detector will trust first, so the two agree before anything moves.
        let l0 = peers.iter().next_back().copied().expect("Π contains at least this process");
        LeaderDrivenConsensus {
            me,
            peers: peers.clone(),
            retransmit,
            val: None,
            proposed: false,
            decided: false,
            ets: 0,
            leader: l0,
            pending: None,
            ec: EpochChange::new(me, peers.clone(), retransmit, heartbeat, detect_after),
            ep: EpochConsensus::new(me, peers, 0, l0, State::default(), retransmit),
            ec_inbox: Vec::new(),
            ep_inbox: Vec::new(),
        }
    }

    /// The epoch now live at this process.
    pub fn epoch(&self) -> u64 {
        self.ets
    }

    /// Who leads the epoch now live.
    pub fn leader(&self) -> NodeId {
        self.leader
    }

    /// Whether this process has reported a decision.
    pub fn has_decided(&self) -> bool {
        self.decided
    }

    /// `(valts, val)` as the epoch now live holds it.
    ///
    /// This is what the abort handshake carries forward: the state a new instance is constructed
    /// from is the state the aborted one returned, so a value accepted in an early epoch is still
    /// here after several leadership changes.
    pub fn state(&self) -> &State<V> {
        self.ep.state()
    }

    /// `upon ℓ = self ∧ val ≠ ⊥ ∧ proposed = FALSE` — a standing condition, re-evaluated whenever
    /// any of the three could have changed.
    fn maybe_propose(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.leader == self.me
            && !self.proposed
            && let Some(v) = self.val.clone()
        {
            self.proposed = true;
            self.through_ep(cx, |e, ccx| e.on_cmd(ep::Cmd::Propose(v), ccx));
        }
    }

    /// `upon event ⟨ ec, StartEpoch | newts, newℓ ⟩ do … trigger ⟨ ep.ets, Abort ⟩`.
    fn on_start_epoch(&mut self, ts: u64, leader: NodeId, cx: &mut ProtoCx<'_, Self>) {
        self.pending = Some((ts, leader));
        self.through_ep(cx, |e, ccx| e.on_cmd(ep::Cmd::Abort, ccx));
    }

    /// `upon event ⟨ ep.ts, Aborted | state ⟩ such that ts = ets`.
    ///
    /// The replacement is built here and nowhere else, because `state` is only available here.
    fn on_aborted(&mut self, state: State<V>, cx: &mut ProtoCx<'_, Self>) {
        let Some((ts, leader)) = self.pending.take() else {
            // An `Aborted` with no epoch waiting: the book's `such that ts = ets` guard, which
            // discards an answer from an instance already superseded.
            return;
        };
        self.ets = ts;
        self.leader = leader;
        self.proposed = false;
        self.ep =
            EpochConsensus::new(self.me, self.peers.clone(), ts, leader, state, self.retransmit);
        self.maybe_propose(cx);
    }

    fn through_ec(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut EpochChange, &mut ProtoCx<'_, EpochChange>),
    ) {
        let mut inbox = core::mem::take(&mut self.ec_inbox);
        {
            let ec = &mut self.ec;
            cx.with_child_consuming(Wire::Change, &mut inbox, |ccx| f(ec, ccx));
        }
        for ec::Ind::StartEpoch { ts, leader } in inbox.drain(..) {
            self.on_start_epoch(ts, leader, cx);
        }
        self.ec_inbox = inbox;
    }

    fn through_ep(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut EpochConsensus<V>, &mut ProtoCx<'_, EpochConsensus<V>>),
    ) {
        let mut inbox = core::mem::take(&mut self.ep_inbox);
        {
            let ep = &mut self.ep;
            cx.with_child_consuming(Wire::Consensus, &mut inbox, |ccx| f(ep, ccx));
        }
        for ind in inbox.drain(..) {
            match ind {
                // `upon event ⟨ ep.ts, Decide | v ⟩ such that ts = ets do if decided = FALSE …`
                ep::Ind::Decide(v) => {
                    if !self.decided {
                        self.decided = true;
                        cx.indicate(Ind::Decide(v));
                    }
                }
                ep::Ind::Aborted(state) => self.on_aborted(state, cx),
            }
        }
        self.ep_inbox = inbox;
    }
}

impl<V: Clone> Protocol for LeaderDrivenConsensus<V> {
    type Cmd = Cmd<V>;
    type Ind = Ind<V>;
    type Msg = Wire<<EpochChange as Protocol>::Msg, ep::BebMsg<V>>;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably. `logged_leader_driven_consensus` is the variant that does.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    /// `upon event ⟨ uc, Propose | v ⟩ do val := v`.
    fn on_cmd(&mut self, Cmd::Propose(v): Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        self.val = Some(v);
        self.maybe_propose(cx);
    }

    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Change(m) => self.through_ec(cx, |ec, ccx| ec.on_msg(from, m, ccx)),
            // The instance guard is inside the child, which drops anything not stamped with its
            // own epoch. See `ep::Tagged`.
            Wire::Consensus(m) => self.through_ep(cx, |ep, ccx| ep.on_msg(from, m, ccx)),
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_ec(cx, |ec, ccx| ec.on_timer(id, ccx));
        self.through_ep(cx, |ep, ccx| ep.on_timer(id, ccx));
    }

    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.through_ec(cx, |ec, ccx| ec.on_init(ccx));
    }
}
