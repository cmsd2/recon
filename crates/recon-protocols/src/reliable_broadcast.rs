//! Regular reliable broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.2 and Algorithm 3.3 ("Eager Reliable Broadcast").
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
//! rung of the ladder does not exist here, and eager needs no failure detector at all. It pays
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

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};

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
}

/// Timers, which are the child's re-wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Broadcast(beb::Timer),
}

/// The wire type: this layer's data, carried by best-effort broadcast.
pub type Wire<P> = <BestEffortBroadcast<Data<P>> as Protocol>::Msg;

/// Broadcast with agreement, over best-effort broadcast.
#[derive(Debug)]
pub struct ReliableBroadcast<P> {
    me: NodeId,
    seq: u64,
    delivered: BTreeSet<BroadcastId>,
    beb: BestEffortBroadcast<Data<P>>,
    /// Indications the child raised, awaiting this layer's attention. Reused across events.
    inbox: Vec<beb::Ind<Data<P>>>,
    /// A second buffer for the relay, which by construction produces no indications. Kept
    /// separate so the assertion below is meaningful rather than accidental.
    relay_inbox: Vec<beb::Ind<Data<P>>>,
}

impl<P> ReliableBroadcast<P> {
    /// Reliable broadcast for process `me` among `peers`, over links retransmitting every
    /// `interval`.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, interval: Duration) -> Self {
        ReliableBroadcast {
            me,
            seq: 0,
            delivered: BTreeSet::new(),
            beb: BestEffortBroadcast::new(me, peers, interval),
            inbox: Vec::new(),
            relay_inbox: Vec::new(),
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

impl<P: Clone> ReliableBroadcast<P> {
    /// Run the child, then act on whatever it reported.
    fn with_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut BestEffortBroadcast<Data<P>>,
            &mut ProtoCx<'_, BestEffortBroadcast<Data<P>>>,
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
            let beb::Ind::Deliver { msg: Data { id, payload }, .. } = ind;
            self.on_beb_deliver(id, payload, cx);
        }
        self.inbox = inbox;
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
    /// This re-enters the child, which is why it uses its own buffer. Best-effort broadcast turns
    /// a request into sends and timers only — a message to this process travels through the links
    /// like any other and arrives later — so no indication can be raised here. The assertion
    /// records that reasoning rather than trusting it.
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

impl<P: Clone> Protocol for ReliableBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    type Timer = Timer;

    fn on_cmd(&mut self, Cmd::Broadcast(msg): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let data = Data { id: BroadcastId { origin: self.me, seq: self.seq }, payload: msg };
        self.with_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::Broadcast(data), ccx));
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, Timer::Broadcast(token): Timer, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_timer(token, ccx));
    }
}
