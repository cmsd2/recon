//! Consensus-based total-order broadcast.
//!
//! **Status: transcription. Space: unbounded — `unordered`, `delivered` and the family of consensus
//! instances all grow with the number of entries handled.** That is the page, and
//! `docs/bounded-space.md` is explicit that inheriting the book's omissions is correct of a
//! transcription and disqualifying of an implementation. Bounding any of them weakens a guarantee to
//! a scope and belongs to a change with a proposal.
//!
//! Cachin, Guerraoui & Rodrigues, Module 6.1 (`TotalOrderBroadcast`) and Algorithm 6.1, quoted from
//! the book:
//!
//! ```text
//! Algorithm 6.1: Consensus-Based Total-Order Broadcast
//! Implements: TotalOrderBroadcast, instance tob.
//! Uses:
//!     ReliableBroadcast, instance rb;
//!     Consensus (multiple instances).
//!
//! upon event ⟨ tob, Init ⟩ do
//!     unordered := ∅;
//!     delivered := ∅;
//!     round := 1;
//!     wait := FALSE;
//!
//! upon event ⟨ tob, Broadcast | m ⟩ do
//!     trigger ⟨ rb, Broadcast | m ⟩;
//!
//! upon event ⟨ rb, Deliver | p, m ⟩ do
//!     if m ∉ delivered then
//!         unordered := unordered ∪ {(p, m)};
//!
//! upon unordered ≠ ∅ ∧ wait = FALSE do
//!     wait := TRUE;
//!     Initialize a new instance c.round of consensus;
//!     trigger ⟨ c.round, Propose | unordered ⟩;
//!
//! upon event ⟨ c.r, Decide | decided ⟩ such that r = round do
//!     forall (s, m) ∈ sort(decided) do     // by the order in the resulting sorted list
//!         trigger ⟨ tob, Deliver | s, m ⟩;
//!     delivered := delivered ∪ decided;
//!     unordered := unordered \ decided;
//!     round := round + 1;
//!     wait := FALSE;
//! ```
//!
//! The shape is one round at a time: everything reliable broadcast has delivered and this process
//! has not yet ordered is proposed as a *set*, consensus agrees on a set, and every process turns
//! that same set into the same sequence by sorting it. Ordering is therefore agreed without anyone
//! communicating about order at all — the sort does that work, which is why it must be deterministic.
//!
//! # Departures from the page
//!
//! - **A read.** The port this satisfies offers [`crate::total_order_log::TotalOrderLog::read`],
//!   which the book's abstraction does not: its clients observe deliveries. The algorithm already
//!   maintains `delivered`, so the read exposes what the page keeps and does not offer, served
//!   locally. See the port's own documentation.
//!
//! - **Consensus instances are held explicitly, keyed by round.** The page writes `c.round` and
//!   `⟨ c.r, Decide ⟩`, so instances are a family addressed by round; the book's runtime routes to
//!   them and this one does not. They are created on demand — including for a round this process has
//!   not reached, which is what lets a peer that is ahead make progress — and never pruned, as the
//!   page has them. Creation runs the instance's `⟨ Init ⟩` before the event that provoked it:
//!   "Initialize a new instance c.round" is an event the book's runtime delivers, and skipping it
//!   leaves the instance's failure detector without its timers — which no fault-free run notices,
//!   because deciding under the initial epoch never consults the detector. A crash is then never
//!   detected and the survivors stall, which is what the suite's crash property caught.
//!
//! - **The conditional event handler is discharged here.** `such that r = round` is not a guard that
//!   discards. The book states its meaning: "An algorithm that uses conditional event handlers
//!   relies on the run-time system to buffer external events until the condition on internal
//!   variables becomes satisfied." `Cx` has no such facility, so a decision for a round this process
//!   has not reached is **held** in `decisions` and acted on when `round` catches up.
//!   [`crate::leader_driven_consensus`]'s `pending` is the same pattern, for the same reason.
//!
//! - **A consensus message carries its round.** The page addresses instances; nothing on this wire
//!   does. Unlike [`crate::epoch_consensus`], whose instance stamps its own messages because the
//!   epoch is its identity, the round is *this* layer's concept and not the consensus's — so this
//!   layer stamps, and the stamp is the one thing it cannot delegate.
//!
//! - **`unordered` is deduplicated on the pair `(p, m)`**, not on `m` alone. The page's
//!   `if m ∉ delivered` reads against a `delivered` that holds pairs, so one of the two is loose;
//!   the pair is what makes `unordered \ decided` well defined, and it is what is used here. The
//!   consequence, which the page shares: one process appending the same value twice contributes one
//!   entry.
//!
//! - **The consensus beneath is not a type parameter, and its link is fixed.** Every other composing
//!   layer here takes its child as a parameter with a default. This one cannot for the consensus:
//!   instances are created *at run time*, one per round, so the layer would need a link **factory**
//!   rather than a link — a runtime indirection the static composition model exists to avoid. The
//!   reliable broadcast keeps its parameter, because there is exactly one of it and the caller
//!   supplies it once. Revisit if a stack ever wants rounds agreed over something else.
//!
//! - **The sort is a `BTreeSet`'s own order.** Proposing an ordered set means `sort(decided)` is
//!   iteration, and every process computes the same sequence because `Ord` is a function of the
//!   values rather than of anything local. The guard forbidding hash-keyed maps in these three
//!   crates exists for the converse reason: an iteration order that varies per process is exactly
//!   what would break agreement here.

use recon_core::{Child, NodeId, Position, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::Timing;
use crate::flooding_consensus::{self as fc, FloodingConsensus};
use crate::link::{Boundary, VolatileLink};
use crate::perfect_link::PerfectLink;
use crate::reliable_broadcast::{self as rb, ReliableBroadcast};
use crate::total_order_log::{LogInd, TotalOrderLog};

/// One entry with the process that appended it — the page's `(s, m)`.
///
/// `Ord` is what makes the ordering agreed: consensus decides a *set*, and every process must turn
/// that set into the same sequence with no further communication.
pub type Slot<V> = (NodeId, V);

/// What a round proposes and decides: the set of entries not yet ordered.
pub type Batch<V> = BTreeSet<Slot<V>>;

/// What reliable broadcast carries for this layer.
pub type Carried<V> = rb::Carried<V>;

/// What the consensus beneath carries for this layer.
pub type ConsensusCarried<V> = fc::Flood<Batch<V>>;

/// The consensus one round runs. Not a type parameter — see the module's departures.
pub type Consensus<V> = FloodingConsensus<Batch<V>, PerfectLink<ConsensusCarried<V>>>;

/// This layer's messages: the broadcast's, and a consensus instance's stamped with its round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<R, C> {
    /// A reliable broadcast message.
    Broadcast(R),
    /// A consensus message, stamped with the round whose instance it belongs to.
    ///
    /// The page addresses instances as `c.round`; nothing on this wire does. The stamp is this
    /// layer's rather than the consensus's, because the round is this layer's concept — which is why
    /// it cannot live in the child as [`crate::epoch_consensus::Tagged`]'s does.
    Consensus { round: u64, msg: C },
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<V> {
    /// `⟨ tob, Broadcast | m ⟩`, in the port's terms: append `v` to the log.
    Append(V),
    /// Read the ordered sequence from `from` onwards. The page has no such request; see the
    /// module's departures.
    Read { from: Position },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<V> {
    /// `⟨ tob, Deliver | s, m ⟩`, with the position it took in the agreed sequence.
    Ordered { position: Position, from: NodeId, value: V },
    /// The answer to a [`Cmd::Read`].
    Contents { from: Position, entries: Vec<V> },
    /// The scope with `peer` ended at `epoch`, as reliable broadcast beneath reported it.
    ///
    /// Propagated rather than absorbed: this layer's only redundancy is the broadcast's, which
    /// relays once and cannot resend across an ending, so it cannot bridge either. Raised only over
    /// a link that reports boundaries, which the default stack's perfect link does not.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// A totally ordered log, agreed by one consensus instance per round.
pub struct ConsensusBasedTotalOrderBroadcast<
    V: Clone + Ord,
    L: VolatileLink<Carried<V>> = PerfectLink<Carried<V>>,
> {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    timing: Timing,
    /// `unordered` — delivered by the broadcast beneath, not yet ordered.
    unordered: Batch<V>,
    /// `delivered`, as the agreed sequence. The page writes a set; the order is the whole point, so
    /// it is kept as one and `ordered` is the membership test the page's `∉` needs.
    delivered: Vec<Slot<V>>,
    ordered: BTreeSet<Slot<V>>,
    /// `round`.
    round: u64,
    /// `wait`.
    wait: bool,
    rb: Child<ReliableBroadcast<V, L>>,
    /// `c.r` for every `r` this process has started or been sent a message for. A family, as the
    /// page has it — never one instance replaced, because no round supersedes another.
    consensus: BTreeMap<u64, Child<Consensus<V>>>,
    /// Decisions for rounds not yet reached, held rather than discarded — the conditional event
    /// handler `such that r = round`, discharged here because `Cx` cannot buffer on a condition.
    decisions: BTreeMap<u64, Batch<V>>,
}

impl<V: Clone + Ord> ConsensusBasedTotalOrderBroadcast<V> {
    /// A totally ordered log among `peers`, over perfect links.
    ///
    /// `timing` is the consensus beneath's: its `detect_after` must exceed `heartbeat` plus the
    /// network's delivery bound, or the perfect failure detector under flooding consensus accuses a
    /// correct process and agreement can break.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, timing: Timing) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        ConsensusBasedTotalOrderBroadcast {
            me,
            peers: peers.clone(),
            timing,
            unordered: BTreeSet::new(),
            delivered: Vec::new(),
            ordered: BTreeSet::new(),
            round: 1,
            wait: false,
            rb: Child::new(ReliableBroadcast::new(me, peers, timing.retransmit)),
            consensus: BTreeMap::new(),
            decisions: BTreeMap::new(),
        }
    }
}

impl<V: Clone + Ord, L: VolatileLink<Carried<V>>> ConsensusBasedTotalOrderBroadcast<V, L> {
    /// The agreed sequence as this process holds it.
    pub fn entries(&self) -> &[Slot<V>] {
        &self.delivered
    }

    /// How many entries this process has ordered.
    pub fn len(&self) -> usize {
        self.delivered.len()
    }

    pub fn is_empty(&self) -> bool {
        self.delivered.is_empty()
    }

    /// The round this process is running.
    pub fn round(&self) -> u64 {
        self.round
    }

    /// How many consensus instances are held. Grows with rounds, and is never pruned — see the
    /// module's space statement.
    pub fn instances(&self) -> usize {
        self.consensus.len()
    }

    /// `upon unordered ≠ ∅ ∧ wait = FALSE do` — a standing condition, re-evaluated whenever either
    /// could have changed.
    fn maybe_propose(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.unordered.is_empty() || self.wait {
            return;
        }
        self.wait = true;
        let round = self.round;
        let batch = self.unordered.clone();
        self.through_consensus(round, cx, |c, ccx| c.on_cmd(fc::Cmd::Propose(batch), ccx));
    }

    /// `upon event ⟨ c.r, Decide | decided ⟩ such that r = round`, with the buffering the book's
    /// run-time system would have done.
    fn drain_decisions(&mut self, cx: &mut ProtoCx<'_, Self>) {
        while let Some(decided) = self.decisions.remove(&self.round) {
            // `forall (s, m) ∈ sort(decided)` — iteration of an ordered set *is* the sort, and every
            // process computes the same one because `Ord` reads only the values.
            for (from, value) in &decided {
                if self.ordered.contains(&(*from, value.clone())) {
                    continue;
                }
                let position = Position(self.delivered.len() as u64);
                self.delivered.push((*from, value.clone()));
                self.ordered.insert((*from, value.clone()));
                cx.indicate(Ind::Ordered { position, from: *from, value: value.clone() });
            }
            for slot in &decided {
                self.unordered.remove(slot);
            }
            self.round += 1;
            self.wait = false;
        }
        self.maybe_propose(cx);
    }

    fn through_rb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut ReliableBroadcast<V, L>, &mut ProtoCx<'_, ReliableBroadcast<V, L>>),
    ) {
        let mut inds = self.rb.run(cx, Wire::Broadcast, f);
        for ind in inds.drain(..) {
            match ind {
                // `upon event ⟨ rb, Deliver | p, m ⟩ do if (p, m) ∉ delivered then …`
                rb::Ind::Deliver { from, msg } => {
                    let slot = (from, msg);
                    if !self.ordered.contains(&slot) {
                        self.unordered.insert(slot);
                    }
                }
                // Reliable broadcast cannot bridge a scope ending and propagates; neither can this
                // layer, whose only redundancy is the broadcast's. Over a perfect link the arm is
                // unreachable — see `link.rs`.
                rb::Ind::SessionEnded { peer, epoch } => {
                    cx.indicate(Ind::SessionEnded { peer, epoch });
                }
                rb::Ind::SessionEstablished { peer, epoch } => {
                    cx.indicate(Ind::SessionEstablished { peer, epoch });
                }
            }
        }
        self.rb.reclaim(inds);
        self.maybe_propose(cx);
    }

    /// Run `f` against round `r`'s instance, creating it if this process has not started it.
    fn through_consensus(
        &mut self,
        r: u64,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut Consensus<V>, &mut ProtoCx<'_, Consensus<V>>),
    ) {
        // "Initialize a new instance c.round of consensus" — an event, which runs before whatever
        // provoked the creation. See the module's departures for what skipping it cost.
        let created = !self.consensus.contains_key(&r);
        let entry = self.consensus.entry(r).or_insert_with(|| {
            Child::new(FloodingConsensus::new(
                self.me,
                self.peers.clone(),
                self.timing.retransmit,
                self.timing.heartbeat,
                self.timing.detect_after,
            ))
        });
        let mut inds = entry.run(
            cx,
            |m| Wire::Consensus { round: r, msg: m },
            |c, ccx| {
                if created {
                    c.on_init(ccx);
                }
                f(c, ccx)
            },
        );
        for fc::Ind::Decide(decided) in inds.drain(..) {
            self.decisions.entry(r).or_insert(decided);
        }
        if let Some(child) = self.consensus.get_mut(&r) {
            child.reclaim(inds);
        }
        self.drain_decisions(cx);
    }
}

impl<V: Clone + Ord, L: VolatileLink<Carried<V>>> Protocol
    for ConsensusBasedTotalOrderBroadcast<V, L>
{
    type Cmd = Cmd<V>;
    type Ind = Ind<V>;
    type Msg = Wire<rb::Wire<V, L>, <Consensus<V> as Protocol>::Msg>;
    type Scope = core::convert::Infallible;
    type Note = crate::Note;
    /// Keeps nothing durably. The fail-recovery variant is the one that does.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            // `upon event ⟨ tob, Broadcast | m ⟩ do trigger ⟨ rb, Broadcast | m ⟩`
            Cmd::Append(v) => self.through_rb(cx, |r, ccx| r.on_cmd(rb::Cmd::Broadcast(v), ccx)),
            // The departure. Served from this process's own sequence, so it may lag.
            Cmd::Read { from } => {
                let entries: Vec<V> =
                    self.delivered.iter().skip(from.0 as usize).map(|(_, v)| v.clone()).collect();
                cx.indicate(Ind::Contents { from, entries });
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Broadcast(m) => self.through_rb(cx, |r, ccx| r.on_msg(from, m, ccx)),
            // Routed to the round's own instance, creating it if this process has not started that
            // round — which is what lets a peer that is ahead make progress.
            Wire::Consensus { round, msg } => {
                self.through_consensus(round, cx, |c, ccx| c.on_msg(from, msg, ccx));
            }
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_rb(cx, |r, ccx| r.on_timer(id, ccx));
        let rounds: Vec<u64> = self.consensus.keys().copied().collect();
        for r in rounds {
            self.through_consensus(r, cx, |c, ccx| c.on_timer(id, ccx));
        }
    }

    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.through_rb(cx, |r, ccx| r.on_init(ccx));
    }
}

impl<V: Clone + Ord, L: VolatileLink<Carried<V>>> TotalOrderLog<V>
    for ConsensusBasedTotalOrderBroadcast<V, L>
{
    fn append(value: V) -> Cmd<V> {
        Cmd::Append(value)
    }

    fn read(from: Position) -> Cmd<V> {
        Cmd::Read { from }
    }

    fn classify(ind: Ind<V>) -> LogInd<V> {
        match ind {
            Ind::Ordered { position, from, value } => LogInd::Ordered { position, from, value },
            Ind::Contents { from, entries } => LogInd::Contents { from, entries },
            Ind::SessionEnded { peer, epoch } => LogInd::Boundary(Boundary::Ended { peer, epoch }),
            Ind::SessionEstablished { peer, epoch } => {
                LogInd::Boundary(Boundary::Established { peer, epoch })
            }
        }
    }
}
