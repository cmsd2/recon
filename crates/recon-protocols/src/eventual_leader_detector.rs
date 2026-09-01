//! Ω — an eventual leader detector.
//!
//! **Status: implementation. Space: bounded by membership.**
//!
//! Cachin, Guerraoui & Rodrigues, Module 2.9 and Algorithm 2.8 ("Monarchical Eventual Leader
//! Detection"), quoted from the book:
//!
//! ```text
//! Algorithm 2.8: Monarchical Eventual Leader Detection
//! Implements: EventualLeaderDetector, instance Ω.
//! Uses: EventuallyPerfectFailureDetector, instance ◇P.
//!
//! upon event ⟨ Ω, Init ⟩ do
//!     suspected := ∅;
//!     leader := ⊥;
//!
//! upon event ⟨ ◇P, Suspect | p ⟩ do
//!     suspected := suspected ∪ {p};
//!
//! upon event ⟨ ◇P, Restore | p ⟩ do
//!     suspected := suspected \ {p};
//!
//! upon leader ≠ maxrank(Π \ suspected) do
//!     leader := maxrank(Π \ suspected);
//!     trigger ⟨ Ω, Trust | leader ⟩;
//! ```
//!
//! # The detector beneath is a parameter, and defaults to `◇P`
//!
//! Algorithm 2.8 says `Uses: EventuallyPerfectFailureDetector`, and
//! [`crate::eventually_perfect_failure_detector`] is what that names. `D` defaults to it, so the
//! plain [`EventualLeaderDetector`] is the algorithm as written.
//!
//! It can also be composed over [`crate::perfect_failure_detector`], which is **strictly stronger**:
//! it never suspects a correct process and never retracts, so `suspected` only grows and the
//! `Restore` arm is unreachable. That composition is named in [`crate::stacks`] and was this
//! module's only form until `◇P` existed. The difference is not academic in the fail-recovery model:
//! under `P` a suspicion is permanent, `maxrank` only ever walks *downward* through the membership,
//! and a process that crashed and recovered can never lead again.
//!
//! **An Ω that is never wrong is a trap, and naming it is the point.** It makes every test of the
//! layers above vacuous: Paxos exists to stay safe while the leader detector lies, and over an
//! honest detector that property is untestable. So the suites that matter withdraw the detector's
//! accuracy — the timing assumption both detectors rest on is what they remove — and this module is
//! then wrong in exactly the way Ω is allowed to be. `tests/eventual_leader_detector.rs` checks that
//! it *can* disagree before anything is built on it.
//!
//! # `maxrank`, and what rank means here
//!
//! The book leaves `rank` as any fixed injective map from processes to integers. Here it is the
//! [`NodeId`] ordering, and `maxrank` takes the greatest — so leadership passes downward through the
//! membership as processes are suspected, and two processes with the same suspicions always agree.
//! Which direction it runs does not matter for correctness; that it is a *function of the suspected
//! set alone* does, and that is what the suite pins.
//!
//! ```text
//! ELD1 [eventual]  Eventual accuracy — eventually every correct process trusts the same correct
//!                  process. Before then it may trust a crashed process, or disagree.
//! ```
//!
//! `ELD1` inherits its condition from the detector beneath: over `◇P` it holds while `◇P2` does, and
//! `◇P2` is itself conditional on the delay cap and on the network settling. The chain is stated at
//! each link rather than collapsed into one unqualified claim.

use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use std::collections::BTreeSet;

use crate::detector::{DetectorInd, VolatileDetector};
use crate::eventually_perfect_failure_detector::{self as dp, EventuallyPerfectFailureDetector};

/// Requests from the layer above.
///
/// Uninhabited: detection begins at initialisation and there is nothing to ask for, exactly as
/// [`crate::perfect_failure_detector`] has it.
pub type Cmd = core::convert::Infallible;

/// Indications to the layer above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ind {
    /// `⟨ Ω, Trust | leader ⟩` — this process now trusts `leader`.
    ///
    /// Raised when the trusted process *changes*, and not otherwise. A layer above uses it to start
    /// an epoch, and an epoch costs an abort, so repeating an unchanged answer would be pure loss.
    Trust { leader: NodeId },
}

/// Trust the highest-ranked process not currently suspected.
#[derive(Debug)]
pub struct EventualLeaderDetector<D: VolatileDetector = EventuallyPerfectFailureDetector> {
    /// Π — every process, including this one.
    peers: BTreeSet<NodeId>,
    /// `suspected`. Grows on a suspicion and shrinks on its withdrawal — over `P`, which raises no
    /// withdrawal, it only grows.
    suspected: BTreeSet<NodeId>,
    /// `leader`. `None` is the book's `⊥`, before anything has been trusted.
    leader: Option<NodeId>,
    detector: Child<D>,
}

impl EventualLeaderDetector<EventuallyPerfectFailureDetector> {
    /// Ω among `peers`, over the eventually perfect detector Algorithm 2.8 names.
    ///
    /// `detect_after` is the silence a peer is allowed *to begin with*: `◇P` adapts it, and will
    /// suspect correct processes while it is below what the network actually needs — which is a
    /// fault this module faithfully passes on, and which the suites above deliberately provoke.
    pub fn new(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        heartbeat: core::time::Duration,
        detect_after: core::time::Duration,
    ) -> Self {
        let config = dp::Config::new(heartbeat, detect_after, detect_after * 20);
        Self::with_detector(me, peers, |me, all| {
            EventuallyPerfectFailureDetector::new(me, all, config)
        })
    }
}

impl<D: VolatileDetector> EventualLeaderDetector<D> {
    /// Ω among `peers`, over whatever detector `build` supplies.
    pub fn with_detector(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        build: impl FnOnce(NodeId, BTreeSet<NodeId>) -> D,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        let detector = build(me, peers.clone());
        EventualLeaderDetector {
            peers,
            suspected: BTreeSet::new(),
            leader: None,
            detector: Child::new(detector),
        }
    }

    /// Who this process currently trusts, if anyone.
    pub fn leader(&self) -> Option<NodeId> {
        self.leader
    }

    /// The processes currently suspected by the detector beneath, as this layer has recorded them.
    pub fn suspected(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.suspected.iter().copied()
    }

    /// `maxrank(Π \ suspected)` — the greatest [`NodeId`] not suspected.
    ///
    /// A pure function of the suspected set, which is what makes two processes with the same
    /// suspicions agree without exchanging anything.
    fn maxrank(&self) -> Option<NodeId> {
        self.peers.iter().rev().find(|p| !self.suspected.contains(p)).copied()
    }

    /// `upon leader ≠ maxrank(Π \ suspected)` — a standing condition, re-evaluated after every
    /// change to `suspected`.
    fn reconsider(&mut self, cx: &mut ProtoCx<'_, Self>) {
        let candidate = self.maxrank();
        if candidate != self.leader
            && let Some(leader) = candidate
        {
            self.leader = candidate;
            cx.indicate(Ind::Trust { leader });
        }
    }

    /// Run the detector, then act on what it reported.
    fn through_detector(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut D, &mut ProtoCx<'_, D>),
    ) {
        let mut inds = self.detector.run(cx, core::convert::identity, f);
        let changed = !inds.is_empty();
        for ind in inds.drain(..) {
            match D::classify(ind) {
                // `upon event ⟨ ◇P, Suspect | p ⟩`
                DetectorInd::Suspect { node } => {
                    self.suspected.insert(node);
                }
                // `upon event ⟨ ◇P, Restore | p ⟩`. Over a detector that never retracts this is
                // unreachable rather than merely unused — see `crate::detector`.
                DetectorInd::Restore { node } => {
                    self.suspected.remove(&node);
                }
            }
        }
        self.detector.reclaim(inds);
        if changed {
            self.reconsider(cx);
        }
    }
}

impl<D: VolatileDetector> Protocol for EventualLeaderDetector<D> {
    type Cmd = Cmd;
    type Ind = Ind;
    type Msg = D::Msg;
    /// No scope conditions of its own.
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a restarted process suspects nobody and trusts afresh.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd, _: &mut ProtoCx<'_, Self>) {
        match cmd {}
    }

    fn on_msg(&mut self, from: NodeId, msg: D::Msg, cx: &mut ProtoCx<'_, Self>) {
        self.through_detector(cx, |d, ccx| d.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_detector(cx, |d, ccx| d.on_timer(id, ccx));
    }

    /// `upon event ⟨ Ω, Init ⟩` — and immediately a first `Trust`, since with nobody suspected
    /// `maxrank(Π)` is already defined and the standing condition already holds.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.through_detector(cx, |d, ccx| d.on_init(ccx));
        self.reconsider(cx);
    }
}
