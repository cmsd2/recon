//! Lazy probabilistic broadcast — gossip, then pull back what it missed.
//!
//! **Status: implementation. Space: bounded by a retention window.**
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.7 and **Algorithms 3.10 and 3.11**. The book splits it
//! in two — a data half and a recovery half — and this module quotes both, verbatim from the source
//! rather than from any recollection of it.
//!
//! ```text
//! Algorithm 3.10: Lazy Probabilistic Broadcast (part 1, data dissemination)
//! Implements: ProbabilisticBroadcast, instance pb.
//! Uses:
//!     FairLossPointToPointLinks, instance fll;
//!     ProbabilisticBroadcast, instance upb.        // an unreliable implementation
//!
//! upon event ⟨ pb, Init ⟩ do
//!     next := [1]^N; lsn := 0; pending := ∅; stored := ∅;
//!
//! procedure gossip(msg) is
//!     forall t ∈ picktargets(k) do trigger ⟨ fll, Send | t, msg ⟩;
//!
//! upon event ⟨ pb, Broadcast | m ⟩ do
//!     lsn := lsn + 1;
//!     trigger ⟨ upb, Broadcast | [DATA, self, m, lsn] ⟩;
//!
//! upon event ⟨ upb, Deliver | p, [DATA, s, m, sn] ⟩ do
//!     if random([0, 1]) > α then
//!         stored := stored ∪ {[DATA, s, m, sn]};
//!     if sn = next[s] then
//!         next[s] := next[s] + 1;
//!         trigger ⟨ pb, Deliver | s, m ⟩;
//!     else if sn > next[s] then
//!         pending := pending ∪ {[DATA, s, m, sn]};
//!         forall missing ∈ [next[s], . . . , sn − 1] do
//!             if no m′ exists such that [DATA, s, m′, missing] ∈ pending then
//!                 gossip([REQUEST, self, s, missing, R − 1]);
//!         starttimer(Δ, s, sn);
//! ```
//!
//! ```text
//! Algorithm 3.11: Lazy Probabilistic Broadcast (part 2, recovery)
//!
//! upon event ⟨ fll, Deliver | p, [REQUEST, q, s, sn, r] ⟩ do
//!     if exists m such that [DATA, s, m, sn] ∈ stored then
//!         trigger ⟨ fll, Send | q, [DATA, s, m, sn] ⟩;
//!     else if r > 0 then
//!         gossip([REQUEST, q, s, sn, r − 1]);
//!
//! upon event ⟨ fll, Deliver | p, [DATA, s, m, sn] ⟩ do
//!     pending := pending ∪ {[DATA, s, m, sn]};
//!
//! upon exists [DATA, s, x, sn] ∈ pending such that sn = next[s] do
//!     next[s] := next[s] + 1;
//!     pending := pending \ {[DATA, s, x, sn]};
//!     trigger ⟨ pb, Deliver | s, x ⟩;
//!
//! upon event ⟨ Timeout | s, sn ⟩ do
//!     if sn > next[s] then
//!         next[s] := sn + 1;
//! ```
//!
//! # Two children, and why the second one matters
//!
//! `Uses:` names both `fll` and `upb`. Data is gossiped by the unreliable broadcast beneath;
//! **requests and their answers travel directly over the link**, bypassing the gossip. That is what
//! makes the second phase a *pull* and the algorithm lazy. Routing a request through `upb` would
//! flood the membership to repair one process's gap, which is exactly the cost this phase exists to
//! avoid. So this layer multiplexes two children onto one wire, as
//! [`crate::uniform_reliable_broadcast`] does for its broadcast and its detector.
//!
//! # Three readings the page settled, each of which was about to go the other way
//!
//! - **`next := [1]^N`.** Sequence numbers start at one. A zero-based `next` leaves every process
//!   waiting for a message no sender ever sends.
//! - **The timeout skips *past* the gap.** `if sn > next[s] then next[s] := sn + 1` abandons the
//!   message at `sn` too, not just those before it. Setting `next[s] := sn` would deliver a message
//!   the process has already given up on.
//! - **Draining `pending` is a standing condition.** `upon exists … such that sn = next[s]` is
//!   re-evaluated whenever `next` or `pending` changes, so closing one gap can release a long run at
//!   once. Written here as a loop after every mutation of either, which is the same thing.
//!
//! # The α, which the book states twice and inconsistently
//!
//! The pseudocode stores when `random([0,1]) > α`, so α is the probability of *not* storing. Page 99
//! says in prose that a process stores "with probability α", which is the opposite. Page 100 breaks
//! the tie: it describes setting `α = 0` as every process storing, which only holds under the
//! pseudocode's reading.
//!
//! `docs/postmortem.md` disagrees with itself on this too — its re-examination reaches the same
//! conclusion, and its own worked sketch writes `gen_bool(alpha)`. This module ends the question by
//! not using α at all: [`Config::store_probability`] is the probability of storing, named for what
//! it does, and the book's α is one minus it.
//!
//! # What this buys, and what it costs
//!
//! ```text
//! PB1 [probabilistic]  Probabilistic validity — strictly better than the eager algorithm's under
//!                      loss, because a gap is repaired rather than lost
//! PB2 [window]         No duplication — within the retention window
//! PB3 [always]         No creation
//! ```
//!
//! Recovery depends on some reachable process having stored the message, so `PB1` here is
//! conditional on `store_probability` and on that process being reachable — not absolute. A gap
//! nobody stored is skipped by the timeout, which converts a permanent stall into a lost message,
//! and a stall would be the worse outcome.
//!
//! # The retention window, which is this project's and not the book's
//!
//! Page 100: "garbage collection of the stored message copies is omitted in the pseudo code for
//! simplicity." Both `stored` and `pending` are bounded here by a per-sender window and evicted on
//! insert, for the reasons [`crate::probabilistic_broadcast`] gives at length. A request for
//! something evicted is answered as unavailable, and the requester's timeout moves it past the gap.
//!
//! # Identity is scoped to the originator's incarnation — departure
//!
//! The book's `s` is a process, and `next[s]`, `pending` and `stored` are keyed by it. `lsn` is
//! volatile, so a process that crashes and comes back numbers its messages from one again — and
//! every receiver, holding `next[s] = 4`, would drop its first three as already delivered, silently,
//! through the `sn < next[s]` case the pseudocode does not even write. Under the book's crash-stop
//! model that case never arises; in the real-world set it is the first thing a restart does.
//!
//! So the sender of a [`Data`] is a [`Sender`] — the originator **and its incarnation**, a value
//! drawn from the seeded generator at `Init` exactly as [`crate::probabilistic_broadcast`] draws
//! its own — and every per-sender structure is keyed by that. A restarted originator is a new
//! sender with `next = 1`, and its messages are delivered.
//!
//! What bounds it: a receiver remembers the **two most recent incarnations** of each originator, and
//! admitting a third retires the oldest — its `next`, its pending and stored messages, its timers.
//! Two rather than one because relayed copies from the incarnation just retired can still be
//! arriving while the new one's begin, and a one-deep memory would flip between them, losing both.
//! Two rather than more because a process has one live incarnation and at most one being retired;
//! a message from an incarnation older than that is a straggler this abstraction may lose. State is
//! therefore bounded by `2 × membership × window`, and a restart costs one purge, not a leak.

use core::time::Duration;
use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::fair_loss_link::FairLossLink;
use crate::link::{Boundary, LinkInd, VolatileLink};
use crate::probabilistic_broadcast::{self as pb, ProbabilisticBroadcast};

/// `[DATA, s, m, sn]` — this layer's header, carried as the gossip's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data<P> {
    /// `s` — who originated it.
    pub origin: NodeId,
    /// Which incarnation of `origin`. See the module note on identity.
    pub incarnation: u64,
    /// `sn` — that sender's sequence number for it.
    pub seq: u64,
    pub payload: P,
}

impl<P> Data<P> {
    /// The book's `s`, as this module keys on it: the originator in a particular incarnation.
    pub fn sender(&self) -> Sender {
        Sender { origin: self.origin, incarnation: self.incarnation }
    }
}

/// An originator in one incarnation — what `next`, `pending` and `stored` are keyed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Sender {
    pub origin: NodeId,
    pub incarnation: u64,
}

/// How many incarnations of one originator a receiver keeps state for. See the module note.
const INCARNATIONS_REMEMBERED: usize = 2;

/// What travels over the link directly, outside the gossip: `[REQUEST, …]` and its answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recovery<P> {
    /// `[REQUEST, q, s, sn, r]` — `q` is the process that wants it, carried so that whoever holds
    /// it answers the requester rather than the relayer.
    Request { requester: NodeId, origin: NodeId, incarnation: u64, seq: u64, ttl: u32 },
    /// `[DATA, s, m, sn]` sent back to a requester.
    Data(Data<P>),
}

/// The wire, multiplexing the two children Algorithm 3.10 names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<G, R> {
    /// The gossip child's traffic.
    Gossip(G),
    /// Recovery traffic, which deliberately does not go through the gossip.
    Recovery(R),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// `from` is the originator, never a relayer, and deliveries from one sender are in sequence.
    Deliver { from: NodeId, msg: P },
    /// The scope with `peer` ended at `epoch`. Raised only over a link that reports boundaries.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// How this instance recovers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// The gossip beneath: its fanout, rounds and window.
    pub gossip: pb::Config,
    /// How likely a process is to keep a copy for answering requests.
    ///
    /// **The book's α is one minus this.** Named for what it does; see the module note. At `1.0`
    /// every process stores everything, which is the certain and expensive case the book describes
    /// as `α = 0`.
    pub store_probability: f64,
    /// `R` for a request — how far a request is relayed before it is abandoned.
    pub request_rounds: u32,
    /// `Δ` — how long to wait for a gap before giving up on it.
    pub gap_timeout: Duration,
    /// How many messages to keep in `stored` and `pending`, per sender.
    pub window: usize,
}

/// The gossip this layer rides on: Algorithm 3.9 carrying [`Data`], over `G` — the fair-loss link
/// the book names unless the caller says otherwise.
pub type Gossiper<P, G = FairLossLink<pb::Carried<Data<P>>>> = ProbabilisticBroadcast<Data<P>, G>;

/// Gossip with recovery.
///
/// Two links, both parameters: `L` carries the recovery traffic and `G` carries the gossip. Over
/// sessions both are session links — two instances each holding one epoch per peer, both handed
/// every scope event, on one wire — which is how [`crate::uniform_reliable_broadcast`] already puts
/// a broadcast and a detector together. Their scopes must agree (`G::Scope = L::Scope`), because a
/// scope event reaching this layer is one event about one session and goes to both.
pub struct LazyProbabilisticBroadcast<
    P,
    L = FairLossLink<Recovery<P>>,
    G = FairLossLink<pb::Carried<Data<P>>>,
> where
    P: Clone + serde::Serialize + serde::de::DeserializeOwned,
    L: VolatileLink<Recovery<P>>,
    L::Scope: Clone,
    G: VolatileLink<pb::Carried<Data<P>>, Scope = L::Scope>,
{
    me: NodeId,
    peers: BTreeSet<NodeId>,
    config: Config,
    /// This incarnation's name, drawn at `Init`. Zero until then.
    incarnation: u64,
    /// `lsn` — this process's own sequence counter.
    lsn: u64,
    /// The incarnations of each originator this process keeps state for, oldest first. At most
    /// [`INCARNATIONS_REMEMBERED`]; admitting another retires the oldest.
    incarnations: BTreeMap<NodeId, VecDeque<u64>>,
    /// `next[s]` — the next sequence number expected from `s`. Absent means one, per `[1]^N`.
    next: BTreeMap<Sender, u64>,
    /// `pending` — received, but ahead of a gap.
    pending: BTreeMap<(Sender, u64), P>,
    /// `stored` — kept so this process can answer a request.
    stored: BTreeMap<(Sender, u64), P>,
    /// Insertion order per sender, so both collections evict in constant time.
    pending_order: BTreeMap<Sender, VecDeque<u64>>,
    stored_order: BTreeMap<Sender, VecDeque<u64>>,
    /// Which gap each outstanding timer is waiting on.
    timers: BTreeMap<TimerId, (Sender, u64)>,
    upb: Child<Gossiper<P, G>>,
    link: Child<L>,
}

impl<P> LazyProbabilisticBroadcast<P, FairLossLink<Recovery<P>>>
where
    P: Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    /// Lazy probabilistic broadcast among `peers`, over the fair-loss links the book names.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, config: Config) -> Self {
        Self::with_link(me, peers, FairLossLink::new(), config)
    }
}

impl<P, L> LazyProbabilisticBroadcast<P, L>
where
    P: Clone + serde::Serialize + serde::de::DeserializeOwned,
    L: VolatileLink<Recovery<P>, Scope = core::convert::Infallible>,
{
    /// Lazy probabilistic broadcast, over the link supplied for its recovery traffic and the
    /// book's fair-loss link for the gossip.
    ///
    /// Only for a recovery link that reports no boundary: the gossip beneath runs over a fair-loss
    /// link here, and the two links' scopes have to agree. Over sessions use
    /// [`LazyProbabilisticBroadcast::with_links`] with a session link for both.
    pub fn with_link(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        link: L,
        config: Config,
    ) -> Self {
        Self::with_links(me, peers, FairLossLink::new(), link, config)
    }
}

impl<P, L, G> LazyProbabilisticBroadcast<P, L, G>
where
    P: Clone + serde::Serialize + serde::de::DeserializeOwned,
    L: VolatileLink<Recovery<P>>,
    L::Scope: Clone,
    G: VolatileLink<pb::Carried<Data<P>>, Scope = L::Scope>,
{
    /// Lazy probabilistic broadcast over two links: `gossip` beneath the eager broadcast that
    /// disseminates data, `recovery` for the requests and answers that repair gaps.
    pub fn with_links(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        gossip: G,
        recovery: L,
        config: Config,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        LazyProbabilisticBroadcast {
            me,
            peers: peers.clone(),
            config,
            incarnation: 0,
            lsn: 0,
            incarnations: BTreeMap::new(),
            next: BTreeMap::new(),
            pending: BTreeMap::new(),
            stored: BTreeMap::new(),
            pending_order: BTreeMap::new(),
            stored_order: BTreeMap::new(),
            timers: BTreeMap::new(),
            upb: Child::new(ProbabilisticBroadcast::with_link(me, peers, gossip, config.gossip)),
            link: Child::new(recovery),
        }
    }

    /// The link the gossip travels over.
    pub fn gossip_link(&self) -> &G {
        self.upb.link()
    }

    /// The link the recovery traffic travels over.
    pub fn recovery_link(&self) -> &L {
        &self.link
    }

    /// `next[s]`, which is one until this process has delivered anything from `s`.
    pub fn next_expected_of(&self, sender: Sender) -> u64 {
        self.next.get(&sender).copied().unwrap_or(1)
    }

    /// `next[s]` for the incarnation of `from` most recently heard from, or one if none has been.
    pub fn next_expected(&self, from: NodeId) -> u64 {
        self.latest(from).map(|s| self.next_expected_of(s)).unwrap_or(1)
    }

    /// The incarnation of `from` most recently admitted, if any.
    pub fn latest(&self, from: NodeId) -> Option<Sender> {
        self.incarnations
            .get(&from)
            .and_then(|v| v.back())
            .map(|incarnation| Sender { origin: from, incarnation: *incarnation })
    }

    /// How many incarnations of `from` this process keeps state for.
    pub fn incarnations_of(&self, from: NodeId) -> usize {
        self.incarnations.get(&from).map(|v| v.len()).unwrap_or(0)
    }

    /// How many messages this process is holding ahead of a gap.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// How many copies this process is holding for answering requests.
    pub fn stored_count(&self) -> usize {
        self.stored.len()
    }

    /// Whether this process could answer a request for `seq` from the incarnation of `origin`
    /// most recently heard from.
    pub fn has_stored(&self, origin: NodeId, seq: u64) -> bool {
        self.latest(origin).is_some_and(|s| self.stored.contains_key(&(s, seq)))
    }
}

impl<P: Clone, L, G> LazyProbabilisticBroadcast<P, L, G>
where
    L: VolatileLink<Recovery<P>>,
    L::Scope: Clone,
    G: VolatileLink<pb::Carried<Data<P>>, Scope = L::Scope>,
    P: serde::Serialize + serde::de::DeserializeOwned,
{
    /// Run the gossip child, then act on what it reported.
    fn through_upb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut Gossiper<P, G>, &mut ProtoCx<'_, Gossiper<P, G>>),
    ) {
        let mut inds = self.upb.run(cx, Wire::Gossip, f);
        for ind in inds.drain(..) {
            match ind {
                pb::Ind::Deliver { msg, .. } => self.on_upb_deliver(msg, cx),
                // A boundary the gossip's link observed. The recovery link observed the same one —
                // both links are handed every scope event, and their scopes are one type by the
                // bound on `G` — so it is reported upward from `through_link` and once. Reporting
                // it here too would tell the layer above one session ended twice.
                pb::Ind::SessionEnded { .. } | pb::Ind::SessionEstablished { .. } => {}
            }
        }
        self.upb.reclaim(inds);
    }

    /// Run the link, then act on the recovery traffic it reported.
    fn through_link(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut L, &mut ProtoCx<'_, L>),
    ) {
        let mut inds = self.link.run(cx, Wire::Recovery, f);
        for ind in inds.drain(..) {
            match L::classify(ind) {
                LinkInd::Deliver { msg, .. } => self.on_recovery(msg, cx),
                LinkInd::Boundary(Boundary::Ended { peer, epoch }) => {
                    cx.indicate(Ind::SessionEnded { peer, epoch })
                }
                LinkInd::Boundary(Boundary::Established { peer, epoch }) => {
                    cx.indicate(Ind::SessionEstablished { peer, epoch })
                }
            }
        }
        self.link.reclaim(inds);
    }

    /// `upon event ⟨ upb, Deliver | p, [DATA, s, m, sn] ⟩` — Algorithm 3.10.
    fn on_upb_deliver(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        use rand::Rng;

        let sender = data.sender();
        self.admit(sender);

        // `if random([0, 1]) > α then stored := stored ∪ {…}`, with α restated as its complement.
        // Before the sequence checks, so a message this process cannot deliver yet is still one it
        // can answer a request for.
        if cx.rng().random_bool(self.config.store_probability) {
            self.store(data.clone());
        }

        let next = self.next_expected_of(sender);
        if data.seq == next {
            self.next.insert(sender, next + 1);
            cx.indicate(Ind::Deliver { from: data.origin, msg: data.payload });
            self.drain_pending(sender, cx);
        } else if data.seq > next {
            // `forall missing ∈ [next[s], …, sn − 1] do if no m′ … ∈ pending then gossip(REQUEST)`.
            // The guard is `pending`, and nothing else: the book re-requests a gap on every
            // out-of-order arrival that does not already have it pending. That looks like a defect
            // and was once reported as one; it is the page.
            for missing in next..data.seq {
                if !self.pending.contains_key(&(sender, missing)) {
                    self.request(sender, missing, cx);
                }
            }
            let seq = data.seq;
            self.hold(data);
            // `starttimer(Δ, s, sn)` — one timer per gap, remembered by its handle so the expiry
            // can be matched back to the gap it was waiting on.
            let id = cx.set_timer(self.config.gap_timeout);
            self.timers.insert(id, (sender, seq));
        }
    }

    /// Note an incarnation of an originator, retiring the oldest if this makes one too many. See
    /// the module note on identity for why two, and what retiring costs.
    fn admit(&mut self, sender: Sender) {
        let known = self.incarnations.entry(sender.origin).or_default();
        if known.contains(&sender.incarnation) {
            return;
        }
        known.push_back(sender.incarnation);
        if known.len() > INCARNATIONS_REMEMBERED
            && let Some(retired) = known.pop_front()
        {
            let retired = Sender { origin: sender.origin, incarnation: retired };
            self.next.remove(&retired);
            self.pending.retain(|(s, _), _| *s != retired);
            self.stored.retain(|(s, _), _| *s != retired);
            self.pending_order.remove(&retired);
            self.stored_order.remove(&retired);
            self.timers.retain(|_, (s, _)| *s != retired);
        }
    }

    /// `upon event ⟨ fll, Deliver | p, [REQUEST | DATA, …] ⟩` — Algorithm 3.11.
    fn on_recovery(&mut self, r: Recovery<P>, cx: &mut ProtoCx<'_, Self>) {
        match r {
            Recovery::Request { requester, origin, incarnation, seq, ttl } => {
                let sender = Sender { origin, incarnation };
                if let Some(payload) = self.stored.get(&(sender, seq)).cloned() {
                    // `trigger ⟨ fll, Send | q, [DATA, s, m, sn] ⟩` — to the requester, not the
                    // relayer, which is why `q` travels in the request.
                    let answer = Recovery::Data(Data { origin, incarnation, seq, payload });
                    self.send_to(requester, answer, cx);
                } else if ttl > 0 {
                    // `else if r > 0 then gossip([REQUEST, q, s, sn, r − 1])` — `q` is preserved.
                    self.gossip_request(
                        Recovery::Request { requester, origin, incarnation, seq, ttl: ttl - 1 },
                        cx,
                    );
                }
            }
            // `upon event ⟨ fll, Deliver | p, [DATA, s, m, sn] ⟩ do pending := pending ∪ {…}`.
            // A recovered message joins `pending` and is released by the standing condition, which
            // is what lets it close a gap without a second code path for delivery.
            Recovery::Data(data) => {
                let sender = data.sender();
                self.admit(sender);
                self.hold(data);
                self.drain_pending(sender, cx);
            }
        }
    }

    /// `upon exists [DATA, s, x, sn] ∈ pending such that sn = next[s]` — the standing condition.
    ///
    /// A loop rather than a single step: closing one gap can release an arbitrarily long run, and
    /// the book's `upon` is re-evaluated after every change.
    fn drain_pending(&mut self, from: Sender, cx: &mut ProtoCx<'_, Self>) {
        while let Some(payload) = self.pending.remove(&(from, self.next_expected_of(from))) {
            let seq = self.next_expected_of(from);
            self.next.insert(from, seq + 1);
            if let Some(order) = self.pending_order.get_mut(&from) {
                order.retain(|s| *s != seq);
            }
            cx.indicate(Ind::Deliver { from: from.origin, msg: payload });
        }
    }

    /// `gossip([REQUEST, self, s, missing, R − 1])` — over the link, not through the gossip child.
    fn request(&mut self, from: Sender, seq: u64, cx: &mut ProtoCx<'_, Self>) {
        let r = Recovery::Request {
            requester: self.me,
            origin: from.origin,
            incarnation: from.incarnation,
            seq,
            ttl: self.config.request_rounds.saturating_sub(1),
        };
        self.gossip_request(r, cx);
    }

    /// `procedure gossip(msg)` for recovery traffic: `picktargets(k)` over the link.
    fn gossip_request(&mut self, r: Recovery<P>, cx: &mut ProtoCx<'_, Self>) {
        for target in self.picktargets(cx) {
            self.send_to(target, r.clone(), cx);
        }
    }

    /// `trigger ⟨ fll, Send | t, msg ⟩`.
    fn send_to(&mut self, to: NodeId, r: Recovery<P>, cx: &mut ProtoCx<'_, Self>) {
        let inds = self.link.run(cx, Wire::Recovery, |link, ccx| link.on_cmd(L::send(to, r), ccx));
        debug_assert!(inds.is_empty(), "a send must not deliver synchronously");
        self.link.reclaim(inds);
    }

    /// `picktargets(k)` — the same uniform draw without replacement the gossip uses.
    fn picktargets(&self, cx: &mut ProtoCx<'_, Self>) -> Vec<NodeId> {
        use rand::Rng;
        let mut candidates: Vec<NodeId> =
            self.peers.iter().copied().filter(|p| *p != self.me).collect();
        let take = self.config.gossip.fanout.min(candidates.len());
        for i in 0..take {
            let j = i + cx.rng().random_range(0..candidates.len() - i);
            candidates.swap(i, j);
        }
        candidates.truncate(take);
        candidates
    }

    /// `pending := pending ∪ {[DATA, s, m, sn]}`, bounded by the window.
    fn hold(&mut self, data: Data<P>) {
        let sender = data.sender();
        if self.pending.insert((sender, data.seq), data.payload).is_none() {
            let order = self.pending_order.entry(sender).or_default();
            order.push_back(data.seq);
            if order.len() > self.config.window
                && let Some(evicted) = order.pop_front()
            {
                self.pending.remove(&(sender, evicted));
            }
        }
    }

    /// `stored := stored ∪ {[DATA, s, m, sn]}`, bounded by the window.
    fn store(&mut self, data: Data<P>) {
        let sender = data.sender();
        if self.stored.insert((sender, data.seq), data.payload).is_none() {
            let order = self.stored_order.entry(sender).or_default();
            order.push_back(data.seq);
            if order.len() > self.config.window
                && let Some(evicted) = order.pop_front()
            {
                self.stored.remove(&(sender, evicted));
            }
        }
    }
}

impl<P, L, G> Protocol for LazyProbabilisticBroadcast<P, L, G>
where
    P: Clone + serde::Serialize + serde::de::DeserializeOwned,
    L: VolatileLink<Recovery<P>>,
    L::Scope: Clone,
    G: VolatileLink<pb::Carried<Data<P>>, Scope = L::Scope>,
{
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<G::Msg, L::Msg>;
    type Scope = L::Scope;
    type Note = crate::Note;
    /// Keeps nothing durably.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    /// `upon event ⟨ pb, Broadcast | m ⟩ do lsn := lsn + 1; trigger ⟨ upb, Broadcast | [DATA, …] ⟩`.
    ///
    /// Note what is *not* here: no delivery to self. The eager child beneath delivers a broadcast
    /// to its own process, and that arrives back through `on_upb_deliver` like any other, which is
    /// what puts this process's own messages through the same sequence check as everyone else's.
    fn on_cmd(&mut self, Cmd::Broadcast(payload): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.lsn += 1;
        let data = Data { origin: self.me, incarnation: self.incarnation, seq: self.lsn, payload };
        self.through_upb(cx, |upb, ccx| upb.on_cmd(pb::Cmd::Broadcast(data), ccx));
    }

    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Gossip(m) => self.through_upb(cx, |upb, ccx| upb.on_msg(from, m, ccx)),
            Wire::Recovery(m) => self.through_link(cx, |link, ccx| link.on_msg(from, m, ccx)),
        }
    }

    /// `upon event ⟨ Timeout | s, sn ⟩ do if sn > next[s] then next[s] := sn + 1`.
    ///
    /// The gap is abandoned, and `sn` with it — the book skips *past* the message the timer was
    /// waiting on, not to it. Whatever is now deliverable is released by the standing condition,
    /// which is why the drain follows.
    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        if let Some((sender, seq)) = self.timers.remove(&id) {
            if seq > self.next_expected_of(sender) {
                self.next.insert(sender, seq + 1);
                self.drain_pending(sender, cx);
            }
            return;
        }
        // Not this layer's. Hand it to both children, since neither's expiry is distinguishable
        // from the other's by its handle alone.
        self.through_upb(cx, |upb, ccx| upb.on_timer(id, ccx));
        self.through_link(cx, |link, ccx| link.on_timer(id, ccx));
    }

    /// Both children run over the same session, so both are told when it ends or begins.
    fn on_scope_event(&mut self, scope: L::Scope, cx: &mut ProtoCx<'_, Self>) {
        let for_gossip = scope.clone();
        self.through_upb(cx, |upb, ccx| upb.on_scope_event(for_gossip, ccx));
        self.through_link(cx, |link, ccx| link.on_scope_event(scope, ccx));
    }

    /// Name this incarnation, then start the children. Runs on every restart, which is the point.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.incarnation = cx.rng().next_u64();
        self.through_upb(cx, |upb, ccx| upb.on_init(ccx));
        self.through_link(cx, |link, ccx| link.on_init(ccx));
    }
}
