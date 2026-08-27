//! A link whose guarantees come from an underlying session.
//!
//! **Status: deployable. Space: bounded by membership.**
//!
//! This is what would run over TCP or QUIC. It does not retransmit and it does not deduplicate,
//! because within a session the transport does neither — it delivers reliably and in order, or
//! the session ends. So this link holds one epoch per peer and nothing per message, which makes
//! it the first in this repository to satisfy the rule in `docs/bounded-space.md`.
//!
//! Compare the perfect link, which obtains the same guarantees from a stubborn link by
//! retransmitting for ever and remembering every identifier it has seen. That is how you build a
//! perfect link when you have nothing underneath — the simulator's situation, not a deployment's.
//! The deployable link needs *less* state, not more.
//!
//! # What it will not pretend
//!
//! A session ends and an unknown suffix of what was in flight is gone. The perfect link has no
//! way to express that and would carry on as if nothing happened; the previous attempt at this
//! project did exactly that, and `docs/postmortem.md` records what it cost. Here the ending is a
//! scope, reported upward as an indication naming the peer and the new epoch, so the layer above
//! must decide what its own guarantee does about it.
//!
//! ```text
//! SL1 [session(q)]  Reliable ordered delivery: while a session with q holds, every message sent
//!                   to q is delivered, in order, exactly once.
//! SL2 [always]      No creation: a message is delivered only if it was previously sent.
//! ```
//!
//! In the notation of `docs/scope-annotated-modules.md`, SL1's scope is well-formed: the session's
//! end is an event this link is told about, so it can react to it, report it, and be tested
//! against it.

use recon_core::{NodeId, ProtoCx, Protocol, SessionEvent, TimerId};
use std::collections::BTreeMap;

/// What crosses the wire: the payload, unchanged.
///
/// The session supplies ordering and reliability, so this layer adds no header at all — no
/// sequence number, no identifier. It is the thinnest link in the repository.
pub type Wire<P> = P;

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Send { to: NodeId, msg: P },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// A message arrived. Ordered with respect to others from the same peer in the same session.
    Deliver { from: NodeId, msg: P },
    /// The session with `peer` ended at `epoch`. Anything sent to that peer and not yet delivered
    /// may have been lost, and this link cannot say which. Nothing can be done about it yet.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A session with `peer` is in force at `epoch`. This is the moment on which anything that
    /// must be resent can be.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// Reliable ordered delivery within a session, and honesty across one.
#[derive(Debug, Default)]
pub struct SessionLink<P> {
    /// The epoch **currently in force** for each peer: inserted on an establishment and removed
    /// on the matching ending. One entry per peer with a live session, and nothing that grows
    /// with messages.
    ///
    /// Removing is the point. Keeping the number after the ending would report a dead session as
    /// current, which is the opposite of what this layer exists to say.
    epochs: BTreeMap<NodeId, u64>,
    _payload: core::marker::PhantomData<P>,
}

impl<P> SessionLink<P> {
    pub fn new() -> Self {
        SessionLink { epochs: BTreeMap::new(), _payload: core::marker::PhantomData }
    }

    /// The epoch in force with `peer`, or `None` when there is no session with it.
    pub fn epoch(&self, peer: NodeId) -> Option<u64> {
        self.epochs.get(&peer).copied()
    }

    /// How many peers this link currently holds a session with. Its entire footprint.
    pub fn tracked_peers(&self) -> usize {
        self.epochs.len()
    }
}

impl<P: Clone> Protocol for SessionLink<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    type Scope = SessionEvent;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Send { to, msg }: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        // No sequence number, no retransmission buffer, no record kept. The session is
        // responsible for getting it there or for telling us it could not.
        cx.send(to, msg);
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        // No deduplication: within a session the transport does not duplicate.
        cx.indicate(Ind::Deliver { from, msg });
    }

    fn on_timer(&mut self, _id: TimerId, _cx: &mut ProtoCx<'_, Self>) {
        // Registers none, and has no child to pass one to.
    }

    fn on_scope_event(&mut self, event: SessionEvent, cx: &mut ProtoCx<'_, Self>) {
        // The one thing this link exists to do that a perfect link cannot: say so. Both events
        // are reported — the ending because a suffix may be gone, the establishment because it is
        // the only moment on which anything can be resent.
        match event {
            SessionEvent::Ended { peer, epoch } => {
                // The session is gone, so the epoch is not current any more. Leaving it would
                // have `epoch()` report a dead session as live.
                self.epochs.remove(&peer);
                cx.indicate(Ind::SessionEnded { peer, epoch });
            }
            SessionEvent::Established { peer, epoch } => {
                self.epochs.insert(peer, epoch);
                cx.indicate(Ind::SessionEstablished { peer, epoch });
            }
        }
    }
}

/// The session link satisfies the link port, and its scoped half as well.
///
/// Implementing [`crate::link::ScopedLink`] is a claim that this link can observe the boundaries of
/// the scope its guarantees hold within, and it can: the simulator raises a session ending and an
/// establishment, and this link is where they enter the stack. That is what a layer above needs in
/// order to repair a lost suffix, and what the perfect link cannot offer.
impl<P> crate::link::Link<P> for SessionLink<P>
where
    P: Clone,
{
    fn send(to: NodeId, msg: P) -> Cmd<P> {
        Cmd::Send { to, msg }
    }

    fn classify(ind: Ind<P>) -> crate::link::LinkInd<P> {
        use crate::link::{Boundary, LinkInd};
        match ind {
            Ind::Deliver { from, msg } => LinkInd::Deliver { from, msg },
            Ind::SessionEnded { peer, epoch } => LinkInd::Boundary(Boundary::Ended { peer, epoch }),
            Ind::SessionEstablished { peer, epoch } => {
                LinkInd::Boundary(Boundary::Established { peer, epoch })
            }
        }
    }
}

impl<P> crate::link::ScopedLink<P> for SessionLink<P> where P: Clone {}
