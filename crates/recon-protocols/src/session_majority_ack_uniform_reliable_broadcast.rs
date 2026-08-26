//! Majority-ack uniform reliable broadcast over session links.
//!
//! **Status: transcription. Space: unbounded.** `pending`, `ack` and `delivered` grow, as in
//! [`crate::session_uniform_reliable_broadcast`]. Removing the detector removes a timing
//! assumption, not the collection debt. See `docs/bounded-space.md`.
//!
//! **Assumption: a correct, mutually reachable majority, `N > 2f`.** There is nothing else.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.3 and Algorithm 3.5 ("Majority-Ack"), with the same
//! one clause added that [`crate::session_uniform_reliable_broadcast`] adds to Algorithm 3.4:
//! on a session being *established*, send that peer what it may have missed.
//!
//! # Dropping the detector removes a whole liveness path, not just a dependency
//!
//! Over a session link a message can be lost with the session that carried it, and the all-ack
//! version needs **two** mechanisms so that no case is left waiting for ever:
//!
//! - the session comes back, and what the peer missed is sent again; or
//! - the peer never comes back, the detector's timeout expires, and `correct ⊆ ack[m]` stops
//!   waiting for it.
//!
//! A majority quorum deletes the second. A peer that never returns was never waited for in the
//! first place — `#(ack[m]) > N/2` asks how many have relayed, not which ones — so no judgement
//! about it is needed, none is made, and there is nothing to be wrong about. Resending on
//! re-establishment is the only liveness mechanism left, and `2 · #(ack[m]) > N` the only
//! condition:
//!
//! ```text
//! function candeliver(m) returns Boolean is
//!     return #(ack[m]) > N/2;
//!
//! upon event ⟨ SessionEstablished | q ⟩ do
//!     forall (s, m) ∈ pending do
//!         trigger ⟨ beb, SendTo | q, [DATA, s, m] ⟩;
//! ```
//!
//! The consequence worth naming: a peer absent for far longer than any timeout the all-ack
//! version would have used is **not a stranger when it returns**. Nothing excluded it, so nothing
//! has to readmit it; it receives what it missed and delivers it. The all-ack version cannot say
//! that, because its detector's accusations are permanent.
//!
//! # The resend is unconditional, and that is not an oversight
//!
//! `ack[m]` records who relayed `m` **to this process**. It says nothing about whether *this*
//! process's relay reached them, and that relay is the token they are waiting for. Filtering the
//! resend by `q ∉ ack[m]` deadlocks — the argument is recorded in full in
//! [`crate::session_uniform_reliable_broadcast`], where a test found it. The delivery predicate
//! changing does not change that argument, so the clause is carried over unchanged, including its
//! cost: a re-establishment sends every pending message to that peer.
//!
//! # When the assumption fails, this layer blocks rather than diverges
//!
//! A partition leaving one side with fewer than half the processes delivers nothing on that side,
//! rather than delivering something the majority will never deliver. When the sides rejoin, the
//! minority catches up through the same resend clause. Compare the all-ack version, where each
//! side accuses the other and both proceed — which is a split, and permanent.
//!
//! # Departures from the page
//!
//! - Algorithm 3.5 has no session events; the establishment clause above is this layer's, and is
//!   the same one the all-ack version over sessions carries.
//! - The predicate is written `2 · #(ack[m]) > N`, for the reason given in
//!   [`crate::majority_ack_uniform_reliable_broadcast`].
//! - One child, so no wire multiplexing: the message *is* the broadcast child's. The all-ack
//!   version needs an enum because its detector also sends.
//! - No `Init` event and no `Start` command; there is nothing to start.
//! - Neither `ack` nor `pending` is garbage collected, as in the book.

use recon_core::{NodeId, ProtoCx, Protocol, SessionEvent};
use std::collections::{BTreeMap, BTreeSet};

use crate::session_best_effort_broadcast::{self as beb, SessionBestEffortBroadcast};
use crate::uniform_reliable_broadcast::{BroadcastId, Data};

/// The message type: the broadcast child's, unwrapped. With one child there is nothing to
/// multiplex and no discriminant to add.
pub type Msg<P> = <SessionBestEffortBroadcast<Data<P>> as Protocol>::Msg;

/// Timers, which are the broadcast child's.
pub type Timer = beb::Timer;

/// Requests from the layer above. Broadcasting is the only one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// `from` is the process that originated the message, never a relayer.
    Deliver {
        from: NodeId,
        msg: P,
    },
    SessionEnded {
        peer: NodeId,
        epoch: u64,
    },
    SessionEstablished {
        peer: NodeId,
        epoch: u64,
    },
}

/// Uniform reliable broadcast over a link that can end, resting on a correct majority.
#[derive(Debug)]
pub struct SessionMajorityAckUniformReliableBroadcast<P> {
    me: NodeId,
    seq: u64,
    /// How many processes there are. The denominator of the majority, and fixed.
    members: usize,
    pending: BTreeMap<BroadcastId, P>,
    ack: BTreeMap<BroadcastId, BTreeSet<NodeId>>,
    delivered: BTreeSet<BroadcastId>,
    beb: SessionBestEffortBroadcast<Data<P>>,
    beb_inbox: Vec<beb::Ind<Data<P>>>,
    send_inbox: Vec<beb::Ind<Data<P>>>,
}

impl<P> SessionMajorityAckUniformReliableBroadcast<P> {
    /// Broadcast among `members`, which must include `me`.
    ///
    /// No timing parameters, because nothing here waits on a clock.
    pub fn new(me: NodeId, members: impl IntoIterator<Item = NodeId>) -> Self {
        let mut members: BTreeSet<NodeId> = members.into_iter().collect();
        members.insert(me);
        let n = members.len();
        SessionMajorityAckUniformReliableBroadcast {
            me,
            seq: 0,
            members: n,
            pending: BTreeMap::new(),
            ack: BTreeMap::new(),
            delivered: BTreeSet::new(),
            beb: SessionBestEffortBroadcast::new(me, members),
            beb_inbox: Vec::new(),
            send_inbox: Vec::new(),
        }
    }

    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Which processes have relayed `id`, for tests watching the majority form.
    pub fn acknowledged_by(&self, id: BroadcastId) -> impl Iterator<Item = NodeId> + '_ {
        self.ack.get(&id).into_iter().flatten().copied()
    }

    /// How many processes a message must be relayed by before it can be delivered.
    pub fn majority(&self) -> usize {
        self.members / 2 + 1
    }
}

impl<P: Clone> SessionMajorityAckUniformReliableBroadcast<P> {
    /// Run the broadcast child, then act on what it reported.
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
            cx.with_child_consuming(
                core::convert::identity,
                core::convert::identity,
                &mut inbox,
                |ccx| f(beb, ccx),
            );
        }
        for ind in inbox.drain(..) {
            match ind {
                beb::Ind::Deliver { from, msg: Data { id, payload } } => {
                    self.on_beb_deliver(from, id, payload, cx)
                }
                beb::Ind::SessionEnded { peer, epoch } => {
                    // Informative only: the peer is unreachable, so nothing can be sent yet.
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

    /// On a session becoming available again, send that peer everything still pending.
    ///
    /// Unconditional, and directed at that peer alone. See the module note.
    fn resend_to(&mut self, peer: NodeId, cx: &mut ProtoCx<'_, Self>) {
        let outstanding: Vec<Data<P>> = self
            .pending
            .iter()
            .map(|(id, payload)| Data { id: *id, payload: payload.clone() })
            .collect();
        for data in outstanding {
            self.through_beb(cx, |beb, ccx| {
                beb.on_cmd(beb::Cmd::SendTo { to: peer, msg: data }, ccx)
            });
        }
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
            self.through_beb(cx, |beb, ccx| {
                beb.on_cmd(beb::Cmd::Broadcast(Data { id, payload }), ccx)
            });
        }
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
        let mut send_inbox = core::mem::take(&mut self.send_inbox);
        send_inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(
                core::convert::identity,
                core::convert::identity,
                &mut send_inbox,
                |ccx| f(beb, ccx),
            );
        }
        debug_assert!(
            send_inbox.is_empty(),
            "sending must not deliver synchronously; if it does, on_beb_deliver can recurse"
        );
        self.send_inbox = send_inbox;
    }

    /// `upon exists (s, m) ∈ pending such that candeliver(m) ∧ m ∉ delivered`.
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

    /// `#(ack[m]) > N/2` — more than half the processes have relayed it. No process is consulted
    /// about who is correct, because no such set exists here.
    fn can_deliver(&self, id: BroadcastId) -> bool {
        match self.ack.get(&id) {
            None => false,
            Some(acked) => 2 * acked.len() > self.members,
        }
    }
}

impl<P: Clone> Protocol for SessionMajorityAckUniformReliableBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Msg<P>;
    type Timer = Timer;
    /// Inherited from the link: a session ending is a scope boundary this layer must see.
    type Scope = SessionEvent;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Broadcast(msg): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let id = BroadcastId { origin: self.me, seq: self.seq };
        self.pending.insert(id, msg.clone());
        self.ack.entry(id).or_default();
        let data = Data { id, payload: msg };
        self.with_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::Broadcast(data), ccx));
    }

    fn on_msg(&mut self, from: NodeId, msg: Msg<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, token: Timer, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_timer(token, ccx));
    }

    fn on_scope_event(&mut self, event: SessionEvent, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_scope_event(event, ccx));
    }
}
