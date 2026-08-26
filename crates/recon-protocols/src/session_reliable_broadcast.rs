//! Eager reliable broadcast over session links.
//!
//! **Status: transcription. Space: unbounded.** `delivered` grows with every message delivered,
//! as in the original.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.2 and Algorithm 3.3, unchanged. What differs is the
//! link beneath, and what that costs the guarantee.
//!
//! # Its agreement is scoped, and it cannot do better
//!
//! Over a perfect link, a relay always arrives; the link retransmits until it does. Over a session
//! link it may not, and this layer has nothing with which to retry:
//!
//! - It relays **once**, on first delivery. That is what makes eager reliable broadcast eager.
//! - It keeps `delivered` as a set of **identifiers**, not payloads — so even knowing a relay was
//!   lost, it has no copy to send again. Retaining payloads would be state growing with messages,
//!   which `docs/bounded-space.md` forbids without a window.
//! - It is **fail-silent**. Algorithm 3.3 uses no failure detector, so it cannot conclude that a
//!   process is gone and stop expecting to reach it.
//!
//! So when a relay is lost to a session ending, nothing retries and nothing gives up:
//!
//! ```text
//! RB1 [session]  Validity
//! RB2 [always]   No duplication
//! RB3 [always]   No creation
//! RB4 [session]  Agreement — within the sessions carrying the relay, and not across one
//! ```
//!
//! This is not a defect to be fixed here. It is the honest reading of Algorithm 3.3 on a link that
//! can lose a suffix, and it is exactly what the uniform version beside it does not share — that
//! one has a failure detector, and between reconnection and accusation it has no third outcome.
//! Reading the two together is the sharpest available argument for why uniform reliable broadcast
//! needs a detector at all.
//!
//! What this layer does do is report the session events upward rather than absorb them, so that a
//! layer which *can* act is not denied the signal.

use recon_core::{NodeId, ProtoCx, Protocol, SessionEvent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::session_best_effort_broadcast::{self as beb, SessionBestEffortBroadcast};

/// Names one broadcast uniquely: who originated it, and their sequence number for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BroadcastId {
    pub origin: NodeId,
    pub seq: u64,
}

/// What this layer adds to the wire. The only header in this stack: the session link adds none,
/// and best-effort broadcast adds none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data<P> {
    pub id: BroadcastId,
    pub payload: P,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// `from` is the originator, never a relayer.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Broadcast(beb::Timer),
}

pub type Wire<P> = Data<P>;

/// Eager reliable broadcast whose agreement is bounded by the sessions carrying its relays.
#[derive(Debug)]
pub struct SessionReliableBroadcast<P> {
    me: NodeId,
    seq: u64,
    delivered: BTreeSet<BroadcastId>,
    beb: SessionBestEffortBroadcast<Data<P>>,
    inbox: Vec<beb::Ind<Data<P>>>,
    relay_inbox: Vec<beb::Ind<Data<P>>>,
}

impl<P> SessionReliableBroadcast<P> {
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>) -> Self {
        SessionReliableBroadcast {
            me,
            seq: 0,
            delivered: BTreeSet::new(),
            beb: SessionBestEffortBroadcast::new(me, peers),
            inbox: Vec::new(),
            relay_inbox: Vec::new(),
        }
    }

    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }
}

impl<P: Clone> SessionReliableBroadcast<P> {
    fn with_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut SessionBestEffortBroadcast<Data<P>>,
            &mut ProtoCx<'_, SessionBestEffortBroadcast<Data<P>>>,
        ),
    ) {
        let mut inbox = core::mem::take(&mut self.inbox);
        inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(core::convert::identity, Timer::Broadcast, &mut inbox, |ccx| {
                f(beb, ccx)
            });
        }
        for ind in inbox.drain(..) {
            match ind {
                beb::Ind::Deliver { msg: Data { id, payload }, .. } => {
                    self.on_beb_deliver(id, payload, cx)
                }
                // Reported onward. This layer cannot retry, but the one above may be able to.
                beb::Ind::SessionEnded { peer, epoch } => {
                    cx.indicate(Ind::SessionEnded { peer, epoch })
                }
                beb::Ind::SessionEstablished { peer, epoch } => {
                    cx.indicate(Ind::SessionEstablished { peer, epoch })
                }
            }
        }
        self.inbox = inbox;
    }

    /// Deliver once, relay once. The relay is never repeated, which is where the scope comes from.
    fn on_beb_deliver(&mut self, id: BroadcastId, payload: P, cx: &mut ProtoCx<'_, Self>) {
        if !self.delivered.insert(id) {
            return;
        }
        cx.indicate(Ind::Deliver { from: id.origin, msg: payload.clone() });
        self.relay(Data { id, payload }, cx);
    }

    fn relay(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        let mut relay_inbox = core::mem::take(&mut self.relay_inbox);
        relay_inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(
                core::convert::identity,
                Timer::Broadcast,
                &mut relay_inbox,
                |ccx| beb.on_cmd(beb::Cmd::Broadcast(data), ccx),
            );
        }
        debug_assert!(
            relay_inbox.is_empty(),
            "relaying must not deliver synchronously; if it does, on_beb_deliver can recurse"
        );
        self.relay_inbox = relay_inbox;
    }
}

impl<P: Clone> Protocol for SessionReliableBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    type Timer = Timer;
    type Scope = SessionEvent;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Broadcast(msg): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let data = Data { id: BroadcastId { origin: self.me, seq: self.seq }, payload: msg };
        self.with_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::Broadcast(data), ccx));
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, Timer::Broadcast(t): Timer, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_timer(t, ccx));
    }

    fn on_scope_event(&mut self, event: SessionEvent, cx: &mut ProtoCx<'_, Self>) {
        // Routed down so the link can record it and report it back up. This layer takes no other
        // action: it has nothing to resend.
        self.with_beb(cx, |beb, ccx| beb.on_scope_event(event, ccx));
    }
}
