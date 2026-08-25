//! Uniform reliable broadcast over session links and a perfect failure detector.
//!
//! **Status: transcription. Space: unbounded.** `pending`, `ack` and `delivered` grow, as in the
//! original.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.3 and Algorithm 3.4 ("All-Ack"), with **one** clause
//! added and nothing else changed.
//!
//! # Why this rung stays live where the reliable one does not
//!
//! Over a session link a message can be lost with the session that carried it. Algorithm 3.4 then
//! waits for an acknowledgement that will never come, and nobody delivers — a validity failure.
//!
//! Two mechanisms between them leave no third outcome:
//!
//! - **The session comes back.** A deployed link keeps trying to reconnect on its own, so a peer
//!   that is reachable will be reconnected to. On being told a session is *established*, this
//!   layer re-broadcasts what that peer has not been seen to acknowledge.
//! - **The peer never comes back.** The failure detector's timeout expires, `correct` loses it,
//!   and `correct ⊆ ack[m]` no longer waits for it.
//!
//! The added clause is the first of those:
//!
//! ```text
//! upon event ⟨ SessionEstablished | q ⟩ do
//!     forall (s, m) ∈ pending such that q ∉ ack[m] do
//!         trigger ⟨ beb, Broadcast | [DATA, s, m] ⟩;
//! ```
//!
//! It adds **no state** — `pending` already holds the payloads and `ack` already records who has
//! been seen to acknowledge what, both from Algorithm 3.4 — and **no message type**: the
//! broadcast is the one the algorithm already performs. It is a new trigger for an existing
//! action, not a new communication step.
//!
//! Nothing is attempted on a session *ending*. The peer is unreachable at that moment and anything
//! sent would be discarded; the ending is informative, not actionable.
//!
//! ```text
//! URB1 [always]  Validity
//! URB2 [incarnation]  No duplication
//! URB3 [always]  No creation
//! URB4 [always]  Uniform agreement
//! ```
//!
//! Read against `session_reliable_broadcast`, which has neither mechanism and whose agreement is
//! therefore scoped, this is the clearest statement of what a failure detector buys.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, SessionEvent};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::perfect_failure_detector::{self as pfd, Heartbeat, PerfectFailureDetector};
use crate::session_best_effort_broadcast::{self as beb, SessionBestEffortBroadcast};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BroadcastId {
    pub origin: NodeId,
    pub seq: u64,
}

/// What this layer adds to the wire — the only header in the stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data<P> {
    pub id: BroadcastId,
    pub payload: P,
}

/// The wire type, multiplexing the two children. Typed, so a mis-route cannot compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<P> {
    Broadcast(Data<P>),
    Detector(Heartbeat),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    /// Begin failure detection. Required before any delivery can complete.
    Start,
    Broadcast(P),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    Deliver { from: NodeId, msg: P },
    SessionEnded { peer: NodeId, epoch: u64 },
    SessionEstablished { peer: NodeId, epoch: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Broadcast(beb::Timer),
    Detector(pfd::Tick),
}

/// Uniform reliable broadcast that survives a link which can lose a suffix.
#[derive(Debug)]
pub struct SessionUniformReliableBroadcast<P> {
    me: NodeId,
    seq: u64,
    correct: BTreeSet<NodeId>,
    pending: BTreeMap<BroadcastId, P>,
    ack: BTreeMap<BroadcastId, BTreeSet<NodeId>>,
    delivered: BTreeSet<BroadcastId>,
    beb: SessionBestEffortBroadcast<Data<P>>,
    detector: PerfectFailureDetector,
    beb_inbox: Vec<beb::Ind<Data<P>>>,
    det_inbox: Vec<pfd::Ind>,
    relay_inbox: Vec<beb::Ind<Data<P>>>,
}

impl<P> SessionUniformReliableBroadcast<P> {
    pub fn new(
        me: NodeId,
        members: impl IntoIterator<Item = NodeId>,
        heartbeat: Duration,
        detect_after: Duration,
    ) -> Self {
        let mut members: BTreeSet<NodeId> = members.into_iter().collect();
        members.insert(me);
        SessionUniformReliableBroadcast {
            me,
            seq: 0,
            correct: members.clone(),
            pending: BTreeMap::new(),
            ack: BTreeMap::new(),
            delivered: BTreeSet::new(),
            beb: SessionBestEffortBroadcast::new(me, members.clone()),
            detector: PerfectFailureDetector::new(me, members, heartbeat, detect_after),
            beb_inbox: Vec::new(),
            det_inbox: Vec::new(),
            relay_inbox: Vec::new(),
        }
    }

    pub fn correct(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.correct.iter().copied()
    }

    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn acknowledged_by(&self, id: BroadcastId) -> impl Iterator<Item = NodeId> + '_ {
        self.ack.get(&id).into_iter().flatten().copied()
    }
}

impl<P: Clone> SessionUniformReliableBroadcast<P> {
    fn with_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut SessionBestEffortBroadcast<Data<P>>,
            &mut ProtoCx<'_, SessionBestEffortBroadcast<Data<P>>>,
        ),
    ) {
        let mut inbox = core::mem::take(&mut self.beb_inbox);
        inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(Wire::Broadcast, Timer::Broadcast, &mut inbox, |ccx| {
                f(beb, ccx)
            });
        }
        for ind in inbox.drain(..) {
            match ind {
                beb::Ind::Deliver { from, msg: Data { id, payload } } => {
                    self.on_beb_deliver(from, id, payload, cx)
                }
                beb::Ind::SessionEnded { peer, epoch } => {
                    // Informative only: the peer is unreachable, so nothing can be resent yet.
                    cx.indicate(Ind::SessionEnded { peer, epoch });
                }
                beb::Ind::SessionEstablished { peer, epoch } => {
                    cx.indicate(Ind::SessionEstablished { peer, epoch });
                    self.resend_to(peer, cx);
                }
            }
        }
        self.beb_inbox = inbox;
        self.check_deliverable(cx);
    }

    fn with_detector(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut PerfectFailureDetector, &mut ProtoCx<'_, PerfectFailureDetector>),
    ) {
        let mut inbox = core::mem::take(&mut self.det_inbox);
        inbox.clear();
        {
            let detector = &mut self.detector;
            cx.with_child_consuming(Wire::Detector, Timer::Detector, &mut inbox, |ccx| {
                f(detector, ccx)
            });
        }
        for ind in inbox.drain(..) {
            let pfd::Ind::Crash { node } = ind;
            self.correct.remove(&node);
        }
        self.det_inbox = inbox;
        self.check_deliverable(cx);
    }

    /// The one clause Algorithm 3.4 does not have: on a session becoming available again, relay
    /// everything still pending to that peer.
    ///
    /// It resends unconditionally, and the reason is worth stating because the obvious
    /// optimisation is wrong. `ack[m]` records who relayed `m` **to me**. It says nothing about
    /// whether **my** relay reached them, and my relay is exactly the token they are waiting for.
    /// Skipping a peer that is already in `ack[m]` deadlocks: p delivers `m`, having seen
    /// everyone relay it, and therefore never resends its own relay to q, while q waits for p's
    /// relay forever. Under Algorithm 3.4's perfect links the question does not arise, because a
    /// relay sent once is a relay eventually delivered. A session link withdraws that, so the
    /// relay has to be repeatable, and nothing short of an acknowledgement message — which would
    /// be a new communication step — can tell us when to stop.
    ///
    /// The cost is that a re-establishment resends all of `pending`, which Algorithm 3.4 never
    /// prunes. That is the transcription's unbounded growth showing up as traffic rather than
    /// just as memory; see `docs/bounded-space.md`.
    fn resend_to(&mut self, peer: NodeId, cx: &mut ProtoCx<'_, Self>) {
        let outstanding: Vec<Data<P>> = self
            .pending
            .iter()
            .map(|(id, payload)| Data { id: *id, payload: payload.clone() })
            .collect();
        for data in outstanding {
            self.send_to(peer, data, cx);
        }
    }

    /// The resend goes only to the peer whose session came back: same wire message, strictly
    /// fewer recipients than a relay, and no new communication step.
    fn send_to(&mut self, peer: NodeId, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        self.through_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::SendTo { to: peer, msg: data }, ccx));
    }

    fn on_beb_deliver(
        &mut self,
        from: NodeId,
        id: BroadcastId,
        payload: P,
        cx: &mut ProtoCx<'_, Self>,
    ) {
        self.ack.entry(id).or_default().insert(from);
        if self.pending.insert(id, payload.clone()).is_none() {
            self.relay(Data { id, payload }, cx);
        }
    }

    fn relay(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        self.through_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::Broadcast(data), ccx));
    }

    /// Drive the broadcast child for an outgoing send, where no indication can come back.
    fn through_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut SessionBestEffortBroadcast<Data<P>>,
            &mut ProtoCx<'_, SessionBestEffortBroadcast<Data<P>>>,
        ),
    ) {
        let mut relay_inbox = core::mem::take(&mut self.relay_inbox);
        relay_inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(Wire::Broadcast, Timer::Broadcast, &mut relay_inbox, |ccx| {
                f(beb, ccx)
            });
        }
        debug_assert!(
            relay_inbox.is_empty(),
            "sending must not deliver synchronously; if it does, on_beb_deliver can recurse"
        );
        self.relay_inbox = relay_inbox;
    }

    fn check_deliverable(&mut self, cx: &mut ProtoCx<'_, Self>) {
        let ready: Vec<BroadcastId> = self
            .pending
            .keys()
            .copied()
            .filter(|id| !self.delivered.contains(id))
            .filter(|id| self.can_deliver(*id))
            .collect();
        for id in ready {
            self.delivered.insert(id);
            let payload = self.pending.get(&id).expect("pending by construction").clone();
            cx.indicate(Ind::Deliver { from: id.origin, msg: payload });
        }
    }

    fn can_deliver(&self, id: BroadcastId) -> bool {
        match self.ack.get(&id) {
            None => false,
            Some(acked) => self.correct.iter().all(|p| acked.contains(p)),
        }
    }
}

impl<P: Clone> Protocol for SessionUniformReliableBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    type Timer = Timer;
    type Scope = SessionEvent;

    fn on_cmd(&mut self, cmd: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            Cmd::Start => self.with_detector(cx, |d, ccx| d.on_cmd(pfd::Cmd::Start, ccx)),
            Cmd::Broadcast(msg) => {
                self.seq += 1;
                let id = BroadcastId { origin: self.me, seq: self.seq };
                self.pending.insert(id, msg.clone());
                self.ack.entry(id).or_default();
                let data = Data { id, payload: msg };
                self.with_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::Broadcast(data), ccx));
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Broadcast(m) => self.with_beb(cx, |beb, ccx| beb.on_msg(from, m, ccx)),
            Wire::Detector(h) => self.with_detector(cx, |d, ccx| d.on_msg(from, h, ccx)),
        }
    }

    fn on_timer(&mut self, token: Timer, cx: &mut ProtoCx<'_, Self>) {
        match token {
            Timer::Broadcast(t) => self.with_beb(cx, |beb, ccx| beb.on_timer(t, ccx)),
            Timer::Detector(t) => self.with_detector(cx, |d, ccx| d.on_timer(t, ccx)),
        }
    }

    fn on_scope_end(&mut self, event: SessionEvent, cx: &mut ProtoCx<'_, Self>) {
        // Routed to the broadcast child, which owns the link. The detector has no scopes.
        self.with_beb(cx, |beb, ccx| beb.on_scope_end(event, ccx));
    }
}
