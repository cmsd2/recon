//! Eager probabilistic broadcast — gossip.
//!
//! **Status: implementation. Space: bounded by a retention window.** The first module above the
//! failure detector that is not a transcription; see the guarantee table below for what the window
//! costs.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.7 and Algorithm 3.9 ("Eager Probabilistic Broadcast").
//! Quoted from the book rather than from memory, which matters here more than anywhere else in this
//! repository: `docs/postmortem.md` records four bugs once reported in the previous implementation
//! of this algorithm, of which **three were false positives** produced by reading code against
//! remembered pseudocode.
//!
//! ```text
//! upon event ⟨ pb, Init ⟩ do
//!     delivered := ∅;
//!
//! procedure gossip(msg) is
//!     forall t ∈ picktargets(k) do
//!         trigger ⟨ fll, Send | t, msg ⟩;
//!
//! upon event ⟨ pb, Broadcast | m ⟩ do
//!     delivered := delivered ∪ {m};
//!     trigger ⟨ pb, Deliver | self, m ⟩;
//!     gossip([GOSSIP, self, m, R]);
//!
//! upon event ⟨ fll, Deliver | p, [GOSSIP, s, m, r] ⟩ do
//!     if m ∉ delivered then
//!         delivered := delivered ∪ {m};
//!         trigger ⟨ pb, Deliver | s, m ⟩;
//!     if r > 1 then gossip([GOSSIP, s, m, r − 1]);
//! ```
//!
//! # The relay is outside the delivery guard, and that is the book's
//!
//! Read the indentation. `if r > 1 then gossip(...)` sits at the same level as `if m ∉ delivered`,
//! not inside it, so a process relays a message it has **already delivered**. This looks like a
//! defect. It has been reported as one. It is not: the book names the consequence on the same page
//! — "the algorithm induces a significant amount of redundancy in the message exchanges: any given
//! process may receive the same message many times" — and the redundancy is what makes the
//! probability work out. Relaying only on first receipt would cut the fan-out of every message that
//! reaches a process twice, which is most of them.
//!
//! # What is probabilistic, and what is not
//!
//! ```text
//! PB1 [probabilistic]  Probabilistic validity — a correct sender's message reaches every correct
//!                      process with high probability, and on some runs it does not
//! PB2 [window]         No duplication — within the retention window; see below
//! PB3 [always]         No creation
//! ```
//!
//! `PB1` is the whole point and the whole cost. Best-effort broadcast reaches everyone whenever the
//! sender is correct; this does not, and buys in exchange that no process ever sends to all of `Π`.
//! A run in which some correct process never delivers is **not a violation** — the suite counts
//! such runs rather than failing on them, and asserts against a stated threshold.
//!
//! # Identity is an identifier, not the message
//!
//! **Departure.** The book deduplicates on the message itself — `m ∉ delivered` — which assumes
//! messages are unique across senders. Every broadcast here instead carries a [`BroadcastId`]: its
//! originator and a per-sender sequence number, exactly as `reliable_broadcast` does, so identical
//! content broadcast twice is delivered twice. The consequence for this module is that `delivered`
//! holds identifiers rather than payloads, which is also what makes the window below affordable.
//!
//! The sequence counter is volatile and so is the set it keys, which is the pairing
//! `CLAUDE.md` requires: a durable set keyed by a volatile counter is the bug, and neither half is
//! durable here.
//!
//! # The retention window, which is this project's and not the book's
//!
//! Page 100: "garbage collection of the stored message copies is omitted in the pseudo code for
//! simplicity." So there is no page to follow, and the mechanism is a design decision with its own
//! cost. `delivered` keeps the most recent `window` identifiers **per sender** and evicts the
//! oldest when that is exceeded, on insert.
//!
//! Two things follow, and both are deliberate:
//!
//! - **Reclaiming is constant work.** One eviction per insert, never a pass over the set. The
//!   previous implementation expired by wall-clock age and rebuilt the whole set on every event, so
//!   receiving one message cost time linear in everything ever received. That is the one defect in
//!   that code which survived scrutiny, and this is the shape that avoids it rather than a smaller
//!   version of it.
//! - **`PB2` is scoped to the window.** A message re-arriving after its identifier has been evicted
//!   is delivered again. That is the stated guarantee, not a violation of it, and it is why the
//!   table above says `[window]` where the book says nothing.

use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::fair_loss_link::FairLossLink;
use crate::link::{Boundary, LinkInd, VolatileLink};

/// Which broadcast this is: who originated it, and their sequence number for it.
///
/// See the departure note above — the book keys on the message, this keys on an identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BroadcastId {
    pub origin: NodeId,
    pub seq: u64,
}

/// What this layer puts on the wire: the identifier, the rounds still to live, and the payload.
///
/// `ttl` is the book's `r`. It is decremented at each hop and a message is not relayed once it
/// reaches one, which is what makes a broadcast generate finitely many transmissions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gossip<P> {
    pub id: BroadcastId,
    pub ttl: u32,
    pub payload: P,
}

/// What a link beneath this layer must carry.
pub type Carried<P> = Gossip<P>;

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// `from` is the originator, never a relayer.
    Deliver { from: NodeId, msg: P },
    /// The scope with `peer` ended at `epoch`. Raised only over a link that reports boundaries.
    ///
    /// This layer cannot bridge one: it keeps identifiers rather than payloads, so it has nothing
    /// to resend. It propagates, as `docs/conditional-guarantees.md` requires of a layer in that
    /// position. A scope ending is in any case only one more way for a gossip to be lost, which is
    /// a case this abstraction already tolerates by construction.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// How this instance gossips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// The book's `k` — how many peers a relay addresses. Must be smaller than the membership for
    /// the abstraction to be doing anything; see [`Config::fanout`].
    pub fanout: usize,
    /// The book's `R` — how many hops a message travels before it stops being relayed.
    pub rounds: u32,
    /// How many identifiers to remember per sender. See the module note on the retention window.
    pub window: usize,
}

impl Config {
    /// A configuration, with the fanout and rounds the caller wants and a window large enough that
    /// deduplication is not the thing under test.
    pub fn new(fanout: usize, rounds: u32, window: usize) -> Self {
        Config { fanout, rounds, window }
    }
}

/// Gossip: relay to a random few, for a bounded number of rounds.
///
/// `L` is the link beneath and it is a parameter, so this composes over a perfect link, a session
/// link, or an application's own. It bounds on [`Link`] rather than anything narrower because
/// gossip needs nothing of a scope boundary beyond passing it upward.
#[derive(Debug)]
pub struct ProbabilisticBroadcast<P: Clone, L: VolatileLink<Carried<P>> = FairLossLink<Gossip<P>>> {
    me: NodeId,
    /// Π — every process, including this one. The sender delivers to itself directly, as Algorithm
    /// 3.9 has it, so `picktargets` draws from everyone else.
    peers: BTreeSet<NodeId>,
    seq: u64,
    config: Config,
    /// Identifiers already delivered, and the order they arrived in per sender, so the oldest can
    /// be evicted in constant time. Bounded by `config.window` per sender.
    delivered: BTreeSet<BroadcastId>,
    order: BTreeMap<NodeId, VecDeque<u64>>,
    link: Child<L>,
    _payload: core::marker::PhantomData<fn() -> P>,
}

impl<P: Clone> ProbabilisticBroadcast<P, FairLossLink<Gossip<P>>> {
    /// Gossip among `peers`, over the fair-loss link Algorithm 3.9 names.
    ///
    /// The book says `Uses: FairLossPointToPointLinks` and this default honours it. A perfect link
    /// would retransmit until delivery, which masks the probabilistic guarantee this abstraction
    /// exists to provide — and would never fall silent, because the stubborn link beneath it
    /// re-sends everything it has ever sent. Gossip over a link that does not lose is gossip with
    /// nothing to do.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, config: Config) -> Self {
        Self::with_link(me, peers, FairLossLink::new(), config)
    }
}

impl<P: Clone, L: VolatileLink<Carried<P>>> ProbabilisticBroadcast<P, L> {
    /// Gossip among `peers`, over the link supplied.
    pub fn with_link(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        link: L,
        config: Config,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        ProbabilisticBroadcast {
            me,
            peers,
            seq: 0,
            config,
            delivered: BTreeSet::new(),
            order: BTreeMap::new(),
            link: Child::new(link),
            _payload: core::marker::PhantomData,
        }
    }

    /// How many identifiers this process is currently remembering. For the bound test.
    pub fn remembered(&self) -> usize {
        self.delivered.len()
    }

    /// Whether this process has delivered `id` and still remembers doing so.
    pub fn has_delivered(&self, id: BroadcastId) -> bool {
        self.delivered.contains(&id)
    }

    /// The processes this instance gossips among, in a stable order.
    pub fn peers(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.peers.iter().copied()
    }
}

impl<P: Clone, L> ProbabilisticBroadcast<P, L>
where
    L: VolatileLink<Carried<P>>,
{
    /// Run the link, then act on whatever it reported.
    fn through_link(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut L, &mut ProtoCx<'_, L>),
    ) {
        let mut inds = self.link.run(cx, core::convert::identity, f);
        for ind in inds.drain(..) {
            match L::classify(ind) {
                LinkInd::Deliver { msg, .. } => self.on_arrival(msg, cx),
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

    /// `upon event ⟨ fll, Deliver | p, [GOSSIP, s, m, r] ⟩`.
    ///
    /// The two clauses are siblings, not nested. See the module note: relaying a message already
    /// delivered is the book's, and removing the redundancy would remove the guarantee with it.
    fn on_arrival(&mut self, msg: Gossip<P>, cx: &mut ProtoCx<'_, Self>) {
        let Gossip { id, ttl, payload } = msg;

        if !self.delivered.contains(&id) {
            self.record(id);
            cx.indicate(Ind::Deliver { from: id.origin, msg: payload.clone() });
        }

        if ttl > 1 {
            self.gossip(Gossip { id, ttl: ttl - 1, payload }, cx);
        }
    }

    /// `procedure gossip(msg) is forall t ∈ picktargets(k) do trigger ⟨ fll, Send | t, msg ⟩`.
    fn gossip(&mut self, msg: Gossip<P>, cx: &mut ProtoCx<'_, Self>) {
        for target in self.picktargets(cx) {
            let out = msg.clone();
            let inds = self.link.run(cx, core::convert::identity, |link, ccx| {
                link.on_cmd(L::send(target, out), ccx)
            });
            debug_assert!(inds.is_empty(), "a send must not deliver synchronously");
            self.link.reclaim(inds);
        }
    }

    /// `picktargets(k)` — `k` peers other than this one, drawn without replacement.
    ///
    /// Never the whole membership: fanning out to all of `Π` is best-effort broadcast, which this
    /// repository already has, and a probabilistic broadcast that does it has paid for uncertainty
    /// and bought nothing. When the fanout exceeds the number of peers the draw returns them all,
    /// which is a configuration mistake rather than a special case worth encoding.
    fn picktargets(&self, cx: &mut ProtoCx<'_, Self>) -> Vec<NodeId> {
        use rand::Rng;
        let mut candidates: Vec<NodeId> =
            self.peers.iter().copied().filter(|p| *p != self.me).collect();
        let take = self.config.fanout.min(candidates.len());
        // Partial Fisher-Yates: `take` draws, each from what is left, so no peer is chosen twice
        // and the cost is the fanout rather than the membership.
        for i in 0..take {
            let j = i + cx.rng().random_range(0..candidates.len() - i);
            candidates.swap(i, j);
        }
        candidates.truncate(take);
        candidates
    }

    /// Remember `id`, evicting this sender's oldest if the window is full.
    ///
    /// Constant work per insert. The alternative — expiring by age with a pass over the set — is
    /// the previous implementation's one surviving defect, and is what this shape exists to avoid.
    fn record(&mut self, id: BroadcastId) {
        self.delivered.insert(id);
        let seen = self.order.entry(id.origin).or_default();
        seen.push_back(id.seq);
        if seen.len() > self.config.window
            && let Some(evicted) = seen.pop_front()
        {
            self.delivered.remove(&BroadcastId { origin: id.origin, seq: evicted });
        }
    }
}

impl<P: Clone, L> Protocol for ProbabilisticBroadcast<P, L>
where
    L: VolatileLink<Carried<P>>,
{
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = L::Msg;
    /// Whatever the link's guarantees are conditional on. This layer adds no condition of its own
    /// and cannot bridge the link's.
    type Scope = L::Scope;
    /// Keeps nothing durably: a crash loses everything this protocol knows, which is why `PB2` is
    /// scoped to the window *within* an incarnation and says nothing across one.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    /// `upon event ⟨ pb, Broadcast | m ⟩` — deliver to self, then gossip.
    fn on_cmd(&mut self, Cmd::Broadcast(payload): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let id = BroadcastId { origin: self.me, seq: self.seq };
        self.record(id);
        cx.indicate(Ind::Deliver { from: self.me, msg: payload.clone() });
        self.gossip(Gossip { id, ttl: self.config.rounds, payload }, cx);
    }

    fn on_msg(&mut self, from: NodeId, msg: L::Msg, cx: &mut ProtoCx<'_, Self>) {
        self.through_link(cx, |link, ccx| link.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_link(cx, |link, ccx| link.on_timer(id, ccx));
    }

    /// Hand the scope ending down to the link. The trait's default would drop it.
    fn on_scope_event(&mut self, scope: L::Scope, cx: &mut ProtoCx<'_, Self>) {
        self.through_link(cx, |link, ccx| link.on_scope_event(scope, ccx));
    }
}
