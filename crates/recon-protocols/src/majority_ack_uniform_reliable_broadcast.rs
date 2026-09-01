//! Majority-ack uniform reliable broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.3 and Algorithm 3.5 ("Majority-Ack Uniform Reliable
//! Broadcast").
//!
//! **Status: transcription. Space: unbounded.** `pending`, `ack` and `delivered` grow exactly as
//! in [`crate::uniform_reliable_broadcast`]; removing the detector removes a timing assumption,
//! not the collection debt. See `docs/bounded-space.md`.
//!
//! **Assumption: a correct majority, `N > 2f`.** That is the whole of what this layer rests on. It
//! is a standing property of the deployment rather than a moment-to-moment property of the
//! network, and it is the same trade the leader-driven consensus algorithms make.
//!
//! # What changed, and what it bought
//!
//! Algorithm 3.4 delivers when every process still *believed correct* has relayed a message. That
//! belief comes from a perfect failure detector, and
//! `uniform_agreement_breaks_when_the_timing_assumption_is_withdrawn` shows what one wrong belief
//! costs: a live process is dropped from `correct`, the condition is satisfied too early, and a
//! message is delivered by some processes and not others.
//!
//! Algorithm 3.5 asks a different question of the same record, and the book states the change
//! exactly:
//!
//! ```text
//! // Except for the function candeliver(·) below and for the absence of ⟨ Crash ⟩ events
//! // triggered by the perfect failure detector, it is the same as Algorithm 3.4.
//!
//! function candeliver(m) returns Boolean is
//!     return #(ack[m]) > N/2;
//! ```
//!
//! There is no set of believed-correct processes, so no process is ever excluded, so no wrong
//! judgement about who has crashed can be made. What is left is arithmetic over a record this
//! layer already kept. The rest of the algorithm — `pending`, the relay on first sight, the
//! identifier carrying the originator — is [`crate::uniform_reliable_broadcast`] unchanged:
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
//! upon exists (s, m) ∈ pending such that candeliver(m) ∧ m ∉ delivered do
//!     delivered := delivered ∪ {m};
//!     trigger ⟨ urb, Deliver | s, m ⟩;
//! ```
//!
//! # When the assumption fails, this layer blocks rather than diverges
//!
//! With `N ≤ 2f` — half or more of the processes crashed, or a partition leaving no majority
//! anywhere — no message reaches a majority and nothing further is delivered. That is a *worse
//! liveness* failure and no safety failure at all, which is the opposite of what happens to
//! Algorithm 3.4 when its detector is wrong. A blocked cluster can be repaired by restoring
//! processes; a split delivery cannot be repaired by anything.
//!
//! # Departures from the page
//!
//! - The predicate is written `2 · #(ack[m]) > N` rather than `#(ack[m]) > N/2`. The book means
//!   real division; integer division gives the same answer for every `N`, but only by an argument
//!   the reader has to reconstruct.
//! - `N` is the full membership including this process, and a process's own relay counts like any
//!   other, because best-effort broadcast sends to the sender too.
//! - `ack` and `delivered` are keyed by an identifier carrying the originator and a per-sender
//!   sequence number, not by message content — as in the all-ack version, so that identical
//!   content broadcast twice is delivered twice.
//! - This layer has one child, so it has no wire type of its own: the message *is* the broadcast
//!   child's. It is the first place in this stack where a wire type gets simpler going up, and
//!   removing an assumption is what did it.
//! - There is no `Init` event and no `Start` command. `new` establishes the state and there is
//!   nothing to start, failure detection having gone.
//! - Neither `ack` nor `pending` is garbage collected, as in the book. Long runs grow.
//!
//! # Over a link that reports scope boundaries
//!
//! `L` is a parameter, so this one module is also what
//! `session_majority_ack_uniform_reliable_broadcast` used to be. Algorithm 3.5 has no scope
//! events; the establishment clause is this layer's, and it is the same one the all-ack version
//! carries — resend everything pending, unconditionally, directed at the peer whose scope
//! returned.
//!
//! Unconditional matters here for an extra reason. `ack[m]` records who relayed `m` **to this
//! process**. It says nothing about whether *this* process's relay reached them, and that relay is
//! the token they are waiting for. Filtering the resend by `q ∉ ack[m]` deadlocks; the argument is
//! recorded at `resend_to`, where a test found it. The delivery predicate changing does not change
//! that argument, so the clause is carried over unchanged, including its cost: a re-establishment
//! sends every pending message to that peer.
//!
//! # When the assumption fails, this layer blocks rather than diverges
//!
//! A partition leaving one side with fewer than half the processes delivers nothing on that side,
//! rather than delivering something the majority will never deliver. When the sides rejoin, the
//! minority catches up through the same resend clause. Compare the all-ack version, where each
//! side accuses the other and both proceed — which is a split, and permanent.
//!
//! ```text
//! URB1 [always]       Validity — conditional on the reachability below
//! URB2 [incarnation]  No duplication — `delivered` is volatile, so a restart forgets it
//! URB3 [always]       No creation
//! URB4 [always]       Uniform agreement — conditional on the reachability below
//! ```
//!
//! `URB2` is `[incarnation]` by `docs/scope-annotated-modules.md` Corollary 7.2: the redundancy
//! that would have to survive is the `delivered` set, it is held in memory, and the boundary it
//! cannot cross is this process's own `⟨Init⟩`.
//!
//! `URB1` and `URB4` are `[always]` **only while a majority remains mutually reachable**, which is
//! an assumption and not a property of this code. A partition leaving no side with more than `N/2`
//! blocks both sides rather than splitting them, and both properties survive the block — nothing
//! is delivered that should not be. What does not survive a *permanent* split is liveness: the
//! layer waits for ever, which is the honest outcome and the one the all-ack version cannot offer,
//! because its detector's accusations let both sides proceed. See
//! `without_a_majority_the_layer_blocks_rather_than_diverges`.
//!
//! Unlike [`crate::uniform_reliable_broadcast`], no timing assumption is among them: removing the
//! detector removed the synchrony it needed, not just a dependency.

use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use std::collections::{BTreeMap, BTreeSet};

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use crate::link::VolatileLink;
use crate::perfect_link::PerfectLink;
use crate::uniform_reliable_broadcast::{BroadcastId, Data};

/// The message type: the broadcast child's, unwrapped.
///
/// With one child there is nothing to multiplex and no discriminant to add. Compare
/// [`crate::uniform_reliable_broadcast::Wire`], which needs an enum because a detector also sends.
pub type Msg<P, L = PerfectLink<Data<P>>> = <BestEffortBroadcast<Data<P>, L> as Protocol>::Msg;

/// What a link beneath this layer must carry.
///
/// A caller supplying their own link needs this and should not have to read the source to find it:
/// the payload is wrapped as the same `Data` as [`crate::uniform_reliable_broadcast`], which this layer shares, so the link carries `Carried<P>` rather than `P`.
pub type Carried<P> = Data<P>;

/// Requests from the layer above. Broadcasting is the only one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// `from` is the process that originated the message, never a relayer.
    Deliver { from: NodeId, msg: P },
    /// The scope with `peer` ended at `epoch`. Raised only over a link that reports boundaries.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`. The moment the resend becomes possible.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// Broadcast with uniform agreement, resting on a correct majority and on nothing else.
#[derive(Debug)]
pub struct MajorityAckUniformReliableBroadcast<
    P: Clone,
    L: VolatileLink<Data<P>> = PerfectLink<Data<P>>,
> {
    me: NodeId,
    seq: u64,
    /// How many processes there are. The denominator of the majority, and fixed.
    members: usize,
    /// Seen and not yet delivered, with the payload kept for delivery.
    pending: BTreeMap<BroadcastId, P>,
    /// Which processes have been seen to relay each message.
    ack: BTreeMap<BroadcastId, BTreeSet<NodeId>>,
    delivered: BTreeSet<BroadcastId>,
    beb: Child<BestEffortBroadcast<Data<P>, L>>,
}

impl<P: Clone> MajorityAckUniformReliableBroadcast<P, PerfectLink<Data<P>>> {
    /// Broadcast among `members`, which must include `me`.
    ///
    /// The guarantees hold while more than half of `members` are correct. There is no timing
    /// parameter, because there is no timeout: nothing here waits on a clock.
    pub fn new(
        me: NodeId,
        members: impl IntoIterator<Item = NodeId>,
        retransmit: core::time::Duration,
    ) -> Self {
        Self::with_link(me, members, PerfectLink::new(me, retransmit))
    }
}

impl<P: Clone, L: VolatileLink<Data<P>>> MajorityAckUniformReliableBroadcast<P, L> {
    /// Broadcast among `members`, over the link supplied.
    pub fn with_link(me: NodeId, members: impl IntoIterator<Item = NodeId>, link: L) -> Self {
        let mut members: BTreeSet<NodeId> = members.into_iter().collect();
        members.insert(me);
        let n = members.len();
        MajorityAckUniformReliableBroadcast {
            me,
            seq: 0,
            members: n,
            pending: BTreeMap::new(),
            ack: BTreeMap::new(),
            delivered: BTreeSet::new(),
            beb: Child::new(BestEffortBroadcast::with_link(me, members, link)),
        }
    }

    /// How many distinct messages have been delivered upward.
    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    /// Messages seen but not yet deliverable.
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

impl<P: Clone, L> MajorityAckUniformReliableBroadcast<P, L>
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
        let mut inds = self.beb.run(cx, core::convert::identity, f);
        for ind in inds.drain(..) {
            match ind {
                beb::Ind::Deliver { from, msg: Data { id, payload } } => {
                    self.on_beb_deliver(from, id, payload, cx)
                }
                beb::Ind::SessionEnded { peer, epoch } => {
                    // Informative only: the peer is unreachable, so nothing can be resent yet.
                    cx.indicate(Ind::SessionEnded { peer, epoch });
                }
                beb::Ind::SessionEstablished { peer, epoch } => {
                    cx.indicate(Ind::SessionEstablished { peer, epoch });
                    self.resend_to(peer, cx);
                }
            }
        }
        self.beb.reclaim(inds);
        self.check_deliverable(cx);
    }

    /// `upon event ⟨ beb, Deliver | p, [DATA, s, m] ⟩`.
    /// On a scope becoming available again, send that peer everything still pending.
    ///
    /// Unconditional, and directed at that peer alone. Unconditional matters: this layer has no
    /// failure detector, so it has no notion of a peer being *excluded*, and a resend filtered on
    /// some belief about who is still correct would deadlock the very case the majority rule
    /// exists to survive. A peer absent for longer than any timeout the all-ack version would have
    /// used is not a stranger when it returns, because nothing ever excluded it.
    fn resend_to(&mut self, peer: NodeId, cx: &mut ProtoCx<'_, Self>) {
        let outstanding: Vec<Data<P>> = self
            .pending
            .iter()
            .map(|(id, payload)| Data { id: *id, payload: payload.clone() })
            .collect();
        for data in outstanding {
            self.with_beb(cx, |beb, ccx| beb.on_cmd(beb::Cmd::SendTo { to: peer, msg: data }, ccx));
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
            self.relay(Data { id, payload }, cx);
        }
    }

    /// Re-broadcast, so the message survives its originator's crash.
    fn relay(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        // Re-enters the child while its inbox is out on loan, so `run` hands back a fresh one.
        let inds = self.beb.run(cx, core::convert::identity, |beb, ccx| {
            beb.on_cmd(beb::Cmd::Broadcast(data), ccx)
        });
        debug_assert!(
            inds.is_empty(),
            "relaying must not deliver synchronously; if it does, on_beb_deliver can recurse"
        );
        self.beb.reclaim(inds);
    }

    /// `upon exists (s, m) ∈ pending such that candeliver(m) ∧ m ∉ delivered`.
    ///
    /// A predicate over state rather than an event. Its only input is `ack`, which grows on a
    /// delivery from below — there is no second path, the detector having gone, so unlike the
    /// all-ack version this is called from one place.
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

    /// `#(ack[m]) > N/2` — more than half the processes have relayed it.
    fn can_deliver(&self, id: BroadcastId) -> bool {
        match self.ack.get(&id) {
            None => false,
            Some(acked) => 2 * acked.len() > self.members,
        }
    }
}

impl<P: Clone, L> Protocol for MajorityAckUniformReliableBroadcast<P, L>
where
    L: VolatileLink<Data<P>>,
{
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Msg<P, L>;
    /// No scope conditions: this protocol's guarantees do not lapse.
    /// Whatever the link's guarantees are conditional on. This layer bridges an ending by
    /// resending on the establishment that follows.
    type Scope = L::Scope;
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

    fn on_msg(&mut self, from: NodeId, msg: Msg<P, L>, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_timer(id, ccx));
    }

    /// Hand the scope ending down to the link. The trait's default would drop it.
    fn on_scope_event(&mut self, scope: L::Scope, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_scope_event(scope, ccx));
    }
}
