//! Epoch-change that survives a restart.
//!
//! **Status: implementation. Space: bounded by membership, plus what the stubborn children hold
//! outstanding — which nothing here retires, so see the departure on `Stop` below.**
//!
//! Cachin, Guerraoui & Rodrigues, Algorithm 5.8 ("Logged Leader-Based Epoch-Change"), quoted from
//! the book:
//!
//! ```text
//! Algorithm 5.8: Logged Leader-Based Epoch-Change
//! Implements: LoggedEpochChange, instance lec.
//! Uses:
//!     StubbornPointToPointLinks, instance sl;
//!     StubbornBestEffortBroadcast, instance sbeb;
//!     EventualLeaderDetector, instance Ω.
//!
//! upon event ⟨ lec, Init ⟩ do
//!     trusted := ℓ0;
//!     (startts, start) := (0, ℓ0);
//!     ts := rank(self) − N;
//!
//! upon event ⟨ lec, Recovery ⟩ do
//!     retrieve(startts, start);
//!
//! upon event ⟨ Ω, Trust | p ⟩ do
//!     trusted := p;
//!     if p = self then
//!         ts := ts + N;
//!         trigger ⟨ sbeb, Broadcast | [NEWEPOCH, ts] ⟩;
//!
//! upon event ⟨ sbeb, Deliver | ℓ, [NEWEPOCH, newts] ⟩ do
//!     if ℓ = trusted ∧ newts > startts then
//!         (startts, start) := (newts, ℓ);
//!         store(startts, start);
//!         trigger ⟨ lec, StartEpoch | startts, start ⟩;
//!     else
//!         trigger ⟨ sl, Send | ℓ, [NACK, newts] ⟩;
//!
//! upon event ⟨ sl, Deliver | p, [NACK, nts] ⟩ such that nts = ts do
//!     if trusted = self then
//!         ts := ts + N;
//!         trigger ⟨ sbeb, Broadcast | [NEWEPOCH, ts] ⟩;
//! ```
//!
//! # What is durable, and what is not
//!
//! `(startts, start)` — the epoch this process has actually entered, and who leads it. Written
//! before `StartEpoch` is raised, in the handler's own text, because that indication is what the
//! consensus above acts on: a process that told its consensus to enter epoch 20 and then came back
//! believing it had entered nothing would read an empty state where an accepted value should be.
//!
//! `ts` — this process's own next candidate — is **not** durable, and the book does not store it.
//! A recovered leader therefore starts climbing again from `rank(self)`, and has to walk back up in
//! steps of `N` before it can announce a timestamp anybody will accept. That is slow but it is not
//! wrong, and the reason it is not wrong is that `startts` *is* durable: every process refuses a
//! timestamp no greater than the epoch it has already entered, so a reused candidate is refused
//! rather than confused with the epoch that first used it. The NACK carries the timestamp it
//! refuses, so each refusal moves the leader up exactly once.
//!
//! Reusing a candidate is safe for a second reason too: `ts ≡ rank(self) (mod N)` holds across
//! incarnations, because `rank` is a function of the membership rather than of anything this
//! process remembers. Two processes still cannot mint the same timestamp, so `EC2` — one timestamp
//! names one leader — survives a restart even though `ts` does not.
//!
//! # Identity, and how durable it has to be
//!
//! `CLAUDE.md`: an identifier that crosses the wire or lands in storage outlives the handler that
//! minted it. The [`sl::SendId`] and [`sbeb::BroadcastId`] counters here mint identifiers that do
//! neither — they name entries in the stubborn children's own volatile tables, which a crash
//! empties. A restarted process therefore restarts its counters at zero and names nothing that is
//! still live, because nothing is. Their scope is the incarnation, and that is the whole of it.
//!
//! The timestamp is the identifier that does cross the wire, and it is the one that is durable in
//! the sense that matters: not stored, but re-derived from `rank`, which does not change.
//!
//! # Departure: a repeat of the epoch already entered is not refused
//!
//! Algorithm 5.8 answers every NEWEPOCH it does not act on with a NACK. Over the stubborn broadcast
//! the same algorithm's `Uses:` line names, that does not terminate, and the loop is tighter than
//! the one [`crate::epoch_change`] describes: the leader announces `t`, every process enters it and
//! writes it down, and then the broadcast — which retransmits until retired, and nothing here
//! retires it — delivers `t` again. The second delivery fails `newts > startts`, because `startts`
//! is now `t`. So every process refuses the announcement it has just accepted, the leader climbs to
//! `t + N`, and the cycle restarts one retransmission interval later, for ever. Measured: epoch 380
//! and still climbing after eight timeouts, with leadership settled and nothing faulty.
//!
//! [`crate::epoch_change`] does not have this, and the reason is the child rather than the
//! algorithm: a best-effort broadcast over *perfect* links delivers each announcement exactly once,
//! so the repeat never reaches the handler. Moving to a stubborn broadcast — which must not
//! deduplicate, because repeating for ever is what reaches a recovered process — brings it back.
//!
//! The guard is that a repeat is not a refusal. `newts = startts` from the leader of the epoch
//! already entered is silence: there is nothing for the leader to climb past, because its
//! announcement was accepted. A NACK is still sent when the sender is not trusted, and when the
//! timestamp is genuinely stale — those are the two cases the book's `else` is for.
//!
//! # Departure: nothing calls `Stop`
//!
//! [`crate::stubborn_broadcast`] and [`crate::stubborn_link`] retransmit until retired, and this
//! module retires nothing, so its space grows with the number of announcements and refusals rather
//! than with the membership. That is the same unbounded transcription
//! [`crate::logged_uniform_reliable_broadcast`] has and for the same reason: retransmitting for
//! ever is what reaches a process that was down when the message was sent, and a recovered process
//! has no way to ask for what it missed.
//!
//! It is bounded in practice by the thing that bounds the announcements themselves — leadership
//! settling — and the NACK's timestamp guard is what makes that a finite number rather than a
//! feedback loop. See [`crate::epoch_change`], whose module documentation records what the
//! unguarded form cost.
//!
//! ```text
//! EC1 [always]     Monotonicity — the timestamps a process starts strictly increase, across
//!                  restarts as well as within one incarnation, and one timestamp names one leader
//! EC2 [conditional] Consistency — every correct process eventually starts the same last epoch,
//!                  provided the leader detector settles
//! ```

use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::eventual_leader_detector::{self as eld, EventualLeaderDetector};
use crate::perfect_failure_detector::Heartbeat;
use crate::stubborn_broadcast::{self as sbeb, BroadcastId, StubbornBroadcast};
use crate::stubborn_link::{self as sl, SendId, StubbornLink};

/// `[NEWEPOCH, ts]` — the trusted leader announcing the epoch it wants to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEpoch {
    pub ts: u64,
}

/// `[NACK, nts]` — "I will not start that one", naming the timestamp refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nack {
    pub nts: u64,
}

/// The wire, multiplexing the three children the book names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire {
    /// The leader detector's heartbeats.
    Detector(Heartbeat),
    /// `sbeb` — the announcements.
    Announce(NewEpoch),
    /// `sl` — the refusals, which go to one process rather than all.
    Refuse(Nack),
}

/// Requests from the layer above.
///
/// Uninhabited, as in [`crate::epoch_change`]: epochs begin at initialisation and change when
/// leadership does.
pub type Cmd = core::convert::Infallible;

/// Indications to the layer above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ind {
    /// `⟨ lec, StartEpoch | startts, start ⟩`. Raised only after the pair is durable.
    StartEpoch { ts: u64, leader: NodeId },
}

/// `(startts, start)` — the one metadata value this layer rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Started {
    pub ts: u64,
    pub leader: NodeId,
}

/// A sequence of epochs whose current position survives a restart.
#[derive(Debug)]
pub struct LoggedEpochChange {
    me: NodeId,
    /// Π. Its size is the book's `N`, and a process's position in it is `rank`.
    peers: BTreeSet<NodeId>,
    /// `trusted`.
    trusted: NodeId,
    /// `(startts, start)` — durable.
    started: Started,
    /// `ts` — volatile, and re-derived from `rank` on a restart. See the module documentation.
    ts: u64,
    /// Names the next stubborn transmission. Volatile, and so is what it keys.
    next_send: u64,
    /// Names the next stubborn broadcast. Volatile, and so is what it keys.
    next_broadcast: u64,
    omega: EventualLeaderDetector,
    sbeb: StubbornBroadcast<NewEpoch>,
    sl: StubbornLink<Nack>,
    omega_inbox: Vec<eld::Ind>,
    sbeb_inbox: Vec<sbeb::Ind<NewEpoch>>,
    sl_inbox: Vec<sl::Ind<Nack>>,
}

impl LoggedEpochChange {
    /// Epoch-change among `peers`, over a leader detector with the given heartbeat and timeout and
    /// stubborn children retransmitting every `retransmit`.
    pub fn new(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        retransmit: core::time::Duration,
        heartbeat: core::time::Duration,
        detect_after: core::time::Duration,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        let l0 = peers.iter().next_back().copied().expect("Π contains at least this process");
        // `ts := rank(self) − N`, so that the `ts := ts + N` in the first `Trust` makes the first
        // announcement `rank(self)` rather than `rank(self) + N`. Held as a signed value only here;
        // `rank` counts from one and `N ≥ 1`, so the first increment lands on `rank`.
        let ts = rank(&peers, me);
        let n = peers.len() as u64;
        LoggedEpochChange {
            me,
            peers: peers.clone(),
            trusted: l0,
            started: Started { ts: 0, leader: l0 },
            ts: ts.wrapping_sub(n),
            next_send: 0,
            next_broadcast: 0,
            omega: EventualLeaderDetector::new(me, peers.clone(), heartbeat, detect_after),
            sbeb: StubbornBroadcast::new(me, peers.clone(), retransmit),
            sl: StubbornLink::new(retransmit),
            omega_inbox: Vec::new(),
            sbeb_inbox: Vec::new(),
            sl_inbox: Vec::new(),
        }
    }

    /// `startts` — the epoch this process has entered, as its durable record has it.
    pub fn last_timestamp(&self) -> u64 {
        self.started.ts
    }

    /// `start` — who leads the epoch this process has entered.
    pub fn last_leader(&self) -> NodeId {
        self.started.leader
    }

    /// Who this process currently trusts.
    pub fn trusted(&self) -> NodeId {
        self.trusted
    }

    /// `ts` — this process's own next candidate. Volatile; see the module documentation.
    pub fn candidate(&self) -> u64 {
        self.ts
    }

    /// `ts := ts + N; trigger ⟨ sbeb, Broadcast | [NEWEPOCH, ts] ⟩`.
    fn announce(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.ts = self.ts.wrapping_add(self.peers.len() as u64);
        let msg = NewEpoch { ts: self.ts };
        let id = BroadcastId(self.next_broadcast);
        self.next_broadcast += 1;
        self.through_sbeb(cx, |b, ccx| b.on_cmd(sbeb::Cmd::Broadcast { id, msg }, ccx));
    }

    /// `upon event ⟨ Ω, Trust | p ⟩`.
    fn on_trust(&mut self, leader: NodeId, cx: &mut ProtoCx<'_, Self>) {
        self.trusted = leader;
        if leader == self.me {
            self.announce(cx);
        }
    }

    /// `upon event ⟨ sbeb, Deliver | ℓ, [NEWEPOCH, newts] ⟩`.
    ///
    /// **The order of the two statements in the `then` branch is the obligation.** `store` comes
    /// before `trigger`, here in the handler's own text, because `StartEpoch` is what makes the
    /// epoch visible to the consensus above.
    fn on_new_epoch(&mut self, from: NodeId, newts: u64, cx: &mut ProtoCx<'_, Self>) {
        if from == self.trusted && newts > self.started.ts {
            self.started = Started { ts: newts, leader: from };
            cx.storage().set(self.started);
            cx.indicate(Ind::StartEpoch { ts: newts, leader: from });
        } else if from == self.started.leader && newts == self.started.ts {
            // A repeat of the announcement this process has already accepted. Silence, not a
            // refusal — see the departure in the module documentation.
        } else {
            let id = SendId(self.next_send);
            self.next_send += 1;
            self.through_sl(cx, |l, ccx| {
                l.on_cmd(sl::Cmd::Send { id, to: from, msg: Nack { nts: newts } }, ccx)
            });
        }
    }

    /// `upon event ⟨ sl, Deliver | p, [NACK, nts] ⟩ such that nts = ts`.
    ///
    /// The stubborn link beneath repeats a refusal until it is retired, and nothing retires one, so
    /// this handler sees the same NACK many times. `nts = ts` makes that idempotent: the first one
    /// moves `ts`, and every repeat then names a candidate already superseded.
    fn on_nack(&mut self, nts: u64, cx: &mut ProtoCx<'_, Self>) {
        if nts == self.ts && self.trusted == self.me {
            self.announce(cx);
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

    fn through_sbeb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut StubbornBroadcast<NewEpoch>, &mut ProtoCx<'_, StubbornBroadcast<NewEpoch>>),
    ) {
        let mut inbox = core::mem::take(&mut self.sbeb_inbox);
        {
            let b = &mut self.sbeb;
            cx.with_child_consuming(Wire::Announce, &mut inbox, |ccx| f(b, ccx));
        }
        for sbeb::Ind::Deliver { from, msg } in inbox.drain(..) {
            self.on_new_epoch(from, msg.ts, cx);
        }
        self.sbeb_inbox = inbox;
    }

    fn through_sl(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut StubbornLink<Nack>, &mut ProtoCx<'_, StubbornLink<Nack>>),
    ) {
        let mut inbox = core::mem::take(&mut self.sl_inbox);
        {
            let l = &mut self.sl;
            cx.with_child_consuming(Wire::Refuse, &mut inbox, |ccx| f(l, ccx));
        }
        for sl::Ind::Deliver { msg, .. } in inbox.drain(..) {
            self.on_nack(msg.nts, cx);
        }
        self.sl_inbox = inbox;
    }
}

/// `rank(p)` — a process's position in `Π`, counting from one.
///
/// A function of the membership alone, which is why it survives a restart without being stored.
fn rank(peers: &BTreeSet<NodeId>, p: NodeId) -> u64 {
    peers.iter().position(|q| *q == p).expect("p ∈ Π") as u64 + 1
}

impl Protocol for LoggedEpochChange {
    type Cmd = Cmd;
    type Ind = Ind;
    type Msg = Wire;
    type Scope = core::convert::Infallible;
    type Meta = Started;
    /// Nothing accumulates: the epoch entered is one value, rewritten.
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd, _: &mut ProtoCx<'_, Self>) {
        match cmd {}
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Detector(h) => self.through_omega(cx, |o, ccx| o.on_msg(from, h, ccx)),
            Wire::Announce(m) => self.through_sbeb(cx, |b, ccx| b.on_msg(from, m, ccx)),
            Wire::Refuse(m) => self.through_sl(cx, |l, ccx| l.on_msg(from, m, ccx)),
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_omega(cx, |o, ccx| o.on_timer(id, ccx));
        self.through_sbeb(cx, |b, ccx| b.on_timer(id, ccx));
        self.through_sl(cx, |l, ccx| l.on_timer(id, ccx));
    }

    /// `upon event ⟨ lec, Init ⟩` — the state is set in `new`; this starts the detector.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.through_omega(cx, |o, ccx| o.on_init(ccx));
    }

    /// `upon event ⟨ lec, Recovery ⟩ do retrieve(startts, start)`.
    ///
    /// No `StartEpoch` is raised. The epoch is not new — this process entered it before it went
    /// down, and told the layer above so at the time. Re-raising it would announce as fresh an
    /// epoch whose consensus instance already exists, which is the layer above's business to
    /// reconstruct from its own record and not this layer's to invent.
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if let Some(started) = cx.storage().get().copied() {
            self.started = started;
        }
        self.through_omega(cx, |o, ccx| o.on_init(ccx));
    }
}
