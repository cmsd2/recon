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

use core::marker::PhantomData;
use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use std::collections::BTreeSet;

use crate::perfect_link::{self as pl, PerfectLink};

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    Deliver { from: NodeId, msg: P },
}

/// Translate the perfect link's delivery into this layer's — the whole of Algorithm 3.1's
/// second handler.
fn forward<P>(ind: pl::Ind<P>) -> Ind<P> {
    let pl::Ind::Deliver { from, msg } = ind;
    Ind::Deliver { from, msg }
}

/// What a broadcast needs of the link beneath it, and the whole of what it needs.
///
/// Named once so that every layer above states the same requirement, and so that an application
/// bringing its own link has one thing to satisfy. This is the seam
/// `docs/conditional-guarantees.md` describes: layers above the link may depend on its `Cmd` and
/// `Ind` types and nothing else.
pub trait Link<P>:
    Protocol<
        Cmd = pl::Cmd<P>,
        Ind = pl::Ind<P>,
        Meta = core::convert::Infallible,
        Entry = core::convert::Infallible,
    >
{
}

impl<P, L> Link<P> for L where
    L: Protocol<
            Cmd = pl::Cmd<P>,
            Ind = pl::Ind<P>,
            Meta = core::convert::Infallible,
            Entry = core::convert::Infallible,
        >
{
}

/// Fan-out to every process over perfect links.
///
/// `L` is the link beneath, and it is a parameter rather than a fixed type. What this layer needs
/// of it is stated in the bound on the [`Protocol`] impl and nowhere else: that it speaks
/// [`pl::Cmd`] and [`pl::Ind`]. Anything that does — a session link, a logged link, or an
/// application's own driver — can carry this broadcast without either side being edited. That is
/// the seam `docs/conditional-guarantees.md` describes, made checkable.
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
    L: Protocol<
            Cmd = pl::Cmd<P>,
            Ind = pl::Ind<P>,
            Meta = core::convert::Infallible,
            Entry = core::convert::Infallible,
        >,
{
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = L::Msg;
    /// No scope conditions: this protocol's guarantees do not lapse.
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Broadcast(msg): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        let link = &mut self.link;
        let peers = &self.peers;
        cx.with_child(core::convert::identity, forward, |ccx| {
            for &q in peers {
                link.on_cmd(pl::Cmd::Send { to: q, msg: msg.clone() }, ccx);
            }
        });
    }

    fn on_msg(&mut self, from: NodeId, msg: L::Msg, cx: &mut ProtoCx<'_, Self>) {
        let link = &mut self.link;
        cx.with_child(core::convert::identity, forward, |ccx| link.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        let link = &mut self.link;
        cx.with_child(core::convert::identity, forward, |ccx| link.on_timer(id, ccx));
    }
}
