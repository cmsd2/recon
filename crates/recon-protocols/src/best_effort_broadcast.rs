//! Best-effort broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.1 and Algorithm 3.1 ("Basic Broadcast").
//!
//! **Status: deployable. Space: bounded by membership.** This layer holds only the process set;
//! everything else belongs to the link beneath it.
//!
//! Sends the message individually to every process over perfect links. If the sender is
//! correct, every correct process delivers it. If the sender crashes partway through, some
//! processes may deliver and others may not — that is the guarantee this abstraction
//! deliberately does not make, and the reason the stronger broadcasts exist.
//!
//! ```text
//! upon event ⟨ beb, Broadcast | m ⟩ do
//!     forall q ∈ Π do
//!         trigger ⟨ pl, Send | q, m ⟩;
//!
//! upon event ⟨ pl, Deliver | p, m ⟩ do
//!     trigger ⟨ beb, Deliver | p, m ⟩;
//! ```
//!
//! Π includes the sender, so a process broadcasts to itself the same way it broadcasts to
//! everyone else. Self-delivery is not a special case.
//!
//! This layer adds nothing to the wire: its message type is the perfect link's, unchanged, and
//! its indication handler is pure forwarding. It is the second of the three protocols in this
//! stack to contribute no header of its own.
//!
//! # Over a link that reports scope boundaries
//!
//! `L` is a parameter, so this one module is both the perfect-link broadcast above and what
//! `session_best_effort_broadcast` used to be. The algorithm is unchanged either way — Algorithm
//! 3.1 does not mention links — but what it can promise is not.
//!
//! Over a perfect link, a message sent to a correct process arrives; the link retransmits until it
//! does. Over a session link it may not: a session can end with the message in flight, and that
//! link does not retry. So validity holds only while the sessions carrying a broadcast hold.
//!
//! This layer cannot repair that. It keeps nothing but the process set — no copy of what it sent,
//! no record of who received — so there is nothing to resend from, and giving it one would be
//! state growing with messages, which `docs/bounded-space.md` forbids. What it can do is refuse to
//! conceal it: both boundary reports are passed upward, because they are the only signal the
//! layers above have, and one of them — uniform reliable broadcast — can act on what this layer
//! cannot.
//!
//! ```text
//! BEB1 [session]  Best-effort validity   — [always] over a link that cannot end
//! BEB2 [always]   No duplication
//! BEB3 [always]   No creation
//! ```
//!
//! The scope annotation is the link's, not this layer's: over a link whose guarantees never lapse,
//! `[session]` is vacuous and BEB1 reads as the book states it.
//!
//! One request is not in Module 3.1. [`Cmd::SendTo`] sends to a single member of Π — same wire
//! message, same link, strictly fewer recipients, no new communication step. It exists so a layer
//! above can answer a scope that has just come back without paying for a fan-out to everyone
//! else, and it is a narrowing of `Broadcast` rather than an addition to the module.

use core::marker::PhantomData;
use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use std::collections::BTreeSet;

use crate::link::{Boundary, Link, LinkInd, VolatileLink};
use crate::perfect_link::PerfectLink;

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
    /// Send to one member only.
    ///
    /// Not part of Module 3.1, which has only `Broadcast`. It exists so a layer above can answer a
    /// scope that has just come back without re-sending to everyone else: same wire message, same
    /// link, strictly fewer recipients. No new communication step, so no guarantee of Module 3.1
    /// is affected — this is a narrowing of `Broadcast`, not an addition to it.
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
    /// The scope with `peer` ended at `epoch`. A broadcast in flight to it may have been lost, and
    /// this layer cannot say which — it has no redundancy to bridge with, so it propagates.
    ///
    /// Raised only over a link that reports boundaries; over a perfect link this never occurs.
    /// Declaring it regardless is the price of one implementation serving both, and it is a price
    /// paid by the layer above rather than by the link, which is what
    /// `docs/scope-annotated-modules.md` forbids only of the link.
    SessionEnded {
        peer: NodeId,
        epoch: u64,
    },
    /// A scope with `peer` is in force at `epoch`. Anything to be resent can be resent now.
    SessionEstablished {
        peer: NodeId,
        epoch: u64,
    },
}

/// Translate the link's indication into this layer's — Algorithm 3.1's second handler, plus the
/// propagation of a boundary this layer cannot bridge.
fn forward<P, L: Link<P>>(ind: L::Ind) -> Ind<P> {
    match L::classify(ind) {
        LinkInd::Deliver { from, msg } => Ind::Deliver { from, msg },
        LinkInd::Boundary(Boundary::Ended { peer, epoch }) => Ind::SessionEnded { peer, epoch },
        LinkInd::Boundary(Boundary::Established { peer, epoch }) => {
            Ind::SessionEstablished { peer, epoch }
        }
    }
}

/// Fan-out to every process over perfect links.
///
/// `L` is the link beneath, and it is a parameter rather than a fixed type. What this layer needs
/// of it is stated in one bound and nowhere else: [`Link`], the port. Anything satisfying it — a
/// session link, a logged link, or an application's own driver — can carry this broadcast without
/// either side being edited. That is the seam `docs/conditional-guarantees.md` describes, made
/// checkable.
///
/// Fan-out needs nothing of a scope boundary, so the bound is the port and nothing more. This
/// layer composes over every link there is, and passes a boundary upward untouched because it has
/// no redundancy with which to repair one.
///
/// It defaults to [`PerfectLink`], so the ordinary stack is still written `BestEffortBroadcast<P>`.
#[derive(Debug)]
pub struct BestEffortBroadcast<P, L = PerfectLink<P>> {
    /// Π — every process in the system, including this one.
    peers: BTreeSet<NodeId>,
    link: L,
    _payload: PhantomData<fn() -> P>,
}

impl<P, L> BestEffortBroadcast<P, L> {
    /// Broadcast among `peers`, which must include `me`, over a link the caller supplies.
    pub fn with_link(me: NodeId, peers: impl IntoIterator<Item = NodeId>, link: L) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        BestEffortBroadcast { peers, link, _payload: PhantomData }
    }

    /// The processes this broadcasts to, in a stable order.
    pub fn peers(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.peers.iter().copied()
    }

    /// The link beneath, for a caller that has reason to inspect its own.
    pub fn link(&self) -> &L {
        &self.link
    }
}

impl<P> BestEffortBroadcast<P, PerfectLink<P>> {
    /// Broadcast among `peers`, which must include `me`, over the book's perfect link.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, interval: Duration) -> Self {
        Self::with_link(me, peers, PerfectLink::new(me, interval))
    }

    /// How many distinct messages the link below has delivered upward.
    ///
    /// Specific to the perfect link, so it lives here rather than on every link.
    pub fn delivered_count(&self) -> usize {
        self.link.delivered_count()
    }
}

impl<P: Clone, L> Protocol for BestEffortBroadcast<P, L>
where
    L: VolatileLink<P>,
{
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = L::Msg;
    /// Whatever the link's guarantees are conditional on, since this layer adds no condition of
    /// its own and cannot bridge the link's.
    type Scope = L::Scope;
    type Note = crate::Note;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        let link = &mut self.link;
        let peers = &self.peers;
        cx.with_child(core::convert::identity, forward::<P, L>, |ccx| match cmd {
            // Algorithm 3.1: `forall q in Π do trigger <pl, Send | q, m>`. Π includes the sender,
            // so a correct process delivers its own broadcast.
            Cmd::Broadcast(msg) => {
                for &q in peers {
                    link.on_cmd(L::send(q, msg.clone()), ccx);
                }
            }
            // The same send, to one member of Π rather than all of it.
            Cmd::SendTo { to, msg } => {
                debug_assert!(peers.contains(&to), "a directed send addresses a member of Π");
                link.on_cmd(L::send(to, msg), ccx);
            }
        });
    }

    fn on_msg(&mut self, from: NodeId, msg: L::Msg, cx: &mut ProtoCx<'_, Self>) {
        let link = &mut self.link;
        cx.with_child(core::convert::identity, forward::<P, L>, |ccx| link.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        let link = &mut self.link;
        cx.with_child(core::convert::identity, forward::<P, L>, |ccx| link.on_timer(id, ccx));
    }

    /// Hand the scope ending down to the link, which is the layer that knows what it means.
    ///
    /// `Scope` is the link's, so leaving this to the trait's default would take a scope event the
    /// driver raised and drop it — the layer above would never learn its guarantees had lapsed,
    /// and neither would the link. That is the failure `docs/conditional-guarantees.md` calls
    /// cardinal, and the default is silent about committing it, which is why this handler exists
    /// even though its body only forwards.
    fn on_scope_event(&mut self, scope: L::Scope, cx: &mut ProtoCx<'_, Self>) {
        let link = &mut self.link;
        cx.with_child(core::convert::identity, forward::<P, L>, |ccx| {
            link.on_scope_event(scope, ccx)
        });
    }
}
