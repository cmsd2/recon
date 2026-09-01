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
//! # Departure: a leader is told where the processes trusting it have reached
//!
//! `⟨ Ω, Trust | p ⟩` is raised when the trusted process **changes**, and Algorithm 5.5 announces an
//! epoch only on that edge. So a process that trusted *itself* all along is never told it has become
//! everyone's leader — and if the others ran their epochs ahead under other leaders while it did
//! not, nothing it would announce is high enough for them to accept, and nothing prompts it to climb.
//!
//! Measured, five processes partitioned `[A,B] [C] [D,E]` and then healed: Ω converges correctly on
//! `E`, and afterwards `trusted = [E,E,E,E,E]` with `lastts = [32, 27, 43, 10, 10]`. `E` is trusted
//! by everyone, sits in epoch 10, and announces nothing — for ever. Retransmission does not rescue
//! it either: what `E` re-sends is `NEWEPOCH(10)`, which its recipients' links deduplicate, so it
//! draws no refusal. `E` received **zero** in thirty timeouts.
//!
//! This is a gap in Algorithm 5.5 composed with **Algorithm 2.8**, rather than in either module's
//! specification: Module 2.9 says only that Ω eventually agrees, and an Ω that re-raised `Trust`
//! would leave 5.5 correct as written. It was unreachable while the detector beneath was
//! [`crate::perfect_failure_detector`], whose accusations are permanent, because a partition never
//! healed for it.
//!
//! So a process whose trusted leader changes to one that is **not itself**, while its current epoch
//! was started by somebody else, tells that leader the timestamp it has reached. The leader then
//! chooses its next candidate above what it was told. Nothing is sent while nothing has changed: the
//! report rides the same edge the announcement does.
//!
//! # Departure: a refused leader climbs past the refusal in one step
//!
//! The same message carries the refuser's own `lastts`, and the leader jumps its candidate above it
//! rather than adding `N`. Algorithm 5.5 steps once per refusal, which costs a round trip per step —
//! the gap above is 33, or seven round trips. Boundedness is unaffected, and for the same reason as
//! before: after the jump the leader's candidate is strictly above what it was told, so a repeated
//! report names a timestamp it has already passed and moves nothing.
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

use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use crate::eventual_leader_detector::{self as eld, EventualLeaderDetector};
use crate::perfect_failure_detector::Heartbeat;
use crate::perfect_link as pl;
use crate::{Note, Refusal, Timing};

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
    /// Who started it. Not the book's; needed to tell whether a newly trusted leader is already the
    /// one this process is following. See the departure on telling a leader where others have got to.
    started_by: NodeId,
    /// `ts` — this process's own next candidate, always in its own residue class mod `N`.
    ts: u64,
    omega: Child<EventualLeaderDetector>,
    beb: Child<BestEffortBroadcast<EpochMsg>>,
}

impl EpochChange {
    /// Epoch-change among `peers`, over a leader detector with the given heartbeat and timeout and
    /// a best-effort broadcast whose links retransmit every `retransmit`.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, timing: Timing) -> Self {
        let Timing { retransmit, heartbeat, detect_after } = timing;
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
            started_by: l0,
            ts: rank(&peers, me),
            omega: Child::new(EventualLeaderDetector::new(
                me,
                peers.clone(),
                heartbeat,
                detect_after,
            )),
            beb: Child::new(BestEffortBroadcast::new(me, peers, retransmit)),
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

    /// Announce the next candidate strictly above `floor`, staying in this process's residue class.
    ///
    /// The class is what keeps timestamps unique across processes — see the note on that above — so
    /// jumping means rounding up to the next value congruent to `rank(self)` modulo `N`, not simply
    /// taking `floor + 1`.
    fn announce_above(&mut self, floor: u64, cx: &mut ProtoCx<'_, Self>) {
        let n = self.peers.len() as u64;
        let residue = self.ts % n;
        let mut next = floor.max(self.ts) + 1;
        next += (n + residue - next % n) % n;
        debug_assert!(next > floor && next % n == residue);
        self.ts = next;
        let ts = self.ts;
        self.through_beb(cx, |b, ccx| {
            b.on_cmd(beb::Cmd::Broadcast(EpochMsg::NewEpoch { ts }), ccx)
        });
    }

    /// Tell `leader` the timestamp this process has reached, so a leader that was never told it
    /// became one can climb above it. Same message as a refusal, and for the same purpose.
    fn report_to(&mut self, leader: NodeId, cx: &mut ProtoCx<'_, Self>) {
        let nts = self.lastts;
        // The same `NACK` goes on the wire as when an announcement is refused, so the trace cannot
        // tell the two decisions apart. This is the half the trace cannot say.
        cx.note(Note::ReachReported { leader, nts });
        self.through_beb(cx, |b, ccx| {
            b.on_cmd(beb::Cmd::SendTo { to: leader, msg: EpochMsg::Nack { nts } }, ccx)
        });
    }

    /// `upon event ⟨ Ω, Trust | p ⟩`.
    fn on_trust(&mut self, leader: NodeId, cx: &mut ProtoCx<'_, Self>) {
        self.trusted = leader;
        if leader == self.me {
            self.announce(cx);
        } else if self.started_by != leader {
            // Departure: this process is following an epoch its new leader did not start, and that
            // leader may never have been told anything changed. Tell it where we have reached.
            self.report_to(leader, cx);
        }
    }

    /// `upon event ⟨ beb, Deliver | ℓ, … ⟩` and `⟨ pl, Deliver | p, [NACK] ⟩`.
    fn on_epoch_msg(&mut self, from: NodeId, msg: EpochMsg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            EpochMsg::NewEpoch { ts } => {
                if from == self.trusted && ts > self.lastts {
                    self.lastts = ts;
                    self.started_by = from;
                    cx.indicate(Ind::StartEpoch { ts, leader: from });
                } else {
                    // The would-be leader is not who this process trusts, or its timestamp is not
                    // newer than one already started. Either way: tell it, so it can climb past —
                    // and name the higher of the two, so it climbs past in one step rather than one
                    // step per refusal.
                    let why = if from != self.trusted {
                        Refusal::NotTrusted { trusted: self.trusted }
                    } else {
                        Refusal::NotAhead { reached: self.lastts }
                    };
                    cx.note(Note::EpochRefused { from, ts, why });
                    let nts = ts.max(self.lastts);
                    self.through_beb(cx, |b, ccx| {
                        b.on_cmd(beb::Cmd::SendTo { to: from, msg: EpochMsg::Nack { nts } }, ccx)
                    });
                }
            }
            // `upon event ⟨ sl, Deliver | p, [NACK, nts] ⟩ such that nts = ts` — Algorithm 5.8's
            // guard, relaxed to `nts ≥ ts` because the report now carries how far the sender has
            // reached rather than only what it refused. Boundedness is unchanged: the jump leaves
            // the candidate strictly above `nts`, so a repeat names a timestamp already passed.
            EpochMsg::Nack { nts } => {
                if self.trusted == self.me && nts >= self.ts {
                    self.announce_above(nts, cx);
                } else {
                    // **Nothing whatever reaches the trace from here.** This is the shape of
                    // silence that cost the most to diagnose: a leader that everyone trusts and
                    // which announces nothing looks, in a record of effects, exactly like a leader
                    // nobody told anything.
                    let why = if self.trusted != self.me {
                        Refusal::NotLeader { trusted: self.trusted }
                    } else {
                        Refusal::NotAhead { reached: self.ts }
                    };
                    cx.note(Note::ReportIgnored { from, nts, why });
                }
            }
        }
    }

    fn through_omega(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut EventualLeaderDetector, &mut ProtoCx<'_, EventualLeaderDetector>),
    ) {
        let mut inds = self.omega.run(cx, Wire::Detector, f);
        for eld::Ind::Trust { leader } in inds.drain(..) {
            self.on_trust(leader, cx);
        }
        self.omega.reclaim(inds);
    }

    fn through_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut BestEffortBroadcast<EpochMsg>,
            &mut ProtoCx<'_, BestEffortBroadcast<EpochMsg>>,
        ),
    ) {
        let mut inds = self.beb.run(cx, Wire::Epoch, f);
        for ind in inds.drain(..) {
            match ind {
                beb::Ind::Deliver { from, msg } => self.on_epoch_msg(from, msg, cx),
                // The broadcast is over a perfect link, which reports no scope boundary.
                beb::Ind::SessionEnded { .. } | beb::Ind::SessionEstablished { .. } => {}
            }
        }
        self.beb.reclaim(inds);
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
    type Note = crate::Note;
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
