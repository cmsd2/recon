//! Epoch-change — a sequence of epochs, each with a timestamp and a leader.
//!
//! **Status: implementation. Space: bounded by membership.**
//!
//! Cachin, Guerraoui & Rodrigues, Module 5.3 and Algorithm 5.5 ("Leader-Based Epoch-Change"),
//! quoted from the book:
//!
//! ```text
//! Algorithm 5.5: Leader-Based Epoch-Change
//! Implements: EpochChange, instance ec.
//! Uses:
//!     PerfectPointToPointLinks, instance pl;
//!     BestEffortBroadcast, instance beb;
//!     EventualLeaderDetector, instance Ω.
//!
//! upon event ⟨ ec, Init ⟩ do
//!     trusted := ℓ0;
//!     lastts := 0;
//!     ts := rank(self);
//!
//! upon event ⟨ Ω, Trust | p ⟩ do
//!     trusted := p;
//!     if p = self then
//!         ts := ts + N;
//!         trigger ⟨ beb, Broadcast | [NEWEPOCH, ts] ⟩;
//!
//! upon event ⟨ beb, Deliver | ℓ, [NEWEPOCH, newts] ⟩ do
//!     if ℓ = trusted ∧ newts > lastts then
//!         lastts := newts;
//!         trigger ⟨ ec, StartEpoch | newts, ℓ ⟩;
//!     else
//!         trigger ⟨ pl, Send | ℓ, [NACK] ⟩;
//!
//! upon event ⟨ pl, Deliver | p, [NACK] ⟩ do
//!     if trusted = self then
//!         ts := ts + N;
//!         trigger ⟨ beb, Broadcast | [NEWEPOCH, ts] ⟩;
//! ```
//!
//! # Why timestamps are unique without anyone coordinating
//!
//! `ts := rank(self)` and `ts := ts + N`. Each process therefore draws from its own residue class
//! modulo `N`, and two processes cannot mint the same timestamp however far apart they drift. A
//! plain counter would not have that property, and the layer above uses the timestamp to order
//! writes — so two epochs sharing one would make its safety argument meaningless.
//!
//! # The NACK, and what it is for
//!
//! A process that receives a `NEWEPOCH` it will not act on — because the sender is not who it
//! trusts, or because the timestamp is not newer than one it has already started — answers `NACK`.
//! A leader that is nacked bumps its timestamp and tries again.
//!
//! Without it a leader whose timestamp has fallen behind another's would broadcast for ever and
//! never be started by anyone. The NACK is what lets it discover that and climb past.
//!
//! # Departure: the NACK travels by directed broadcast, not by a separate link
//!
//! The book names two message children, `pl` for the NACK and `beb` for the NEWEPOCH. Here there is
//! one: [`crate::best_effort_broadcast`] gained a directed [`beb::Cmd::SendTo`] in the
//! `link-parameterisation` change — same wire message, same link, strictly fewer recipients, no
//! new communication step — and a NACK sent that way is a perfect-link send with an extra layer's
//! name on it.
//!
//! What this buys is one child fewer and one wire variant fewer. What it costs is that the module
//! no longer mirrors the book's `Uses:` line exactly, so it is recorded here rather than left for a
//! reader to notice. Nothing about the guarantee changes: `beb::Cmd::SendTo` reaches exactly the
//! one addressed process, which is what `pl, Send` does.
//!
//! # Departure: the NACK names the timestamp it refuses
//!
//! Algorithm 5.5 sends a bare `[NACK]` and bumps `ts` on every one that arrives. Algorithm 5.8 —
//! the same abstraction in the fail-recovery model — sends `[NACK, nts]` and guards the handler
//! with `such that nts = ts`. This module takes 5.8's form, because over a link that retransmits,
//! 5.5's does not terminate.
//!
//! The loop: the leader broadcasts `NEWEPOCH(t)`; every process that does not yet trust it answers
//! `NACK`; each NACK bumps `ts` and broadcasts again, so one announcement to `N` processes produces
//! `N − 1` further announcements, each of which produces its own. The stubborn link beneath resends
//! everything it has ever sent, so nothing decays. Measured before the guard: a five-process run
//! with one crash reached epoch **647,309** and 2.3 million sends inside a second of virtual time,
//! and no epoch ever lasted long enough for the consensus above it to finish a write. Measured
//! after: single figures.
//!
//! With the guard, an announcement is answered at most once — the first NACK moves `ts`, and every
//! later NACK naming the old timestamp is for an announcement already superseded. That is the whole
//! of the fix, and the book states it one algorithm later.
//!
//! ```text
//! EC1 [always]     Monotonicity — timestamps strictly increase, and one timestamp names one leader
//! EC2 [eventual]   Consistency — eventually every correct process starts the same last epoch
//! ```

use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use crate::eventual_leader_detector::{self as eld, EventualLeaderDetector};
use crate::perfect_failure_detector::Heartbeat;
use crate::perfect_link as pl;

/// What this layer puts on the wire, beneath the broadcast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpochMsg {
    /// `[NEWEPOCH, ts]` — the trusted leader announcing the epoch it wants to start.
    NewEpoch { ts: u64 },
    /// `[NACK, nts]` — "I will not start that one", sent back to the would-be leader, naming the
    /// timestamp it is refusing. Algorithm 5.5 writes a bare `[NACK]`; the timestamp is taken from
    /// Algorithm 5.8, and the reason is in the module documentation.
    Nack { nts: u64 },
}

/// The wire, multiplexing the two children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<B> {
    /// The leader detector's heartbeats.
    Detector(Heartbeat),
    /// The broadcast's traffic, carrying [`EpochMsg`].
    Epoch(B),
}

/// Requests from the layer above.
///
/// Uninhabited: epochs begin at initialisation and change when leadership does. There is nothing
/// for the layer above to ask for.
pub type Cmd = core::convert::Infallible;

/// Indications to the layer above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ind {
    /// `⟨ ec, StartEpoch | newts, ℓ ⟩` — begin the epoch numbered `ts`, led by `leader`.
    StartEpoch { ts: u64, leader: NodeId },
}

/// What the broadcast beneath puts on the wire for this layer's messages.
///
/// Written concretely rather than as a projection, for the reason
/// [`crate::uniform_reliable_broadcast::BebMsg`] gives: the projection is only well-formed where
/// `EpochMsg: Clone`, which would push that bound onto every use. The assertion below keeps the two
/// from drifting apart.
pub type BebMsg = pl::Wire<EpochMsg>;

const _: () = {
    /// Fails to compile if best-effort broadcast ever puts something else on the wire.
    fn _beb_msg_is_what_we_say_it_is(
        m: BebMsg,
    ) -> <BestEffortBroadcast<EpochMsg> as Protocol>::Msg {
        m
    }
};

/// A sequence of epochs, driven by who is trusted.
#[derive(Debug)]
pub struct EpochChange {
    me: NodeId,
    /// Π. Its size is the book's `N`, and a process's position in it is `rank`.
    peers: BTreeSet<NodeId>,
    /// `trusted`.
    trusted: NodeId,
    /// `lastts` — the timestamp of the last epoch this process started.
    lastts: u64,
    /// `ts` — this process's own next candidate, always in its own residue class mod `N`.
    ts: u64,
    omega: EventualLeaderDetector,
    beb: BestEffortBroadcast<EpochMsg>,
    omega_inbox: Vec<eld::Ind>,
    beb_inbox: Vec<beb::Ind<EpochMsg>>,
}

impl EpochChange {
    /// Epoch-change among `peers`, over a leader detector with the given heartbeat and timeout and
    /// a best-effort broadcast whose links retransmit every `retransmit`.
    pub fn new(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        retransmit: core::time::Duration,
        heartbeat: core::time::Duration,
        detect_after: core::time::Duration,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        // `ℓ0` — the book's initial leader, fixed and known to all. `maxrank(Π)` is what Ω will
        // trust first with nobody suspected, so starting there means the first `Trust` that agrees
        // with it changes nothing.
        let l0 = peers.iter().next_back().copied().expect("Π contains at least this process");
        EpochChange {
            me,
            peers: peers.clone(),
            trusted: l0,
            lastts: 0,
            ts: rank(&peers, me),
            omega: EventualLeaderDetector::new(me, peers.clone(), heartbeat, detect_after),
            beb: BestEffortBroadcast::new(me, peers, retransmit),
            omega_inbox: Vec::new(),
            beb_inbox: Vec::new(),
        }
    }

    /// The epoch this process has most recently started, if any.
    pub fn last_timestamp(&self) -> u64 {
        self.lastts
    }

    /// Who this process currently trusts.
    pub fn trusted(&self) -> NodeId {
        self.trusted
    }

    /// `ts := ts + N; trigger ⟨ beb, Broadcast | [NEWEPOCH, ts] ⟩`.
    fn announce(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.ts += self.peers.len() as u64;
        let ts = self.ts;
        self.through_beb(cx, |b, ccx| {
            b.on_cmd(beb::Cmd::Broadcast(EpochMsg::NewEpoch { ts }), ccx)
        });
    }

    /// `upon event ⟨ Ω, Trust | p ⟩`.
    fn on_trust(&mut self, leader: NodeId, cx: &mut ProtoCx<'_, Self>) {
        self.trusted = leader;
        if leader == self.me {
            self.announce(cx);
        }
    }

    /// `upon event ⟨ beb, Deliver | ℓ, … ⟩` and `⟨ pl, Deliver | p, [NACK] ⟩`.
    fn on_epoch_msg(&mut self, from: NodeId, msg: EpochMsg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            EpochMsg::NewEpoch { ts } => {
                if from == self.trusted && ts > self.lastts {
                    self.lastts = ts;
                    cx.indicate(Ind::StartEpoch { ts, leader: from });
                } else {
                    // The would-be leader is not who this process trusts, or its timestamp is not
                    // newer than one already started. Either way: tell it, so it can climb past.
                    self.through_beb(cx, |b, ccx| {
                        b.on_cmd(
                            beb::Cmd::SendTo { to: from, msg: EpochMsg::Nack { nts: ts } },
                            ccx,
                        )
                    });
                }
            }
            // `upon event ⟨ sl, Deliver | p, [NACK, nts] ⟩ such that nts = ts` — Algorithm 5.8's
            // guard, applied here. Without it this is the divergence described in the module
            // documentation.
            EpochMsg::Nack { nts } => {
                if self.trusted == self.me && nts == self.ts {
                    self.announce(cx);
                }
            }
        }
    }

    fn through_omega(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut EventualLeaderDetector, &mut ProtoCx<'_, EventualLeaderDetector>),
    ) {
        let mut inbox = core::mem::take(&mut self.omega_inbox);
        {
            let omega = &mut self.omega;
            cx.with_child_consuming(Wire::Detector, &mut inbox, |ccx| f(omega, ccx));
        }
        for eld::Ind::Trust { leader } in inbox.drain(..) {
            self.on_trust(leader, cx);
        }
        self.omega_inbox = inbox;
    }

    fn through_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut BestEffortBroadcast<EpochMsg>,
            &mut ProtoCx<'_, BestEffortBroadcast<EpochMsg>>,
        ),
    ) {
        let mut inbox = core::mem::take(&mut self.beb_inbox);
        {
            let b = &mut self.beb;
            cx.with_child_consuming(Wire::Epoch, &mut inbox, |ccx| f(b, ccx));
        }
        for ind in inbox.drain(..) {
            match ind {
                beb::Ind::Deliver { from, msg } => self.on_epoch_msg(from, msg, cx),
                // The broadcast is over a perfect link, which reports no scope boundary.
                beb::Ind::SessionEnded { .. } | beb::Ind::SessionEstablished { .. } => {}
            }
        }
        self.beb_inbox = inbox;
    }
}

/// `rank(p)` — a process's position in `Π`, counting from one.
///
/// Any fixed injective map would do; this is the one that makes `ts := rank(self)` put each process
/// in its own residue class modulo `N`.
fn rank(peers: &BTreeSet<NodeId>, p: NodeId) -> u64 {
    peers.iter().position(|q| *q == p).expect("p ∈ Π") as u64 + 1
}

impl Protocol for EpochChange {
    type Cmd = Cmd;
    type Ind = Ind;
    type Msg = Wire<BebMsg>;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably. `logged_epoch_change` is the variant that does.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd, _: &mut ProtoCx<'_, Self>) {
        match cmd {}
    }

    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Detector(h) => self.through_omega(cx, |o, ccx| o.on_msg(from, h, ccx)),
            Wire::Epoch(m) => self.through_beb(cx, |b, ccx| b.on_msg(from, m, ccx)),
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_omega(cx, |o, ccx| o.on_timer(id, ccx));
        self.through_beb(cx, |b, ccx| b.on_timer(id, ccx));
    }

    /// `upon event ⟨ ec, Init ⟩` — the state is set in `new`; this starts the detector, whose first
    /// `Trust` may immediately make this process announce an epoch.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.through_omega(cx, |o, ccx| o.on_init(ccx));
    }
}
