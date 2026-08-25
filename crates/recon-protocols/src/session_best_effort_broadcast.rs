//! Best-effort broadcast over session links.
//!
//! **Status: deployable. Space: bounded by membership.**
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.1 and Algorithm 3.1 ("Basic Broadcast"), unchanged —
//! what differs is beneath it and what it must say about that.
//!
//! ```text
//! upon event ⟨ beb, Broadcast | m ⟩ do
//!     forall q ∈ Π do trigger ⟨ sl, Send | q, m ⟩;
//!
//! upon event ⟨ sl, Deliver | p, m ⟩ do
//!     trigger ⟨ beb, Deliver | p, m ⟩;
//! ```
//!
//! # What it says about sessions, and why it can do no more
//!
//! Over a perfect link, a message sent to a correct process arrives; the link retransmits until it
//! does. Over a session link it may not: a session can end with the message in flight, and the
//! link does not retry. So validity here holds only while the sessions carrying a broadcast hold.
//!
//! This layer cannot repair that. It keeps nothing but the process set — no copy of what it sent,
//! no record of who received — so there is nothing to resend from, and giving it one would be
//! state growing with messages, which `docs/bounded-space.md` forbids.
//!
//! What it can do is refuse to conceal it. Both session reports are passed upward, because they
//! are the only signal the layers above have, and one of them — uniform reliable broadcast — can
//! act on what this layer cannot.
//!
//! ```text
//! BEB1 [session]  Best-effort validity
//! BEB2 [always]   No duplication
//! BEB3 [always]   No creation
//! ```

use recon_core::{NodeId, ProtoCx, Protocol, SessionEvent};
use std::collections::BTreeSet;

use crate::session_link::{self as sl, SessionLink};

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
    /// Send to one member only.
    ///
    /// Not part of Module 3.1, which has only `Broadcast`. It exists so a layer above can
    /// answer a session that has just come back without re-sending to everyone else: same wire
    /// message, same link, strictly fewer recipients. No new communication step.
    SendTo {
        to: NodeId,
        msg: P,
    },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    Deliver {
        from: NodeId,
        msg: P,
    },
    /// The session with `peer` ended at `epoch`. A broadcast in flight to it may be lost.
    SessionEnded {
        peer: NodeId,
        epoch: u64,
    },
    /// A session with `peer` is in force at `epoch`. Anything to be resent can be now.
    SessionEstablished {
        peer: NodeId,
        epoch: u64,
    },
}

/// Timers, which are the child's re-wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Link(<SessionLink<()> as Protocol>::Timer),
}

/// The wire type: the payload, unchanged. Neither this layer nor the link beneath adds a header.
pub type Wire<P> = P;

/// Fan-out over session links.
#[derive(Debug)]
pub struct SessionBestEffortBroadcast<P> {
    /// Π — every process, including this one. The whole of this layer's state.
    peers: BTreeSet<NodeId>,
    link: SessionLink<P>,
    inbox: Vec<sl::Ind<P>>,
}

impl<P> SessionBestEffortBroadcast<P> {
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        SessionBestEffortBroadcast { peers, link: SessionLink::new(), inbox: Vec::new() }
    }

    pub fn peers(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.peers.iter().copied()
    }

    /// How many peers this layer holds state for. Its entire footprint, messages aside.
    pub fn tracked_peers(&self) -> usize {
        self.peers.len()
    }
}

impl<P: Clone> SessionBestEffortBroadcast<P> {
    /// Run the link, then pass on what it reported. This layer transforms nothing: a delivery is
    /// a delivery and a session report is a session report, both restated in its own terms.
    fn with_link(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut SessionLink<P>, &mut ProtoCx<'_, SessionLink<P>>),
    ) {
        let mut inbox = core::mem::take(&mut self.inbox);
        inbox.clear();
        {
            let link = &mut self.link;
            cx.with_child_consuming(core::convert::identity, Timer::Link, &mut inbox, |ccx| {
                f(link, ccx)
            });
        }
        for ind in inbox.drain(..) {
            match ind {
                sl::Ind::Deliver { from, msg } => cx.indicate(Ind::Deliver { from, msg }),
                sl::Ind::SessionEnded { peer, epoch } => {
                    cx.indicate(Ind::SessionEnded { peer, epoch })
                }
                sl::Ind::SessionEstablished { peer, epoch } => {
                    cx.indicate(Ind::SessionEstablished { peer, epoch })
                }
            }
        }
        self.inbox = inbox;
    }
}

impl<P: Clone> Protocol for SessionBestEffortBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    type Timer = Timer;
    /// Passed straight through: this layer has no scope of its own, and inherits the link's.
    type Scope = SessionEvent;

    fn on_cmd(&mut self, cmd: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        let msg = match cmd {
            Cmd::Broadcast(msg) => msg,
            Cmd::SendTo { to, msg } => {
                self.with_link(cx, |link, ccx| link.on_cmd(sl::Cmd::Send { to, msg }, ccx));
                return;
            }
        };
        let peers = self.peers.clone();
        for q in peers {
            let msg = msg.clone();
            self.with_link(cx, |link, ccx| link.on_cmd(sl::Cmd::Send { to: q, msg }, ccx));
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_link(cx, |link, ccx| link.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, Timer::Link(t): Timer, cx: &mut ProtoCx<'_, Self>) {
        self.with_link(cx, |link, ccx| link.on_timer(t, ccx));
    }

    fn on_scope_end(&mut self, event: SessionEvent, cx: &mut ProtoCx<'_, Self>) {
        self.with_link(cx, |link, ccx| link.on_scope_end(event, ccx));
    }
}
