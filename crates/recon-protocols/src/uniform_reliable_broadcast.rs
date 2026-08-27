//! Uniform reliable broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.3 and Algorithm 3.4 ("All-Ack Uniform Reliable
//! Broadcast").
//!
//! **Status: transcription. Space: unbounded.** `pending` holds payloads, `ack` holds a process
//! set per message, and `delivered` grows without limit. Deployable once collected — and this
//! layer already computes the predicate it would need, since `correct ⊆ ack[m]` is a stability
//! test: a message every correct process has seen can be dropped from `pending` and `ack` the
//! moment it is delivered. See `docs/bounded-space.md`.
//!
//! Reliable broadcast guarantees agreement only among *correct* processes. A process that
//! delivers a message and then crashes may leave the survivors never delivering it — and if that
//! delivery had any external effect, the divergence cannot be repaired from above. Uniform
//! agreement quantifies over *any* process: delivered by anyone at all, correct or not, means
//! eventually delivered by everyone correct.
//!
//! It is bought by waiting. A message is delivered only once every process still believed correct
//! has been seen to acknowledge it, so nobody can deliver something the others have not yet seen.
//!
//! ```text
//! upon event ⟨ urb, Broadcast | m ⟩ do
//!     pending := pending ∪ {(self, m)};
//!     trigger ⟨ beb, Broadcast | [DATA, self, m] ⟩;
//!
//! upon event ⟨ beb, Deliver | p, [DATA, s, m] ⟩ do
//!     ack[m] := ack[m] ∪ {p};
//!     if (s, m) ∉ pending then
//!         pending := pending ∪ {(s, m)};
//!         trigger ⟨ beb, Broadcast | [DATA, s, m] ⟩;
//!
//! upon event ⟨ P, Crash | p ⟩ do
//!     correct := correct \ {p};
//!
//! function candeliver(m) is  return correct ⊆ ack[m];
//!
//! upon exists (s, m) ∈ pending such that candeliver(m) ∧ m ∉ delivered do
//!     delivered := delivered ∪ {m};
//!     trigger ⟨ urb, Deliver | s, m ⟩;
//! ```
//!
//! # This layer depends on a timing assumption, and cannot detect its failure
//!
//! Uniform agreement holds only while the failure detector is *accurate*, which holds only while
//! the network delivers within a known bound. A wrongly accused process is removed from `correct`,
//! `candeliver` is satisfied too early, and a message can be delivered by some processes and not
//! others.
//!
//! That dependency is stated here rather than expressed with the scope annotation of
//! `docs/scope-annotated-modules.md`, and deliberately: a scope must have a boundary the module
//! can observe. This one has none. Synchrony failing arrives here as the detector reporting a
//! crash, indistinguishable from the detector being right. An assumption a layer rests on but
//! cannot detect is not a scope — tagging it would create an obligation no implementation could
//! discharge and no test could exercise.
//!
//! # Departures from the page
//!
//! - `ack` and `delivered` are keyed by an identifier carrying the originator and a per-sender
//!   sequence number, not by message content. The book's `ack[m]` assumes messages are unique
//!   across senders; identical content broadcast twice must be delivered twice.
//! - Two children both send, so this layer's wire type is an enum distinguishing a broadcast
//!   payload from a heartbeat. It is the first multiplexing in the stack, and it is typed: a
//!   mis-wiring is a compile error rather than a silently undelivered message.
//! - `⟨urb, Init⟩` is not a separate event. `new` establishes the state, and [`Cmd::Start`] begins
//!   failure detection.
//! - Neither `ack` nor `pending` is garbage collected, as in the book. Long runs grow.
//!
//! # Over a link that reports scope boundaries
//!
//! `L` is a parameter, so this one module is also what `session_uniform_reliable_broadcast` used
//! to be. Algorithm 3.4 gains **one** clause and nothing else changes:
//!
//! ```text
//! upon event ⟨ SessionEstablished | q ⟩ do
//!     forall (s, m) ∈ pending do
//!         trigger ⟨ beb, SendTo | q, [DATA, s, m] ⟩;
//! ```
//!
//! It reuses `pending`, which the algorithm already maintains, and `SendTo`, which is a narrowing
//! of an existing action rather than a new communication step.
//!
//! Two things about that clause are deliberate and neither is what one would write first. It is
//! **unconditional**, not filtered by `q ∉ ack[m]`: the filtered version deadlocks, for the reason
//! set out at `resend_to`, which is where a test found it. And it is **directed** at the peer
//! whose scope returned rather than broadcast to everyone, because the ending was per peer and so
//! is the repair. Nothing is attempted on the *ending* itself: the peer is unreachable at that
//! moment and anything sent would be discarded.
//!
//! ```text
//! URB1 [always]       Validity — conditional on the two mechanisms below
//! URB2 [incarnation]  No duplication — `delivered` is volatile, so a restart forgets it
//! URB3 [always]       No creation
//! URB4 [always]       Uniform agreement — conditional on the two mechanisms below
//! ```
//!
//! `URB2` is `[incarnation]` by `docs/scope-annotated-modules.md` Corollary 7.2: the set that
//! would have to survive is `delivered`, it is held in memory, and the boundary it cannot cross is
//! this process's own `⟨Init⟩`.
//!
//! `URB1` and `URB4` are `[always]` only because between two mechanisms no third outcome is left —
//! the scope comes back and the resend repairs it, or the peer never returns and the detector's
//! timeout drops it from `correct`. Each carries a condition that is an assumption rather than a
//! property of this code. The reconnection path needs the peer to be reachable again; the
//! accusation path needs the detector's synchrony assumption, and `perfect_failure_detector` is
//! explicit that outside a synchronous system it accuses correct processes. **Both failing at once
//! is a permanent split**: each side of a partition accuses the other, each has `correct ⊆ ack[m]`
//! satisfied among itself, and both deliver — not uniform agreement failing on a technicality but
//! two disjoint sets of processes proceeding as though the other did not exist.
//! [`crate::majority_ack_uniform_reliable_broadcast`] cannot suffer that and blocks instead; that
//! difference is what a quorum buys and a detector costs.
//!
//! Read against reliable broadcast, which has neither mechanism and whose agreement is therefore
//! scoped, this is the clearest statement of what a failure detector buys.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use crate::link::VolatileLink;
use crate::perfect_failure_detector::{self as pfd, Heartbeat, PerfectFailureDetector};
use crate::perfect_link::{self as pl, PerfectLink};

/// Names one broadcast uniquely: who originated it, and their sequence number for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BroadcastId {
    pub origin: NodeId,
    pub seq: u64,
}

/// What this layer adds to a broadcast payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data<P> {
    pub id: BroadcastId,
    pub payload: P,
}

/// What best-effort broadcast puts on the wire for this layer's payloads.
///
/// Written concretely rather than as `<BestEffortBroadcast<Data<P>> as Protocol>::Msg`: the
/// projection is only well-formed where `Data<P>: Clone`, which would push that bound onto every
/// use of [`Wire`]. The assertion below keeps the two from drifting apart.
pub type BebMsg<P> = pl::Wire<Data<P>>;

const _: () = {
    /// Fails to compile if best-effort broadcast ever puts something else on the wire.
    fn _beb_msg_is_what_we_say_it_is<P: Clone>(
        m: BebMsg<P>,
    ) -> <BestEffortBroadcast<Data<P>> as Protocol>::Msg {
        m
    }
};

/// The wire type, multiplexing the two children.
///
/// The first multiplexing in this stack. It is an enum rather than a keyed registry, so a message
/// routed to the wrong child cannot compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<M> {
    Broadcast(M),
    Detector(Heartbeat),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// `from` is the process that originated the message, never a relayer.
    Deliver { from: NodeId, msg: P },
    /// The scope with `peer` ended at `epoch`.
    ///
    /// Raised only over a link that reports boundaries. Unlike reliable broadcast, this layer
    /// *can* bridge one — it holds `pending` until every correct process has acknowledged, so a
    /// re-establishment lets it resend. It reports the ending anyway, because the layer above may
    /// have its own reason to know, and because a guarantee that lapsed and was repaired is not
    /// the same as one that never lapsed.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// Broadcast with uniform agreement, over best-effort broadcast and a failure detector.
#[derive(Debug)]
pub struct UniformReliableBroadcast<P, L = PerfectLink<Data<P>>> {
    me: NodeId,
    seq: u64,
    /// Every process believed correct. Shrinks on a crash indication and never grows.
    correct: BTreeSet<NodeId>,
    /// Seen and not yet delivered, with the payload kept for delivery.
    pending: BTreeMap<BroadcastId, P>,
    /// Which processes have been seen to acknowledge each message.
    ack: BTreeMap<BroadcastId, BTreeSet<NodeId>>,
    delivered: BTreeSet<BroadcastId>,
    beb: BestEffortBroadcast<Data<P>, L>,
    detector: PerfectFailureDetector,
    beb_inbox: Vec<beb::Ind<Data<P>>>,
    det_inbox: Vec<pfd::Ind>,
    /// A relay re-enters the child while its own inbox is in use, so it needs a buffer of its
    /// own. By construction it stays empty; the assertion in `relay` records why.
    relay_inbox: Vec<beb::Ind<Data<P>>>,
}

impl<P> UniformReliableBroadcast<P, PerfectLink<Data<P>>> {
    /// Broadcast among `members`, which must include `me`.
    ///
    /// `heartbeat` and `detect_after` configure the failure detector; `detect_after` must exceed
    /// `heartbeat` plus the network's delivery bound, or the detector will accuse correct
    /// processes and uniform agreement can break.
    pub fn new(
        me: NodeId,
        members: impl IntoIterator<Item = NodeId>,
        retransmit: Duration,
        heartbeat: Duration,
        detect_after: Duration,
    ) -> Self {
        let link = PerfectLink::new(me, retransmit);
        Self::with_link(me, members, link, heartbeat, detect_after)
    }
}

impl<P, L> UniformReliableBroadcast<P, L> {
    /// Broadcast among `members`, over the link supplied.
    pub fn with_link(
        me: NodeId,
        members: impl IntoIterator<Item = NodeId>,
        link: L,
        heartbeat: Duration,
        detect_after: Duration,
    ) -> Self {
        let mut members: BTreeSet<NodeId> = members.into_iter().collect();
        members.insert(me);
        UniformReliableBroadcast {
            me,
            seq: 0,
            correct: members.clone(),
            pending: BTreeMap::new(),
            ack: BTreeMap::new(),
            delivered: BTreeSet::new(),
            beb: BestEffortBroadcast::with_link(me, members.clone(), link),
            detector: PerfectFailureDetector::new(me, members, heartbeat, detect_after),
            beb_inbox: Vec::new(),
            det_inbox: Vec::new(),
            relay_inbox: Vec::new(),
        }
    }

    /// The processes still believed correct, in a stable order.
    pub fn correct(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.correct.iter().copied()
    }

    /// How many distinct messages have been delivered upward.
    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    /// Messages seen but not yet deliverable.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Which processes have acknowledged `id`, for tests that need to see the condition forming.
    pub fn acknowledged_by(&self, id: BroadcastId) -> impl Iterator<Item = NodeId> + '_ {
        self.ack.get(&id).into_iter().flatten().copied()
    }
}

impl<P: Clone, L> UniformReliableBroadcast<P, L>
where
    L: VolatileLink<Data<P>>,
{
    /// Run the broadcast child, then act on what it reported.
    fn with_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut BestEffortBroadcast<Data<P>, L>,
            &mut ProtoCx<'_, BestEffortBroadcast<Data<P>, L>>,
        ),
    ) {
        let mut inbox = core::mem::take(&mut self.beb_inbox);
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(Wire::Broadcast, &mut inbox, |ccx| f(beb, ccx));
        }
        for ind in inbox.drain(..) {
            match ind {
                beb::Ind::Deliver { from, msg: Data { id, payload } } => {
                    self.on_beb_deliver(from, id, payload, cx)
                }
                beb::Ind::SessionEnded { peer, epoch } => {
                    // Informative only: the peer is unreachable, so nothing can be resent yet.
                    // Attempting the repair here would send into a session already gone.
                    cx.indicate(Ind::SessionEnded { peer, epoch });
                }
                beb::Ind::SessionEstablished { peer, epoch } => {
                    // The moment the repair becomes possible. This layer holds `pending` until
                    // every correct process has acknowledged, so its redundancy outlives the
                    // scope and it *can* bridge — which is why it resends rather than merely
                    // propagating, as reliable broadcast beneath it must.
                    cx.indicate(Ind::SessionEstablished { peer, epoch });
                    self.resend_to(peer, cx);
                }
            }
        }
        self.beb_inbox = inbox;
        self.check_deliverable(cx);
    }

    /// Run the detector child, then act on what it reported.
    /// Resend everything still outstanding to the peer whose scope has just come back.
    ///
    /// Only ever reached over a link that reports boundaries: `Link::classify` yields one only for
    /// a link that can observe one, so over a perfect link this is unreachable rather than merely
    /// unused. A `ScopedLink` bound was the original plan and does not work — the arm that calls
    /// this lives in the `Link` impl, so the tighter bound would fall on every link. What keeps it
    /// honest is the port's own guarantee. See `crate::link` and this change's `design.md`.
    fn resend_to(&mut self, peer: NodeId, cx: &mut ProtoCx<'_, Self>) {
        let outstanding: Vec<Data<P>> = self
            .pending
            .iter()
            .map(|(id, payload)| Data { id: *id, payload: payload.clone() })
            .collect();
        for data in outstanding {
            // Same wire message as a relay, strictly fewer recipients, no new communication step.
            self.with_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::SendTo { to: peer, msg: data }, ccx));
        }
    }

    fn with_detector(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut PerfectFailureDetector, &mut ProtoCx<'_, PerfectFailureDetector>),
    ) {
        let mut inbox = core::mem::take(&mut self.det_inbox);
        {
            let detector = &mut self.detector;
            cx.with_child_consuming(Wire::Detector, &mut inbox, |ccx| f(detector, ccx));
        }
        for ind in inbox.drain(..) {
            let pfd::Ind::Crash { node } = ind;
            // A process never returns to `correct`; the detector's reports are permanent.
            self.correct.remove(&node);
        }
        self.det_inbox = inbox;
        self.check_deliverable(cx);
    }

    /// `upon event ⟨ beb, Deliver | p, [DATA, s, m] ⟩`.
    fn on_beb_deliver(
        &mut self,
        from: NodeId,
        id: BroadcastId,
        payload: P,
        cx: &mut ProtoCx<'_, Self>,
    ) {
        self.ack.entry(id).or_default().insert(from);
        // Relay only on first sight. An identifier determines its payload, so re-inserting the
        // same id cannot change what is pending — the returned Option is only being read to
        // learn whether this was the first time.
        if self.pending.insert(id, payload.clone()).is_none() {
            self.relay(Data { id, payload }, cx);
        }
    }

    /// Re-broadcast, so the message survives its originator's crash.
    fn relay(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        let mut relay_inbox = core::mem::take(&mut self.relay_inbox);
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(Wire::Broadcast, &mut relay_inbox, |ccx| {
                beb.on_cmd(beb::Cmd::Broadcast(data), ccx)
            });
        }
        debug_assert!(
            relay_inbox.is_empty(),
            "relaying must not deliver synchronously; if it does, on_beb_deliver can recurse"
        );
        self.relay_inbox = relay_inbox;
    }

    /// `upon exists (s, m) ∈ pending such that candeliver(m) ∧ m ∉ delivered`.
    ///
    /// The book's last clause is a predicate over state rather than an event, so it is evaluated
    /// wherever its inputs change: `ack` growing on a delivery from below, `correct` shrinking on
    /// a crash. Called from both child helpers for that reason.
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

    /// `correct ⊆ ack[m]` — every process still believed correct has been seen to acknowledge it.
    fn can_deliver(&self, id: BroadcastId) -> bool {
        match self.ack.get(&id) {
            None => false,
            Some(acked) => self.correct.iter().all(|p| acked.contains(p)),
        }
    }
}

impl<P: Clone, L> Protocol for UniformReliableBroadcast<P, L>
where
    L: VolatileLink<Data<P>>,
{
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<L::Msg>;
    /// No scope conditions: this protocol's guarantees do not lapse.
    /// Whatever the link's guarantees are conditional on. This layer bridges an ending rather
    /// than absorbing it, but bridging is not the same as never having lapsed.
    type Scope = L::Scope;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    /// Failure detection begins here, as Module 2.6 has it. It used to need a `Start` command
    /// because there was no init event to hang the detector's first timer on.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.with_detector(cx, |d, ccx| d.on_init(ccx));
    }

    fn on_cmd(&mut self, cmd: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
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

    fn on_msg(&mut self, from: NodeId, msg: Wire<L::Msg>, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Broadcast(m) => self.with_beb(cx, |beb, ccx| beb.on_msg(from, m, ccx)),
            Wire::Detector(h) => self.with_detector(cx, |d, ccx| d.on_msg(from, h, ccx)),
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        // Handed to both children: the identity does not say which registered it, and the one that
        // did not will recognise that and do nothing.
        self.with_beb(cx, |beb, ccx| beb.on_timer(id, ccx));
        self.with_detector(cx, |d, ccx| d.on_timer(id, ccx));
    }

    /// Hand the scope ending down to the children. The trait's default would drop it.
    fn on_scope_event(&mut self, scope: L::Scope, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_scope_event(scope, ccx));
    }
}
