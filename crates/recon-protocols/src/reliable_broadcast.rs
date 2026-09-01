//! Regular reliable broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.2 and Algorithm 3.3 ("Eager Reliable Broadcast").
//!
//! **Status: transcription. Space: unbounded.** `delivered` grows with every message delivered,
//! and the book omits its collection deliberately. Deployable once it is windowed — which weakens
//! no duplication to hold within the retention window. See `docs/bounded-space.md`.
//!
//! Best-effort broadcast promises nothing when the sender crashes partway through: some processes
//! deliver, others do not, and they disagree for ever. This layer adds **agreement** — if any
//! correct process delivers a message, every correct process eventually does — by having every
//! process relay each message the first time it delivers it. The redundancy therefore lives at
//! the other processes, which is why this guarantee survives the sender's crash where best-effort
//! broadcast's does not.
//!
//! ```text
//! upon event ⟨ rb, Broadcast | m ⟩ do
//!     trigger ⟨ beb, Broadcast | [DATA, self, m] ⟩;
//!
//! upon event ⟨ beb, Deliver | p, [DATA, s, m] ⟩ do
//!     if m ∉ delivered then
//!         delivered := delivered ∪ {m};
//!         trigger ⟨ rb, Deliver | s, m ⟩;
//!         trigger ⟨ beb, Broadcast | [DATA, s, m] ⟩;
//! ```
//!
//! The relay is unconditional on first delivery — the book's *eager* scheme. Algorithm 3.2, the
//! lazy variant, relays only when a perfect failure detector reports the sender crashed; that
//! abstraction below does not exist here, and eager needs no failure detector at all. It pays
//! for that in messages.
//!
//! Scope tags, in the notation of `docs/scope-annotated-modules.md`:
//!
//! ```text
//! RB1 [always]       Validity
//! RB2 [incarnation]  No duplication  — the delivered set is volatile
//! RB3 [always]       No creation
//! RB4 [always]       Agreement       — bridged by redundancy at the other processes
//! ```
//!
//! **Two departures from the page**, both for reasons already met lower in the stack:
//!
//! - The book deduplicates on message content, assuming messages are unique across senders. Here
//!   each broadcast carries an identifier — its originator and a per-sender sequence number — and
//!   deduplication is on that, so identical content broadcast twice is delivered twice.
//! - `⟨rb, Init⟩` is not a separate event; `new` establishes the same state.
//!
//! # Over a link that reports scope boundaries
//!
//! `L` is a parameter, so this one module is also what `session_reliable_broadcast` used to be.
//! Algorithm 3.3 is unchanged; what changes is the scope its agreement holds within.
//!
//! Over a perfect link a relay always arrives, because the link retransmits until it does. Over a
//! session link it may not, and this layer has nothing with which to retry:
//!
//! - It relays **once**, on first delivery. That is what makes eager reliable broadcast eager.
//! - It keeps `delivered` as a set of **identifiers**, not payloads — so even knowing a relay was
//!   lost, it has no copy to send again. Retaining payloads would be state growing with messages,
//!   which `docs/bounded-space.md` forbids without a window.
//! - It is **fail-silent**. Algorithm 3.3 uses no failure detector, so it cannot conclude that a
//!   process is gone and stop expecting to reach it.
//!
//! So when a relay is lost to a scope ending, nothing retries and nothing gives up. This layer
//! cannot bridge, so it propagates: the boundary is reported upward in its own `Ind` rather than
//! absorbed, which is what `docs/conditional-guarantees.md` requires of a layer in that position.
//!
//! ```text
//! RB1 [session]       Validity
//! RB2 [incarnation]   No duplication — `delivered` is volatile, so a restart forgets it
//! RB3 [always]        No creation
//! RB4 [session]       Agreement — within the scopes carrying the relay, and not across one
//! ```
//!
//! `RB2` is `[incarnation]` for the reason `docs/scope-annotated-modules.md` gives as Corollary
//! 7.2, and by the same argument: the redundancy that would have to survive is `delivered`, that
//! set is held in memory, and the boundary it cannot cross is this process's own `⟨Init⟩`. A
//! recipient that restarts and is then relayed a message it had already delivered — which the
//! eager relay of a peer that did *not* restart will happily do — delivers it a second time.
//! `[always]` would be the claim that a volatile set survives a crash.
//!
//! This is not a defect to be fixed here. It is the honest reading of Algorithm 3.3 on a link that
//! can lose a suffix, and it is exactly what uniform reliable broadcast does not share — that one
//! has a failure detector, and between reconnection and accusation it has no third outcome.
//! Reading the two together is the sharpest available argument for why uniform reliable broadcast
//! needs a detector at all.

use core::time::Duration;
use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use crate::link::VolatileLink;
use crate::perfect_link::PerfectLink;

/// Names one broadcast uniquely: who originated it, and their sequence number for it.
///
/// A relayed message must still be attributed to its originator, so the identifier travels with
/// the message rather than being derived from whoever most recently sent it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BroadcastId {
    pub origin: NodeId,
    pub seq: u64,
}

/// What this layer adds to the wire: the originator, and the payload.
///
/// The first header contributed above the perfect link's. It exists because a relayer is not the
/// sender, and without it a recipient could not tell who originated what it received.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data<P> {
    pub id: BroadcastId,
    pub payload: P,
}

/// What a link beneath this layer must carry.
///
/// A caller supplying their own link needs this and should not have to read the source to find it:
/// the payload is wrapped as `Data` — the payload with the originator and sequence number Algorithm 3.3 needs, so the link carries `Carried<P>` rather than `P`.
pub type Carried<P> = Data<P>;

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// `from` is the process that *originated* the message, never the one that relayed it.
    Deliver { from: NodeId, msg: P },
    /// The scope with `peer` ended at `epoch`.
    ///
    /// RB4's agreement is scoped to the sessions that carried the relay: this layer relays once,
    /// on first receipt, and never again, so a relay lost to an ending is lost for good. It holds
    /// no redundancy that outlives the scope, so it cannot bridge and must propagate.
    /// Raised only over a link that reports boundaries.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// The wire type: this layer's data, carried by best-effort broadcast.
pub type Wire<P, L = PerfectLink<Data<P>>> = <BestEffortBroadcast<Data<P>, L> as Protocol>::Msg;

/// Broadcast with agreement, over best-effort broadcast.
#[derive(Debug)]
pub struct ReliableBroadcast<P: Clone, L: VolatileLink<Data<P>> = PerfectLink<Data<P>>> {
    me: NodeId,
    seq: u64,
    delivered: BTreeSet<BroadcastId>,
    beb: Child<BestEffortBroadcast<Data<P>, L>>,
}

impl<P: Clone> ReliableBroadcast<P, PerfectLink<Data<P>>> {
    /// Reliable broadcast for process `me` among `peers`, over links retransmitting every
    /// `interval`.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, interval: Duration) -> Self {
        Self::with_link(me, peers, PerfectLink::new(me, interval))
    }
}

impl<P: Clone, L: VolatileLink<Data<P>>> ReliableBroadcast<P, L> {
    /// Reliable broadcast for process `me` among `peers`, over the link supplied.
    pub fn with_link(me: NodeId, peers: impl IntoIterator<Item = NodeId>, link: L) -> Self {
        ReliableBroadcast {
            me,
            seq: 0,
            delivered: BTreeSet::new(),
            beb: Child::new(BestEffortBroadcast::with_link(me, peers, link)),
        }
    }

    /// How many distinct broadcasts this process has delivered upward.
    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    /// Whether this process has already delivered `id`.
    pub fn has_delivered(&self, id: BroadcastId) -> bool {
        self.delivered.contains(&id)
    }
}

impl<P: Clone, L> ReliableBroadcast<P, L>
where
    L: VolatileLink<Data<P>>,
{
    /// Run the child, then act on whatever it reported.
    fn with_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut BestEffortBroadcast<Data<P>, L>,
            &mut ProtoCx<'_, BestEffortBroadcast<Data<P>, L>>,
        ),
    ) {
        let mut inds = self.beb.run(cx, core::convert::identity, f);
        for ind in inds.drain(..) {
            match ind {
                beb::Ind::Deliver { msg: Data { id, payload }, .. } => {
                    self.on_beb_deliver(id, payload, cx)
                }
                // This layer relays once, on first receipt, and never again, so it holds no
                // redundancy outliving the scope and cannot repair what an ending lost. It
                // propagates instead — which is the whole of what
                // `docs/conditional-guarantees.md` requires of a layer that cannot bridge.
                beb::Ind::SessionEnded { peer, epoch } => {
                    cx.indicate(Ind::SessionEnded { peer, epoch })
                }
                beb::Ind::SessionEstablished { peer, epoch } => {
                    cx.indicate(Ind::SessionEstablished { peer, epoch })
                }
            }
        }
        self.beb.reclaim(inds);
    }

    /// Algorithm 3.3's second handler: deliver once, then relay once.
    fn on_beb_deliver(&mut self, id: BroadcastId, payload: P, cx: &mut ProtoCx<'_, Self>) {
        if !self.delivered.insert(id) {
            return; // already seen — neither delivered again nor relayed again
        }
        // Attributed to the originator, not to whoever relayed it here.
        cx.indicate(Ind::Deliver { from: id.origin, msg: payload.clone() });
        self.relay(Data { id, payload }, cx);
    }

    /// Re-broadcast a message so that it survives the originator's crash.
    ///
    /// This re-enters the child while its inbox is out on loan, so `run` hands back a fresh one.
    /// Best-effort broadcast turns a request into sends and timers only — a message to this process
    /// travels through the links like any other and arrives later — so no indication can be raised
    /// here. The assertion records that reasoning rather than trusting it.
    fn relay(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        let inds = self.beb.run(cx, core::convert::identity, |beb, ccx| {
            beb.on_cmd(beb::Cmd::Broadcast(data), ccx)
        });
        debug_assert!(
            inds.is_empty(),
            "relaying must not deliver synchronously; if it does, on_beb_deliver can recurse"
        );
        self.beb.reclaim(inds);
    }
}

impl<P: Clone, L> Protocol for ReliableBroadcast<P, L>
where
    L: VolatileLink<Data<P>>,
{
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P, L>;
    /// Whatever the link's guarantees are conditional on. RB4 is scoped to the sessions that
    /// carried the relay, and this layer cannot bridge one ending.
    type Scope = L::Scope;
    type Note = crate::Note;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Broadcast(msg): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let data = Data { id: BroadcastId { origin: self.me, seq: self.seq }, payload: msg };
        self.with_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::Broadcast(data), ccx));
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P, L>, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_timer(id, ccx));
    }

    /// Hand the scope ending down. This layer cannot bridge one, and the default would drop it.
    fn on_scope_event(&mut self, scope: L::Scope, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_scope_event(scope, ccx));
    }
}
