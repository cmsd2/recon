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
//! # Departure: the detector beneath is perfect, not eventually perfect
//!
//! Algorithm 2.8 says `Uses: EventuallyPerfectFailureDetector`. This repository has only
//! [`crate::perfect_failure_detector`], which is **strictly stronger**: it never suspects a correct
//! process, and it never retracts. The construction is correct over it — a detector that is right
//! from the start satisfies "eventually right" trivially — and the two `◇P` handlers collapse to
//! one, because a perfect detector raises no `Restore`. `suspected` therefore only grows.
//!
//! **This is a trap as much as a simplification, and naming it is the point.** An Ω that is never
//! wrong makes every test of the layers above it vacuous: Paxos exists to stay safe while the leader
//! detector lies, and over an honest detector that property is untestable. So the suites that matter
//! withdraw the perfect detector's accuracy — its own tests already show how, by removing the
//! synchrony assumption it rests on — and this module is then wrong in exactly the way Ω is allowed
//! to be. `tests/eventual_leader_detector.rs` checks that it *can* disagree before anything is built
//! on it, rather than discovering later that nothing was ever exercised.
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

use recon_core::{Child, NodeId, ProtoCx, Protocol, TimerId};
use std::collections::BTreeSet;

use crate::perfect_failure_detector::{self as pfd, Heartbeat, PerfectFailureDetector};

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

/// Trust the highest-ranked process not known to have crashed.
#[derive(Debug)]
pub struct EventualLeaderDetector {
    /// Π — every process, including this one.
    peers: BTreeSet<NodeId>,
    /// `suspected`. Only grows: the detector beneath never retracts, so neither does this.
    suspected: BTreeSet<NodeId>,
    /// `leader`. `None` is the book's `⊥`, before anything has been trusted.
    leader: Option<NodeId>,
    detector: Child<PerfectFailureDetector>,
}

impl EventualLeaderDetector {
    /// Ω among `peers`, over a perfect failure detector with the given heartbeat and timeout.
    ///
    /// `detect_after` must exceed `heartbeat` plus the network's delivery bound, or the detector
    /// beneath accuses correct processes — which is a fault this module faithfully passes on, and
    /// which the suites above deliberately provoke.
    pub fn new(
        me: NodeId,
        peers: impl IntoIterator<Item = NodeId>,
        heartbeat: core::time::Duration,
        detect_after: core::time::Duration,
    ) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        EventualLeaderDetector {
            peers: peers.clone(),
            suspected: BTreeSet::new(),
            leader: None,
            detector: Child::new(PerfectFailureDetector::new(me, peers, heartbeat, detect_after)),
        }
    }

    /// Who this process currently trusts, if anyone.
    pub fn leader(&self) -> Option<NodeId> {
        self.leader
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
        f: impl FnOnce(&mut PerfectFailureDetector, &mut ProtoCx<'_, PerfectFailureDetector>),
    ) {
        let mut inds = self.detector.run(cx, core::convert::identity, f);
        let changed = !inds.is_empty();
        for pfd::Ind::Crash { node } in inds.drain(..) {
            // `upon event ⟨ ◇P, Suspect | p ⟩`. There is no `Restore` arm: a perfect detector never
            // retracts, so `suspected` only grows and the set is monotone.
            self.suspected.insert(node);
        }
        self.detector.reclaim(inds);
        if changed {
            self.reconsider(cx);
        }
    }
}

impl Protocol for EventualLeaderDetector {
    type Cmd = Cmd;
    type Ind = Ind;
    type Msg = Heartbeat;
    /// No scope conditions of its own.
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a restarted process suspects nobody and trusts afresh.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd, _: &mut ProtoCx<'_, Self>) {
        match cmd {}
    }

    fn on_msg(&mut self, from: NodeId, msg: Heartbeat, cx: &mut ProtoCx<'_, Self>) {
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
