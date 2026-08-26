//! Flooding consensus.
//!
//! Cachin, Guerraoui & Rodrigues, Module 5.1 and Algorithm 5.1 ("Flooding Consensus").
//!
//! **Status: academic, fail-stop. Space: bounded by membership and rounds.** The state is one
//! proposal set and one heard-from set per round entered, each holding at most one entry per
//! process, and a run enters at most `N` rounds — so it is `O(N²)` and satisfies the rule in
//! `docs/bounded-space.md` without any collection being added. That is not what makes it
//! academic. What makes it academic is the assumption underneath it.
//!
//! Processes flood their accumulated proposal sets in rounds. A process leaves a round when it
//! has heard, in that round, from every process it has not been told has crashed. If a round ends
//! having heard from exactly the same processes as the one before it, nobody new crashed, so
//! everyone holds the same proposal set and it is safe to decide the minimum of it.
//!
//! ```text
//! upon event ⟨ c, Init ⟩ do
//!     correct := Π;
//!     round := 1;
//!     decision := ⊥;
//!     receivedfrom := [∅]^N;
//!     proposals := [∅]^N;
//!     receivedfrom[0] := Π;
//!
//! upon event ⟨ P, Crash | p ⟩ do
//!     correct := correct \ {p};
//!
//! upon event ⟨ c, Propose | v ⟩ do
//!     proposals[1] := proposals[1] ∪ {v};
//!     trigger ⟨ beb, Broadcast | [PROPOSAL, 1, proposals[1]] ⟩;
//!
//! upon event ⟨ beb, Deliver | p, [PROPOSAL, r, ps] ⟩ do
//!     receivedfrom[r] := receivedfrom[r] ∪ {p};
//!     proposals[r] := proposals[r] ∪ ps;
//!
//! upon correct ⊆ receivedfrom[round] ∧ decision = ⊥ do
//!     if receivedfrom[round] = receivedfrom[round − 1] then
//!         decision := min(proposals[round]);
//!         trigger ⟨ beb, Broadcast | [DECIDED, decision] ⟩;
//!         trigger ⟨ c, Decide | decision ⟩;
//!     else
//!         round := round + 1;
//!         trigger ⟨ beb, Broadcast | [PROPOSAL, round, proposals[round − 1]] ⟩;
//!
//! upon event ⟨ beb, Deliver | p, [DECIDED, v] ⟩ such that p ∈ correct ∧ decision = ⊥ do
//!     decision := v;
//!     trigger ⟨ beb, Broadcast | [DECIDED, decision] ⟩;
//!     trigger ⟨ c, Decide | decision ⟩;
//! ```
//!
//! # Agreement rests entirely on strong accuracy
//!
//! `correct` appears in exactly two places, and a false suspicion corrupts both. The round guard
//! is `correct ⊆ receivedfrom[round]`, so wrongly shrinking `correct` lets a process finish a
//! round *without having heard from a correct process*, and two processes can then take `min`
//! over different sets. The decision-adoption rule is guarded by `p ∈ correct`, so a process that
//! wrongly suspects the decider discards its `DECIDED` message. The book's own proof names the
//! dependency: "Because of the *strong accuracy* property of the failure detector, no process
//! that reaches the end of round r receives a proposal containing a smaller value than v."
//!
//! Losing the detector's **accuracy** costs *safety* — two correct processes decide differently,
//! permanently. Losing its **completeness** costs only *liveness* — everyone blocks, but nobody
//! is wrong. The asymmetry is the reason this rung is worth writing.
//!
//! # Why stabilising later does not help
//!
//! The model these algorithms are written against is not one in which the set of correct
//! processes decays. It is one of eventual stability: bounds come to hold, and after that point
//! the correct set is agreed and stays agreed. `docs/scope-annotated-modules.md` names this
//! Assumption F and observes that it is the partial-synchrony global stabilisation time and the ◇
//! of an eventually-accurate detector in the same clothes.
//!
//! An eventually perfect detector would therefore *withdraw* a false suspicion, and every process
//! would again be held correct by every other — and the split would still be there, because a
//! decision is irrevocable and was taken while the system was unstable. This is what separates
//! this rung from the leader-driven family: flooding consensus commits during instability and so
//! has nothing left for stabilisation to rescue, whereas a quorum-based algorithm declines to
//! commit until no conflicting decision is possible. Stated this way the limitation survives
//! replacing `P` with `◇P`; "the detector never withdraws an accusation" would not.
//!
//! # Departures from the page
//!
//! - `receivedfrom` and `proposals` are maps keyed by round rather than arrays of size `N`. Only
//!   rounds actually entered hold an entry; the bound is the same and for the same reason.
//! - The total order the book assumes on proposals ("we implicitly assume here that the set of
//!   all possible proposals is totally ordered and the order is known by all processes") is a
//!   `P: Ord` bound. A value that cannot be totally ordered cannot be proposed.
//! - The standing condition is re-evaluated in a loop rather than once, because a message for a
//!   later round may arrive before this process enters it. It terminates: the guard requires this
//!   process to appear in `receivedfrom[round]`, which happens only when its own broadcast for
//!   that round returns to it.
//! - A second `Propose` from the same process is ignored. The book's model has one proposal per
//!   process, and Module 5.1 provides one decision per instance.
//! - `⟨c, Init⟩` is not a separate event. `new` establishes the state, and [`Cmd::Start`] begins
//!   failure detection, without which no round can ever complete after a crash.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, absurd};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use crate::perfect_failure_detector::{self as pfd, Heartbeat, PerfectFailureDetector};
use crate::perfect_link as pl;

/// What this layer puts on the wire: the two messages Algorithm 5.1 sends, and nothing else.
///
/// The round number is the one field this layer adds, and it adds it because it is the one thing
/// this layer keeps that its children do not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// The derived bound would be `P: Deserialize`; rebuilding the set on the way in also needs the
// total order the algorithm assumes anyway.
#[serde(bound(deserialize = "P: Ord + Deserialize<'de>"))]
pub enum Flood<P> {
    Proposal { round: u64, proposals: BTreeSet<P> },
    Decided(P),
}

/// What best-effort broadcast puts on the wire for this layer's payloads.
///
/// Written concretely rather than as a projection, for the reason given in
/// [`crate::uniform_reliable_broadcast::BebMsg`]. The assertion below keeps the two in step.
pub type BebMsg<P> = pl::Wire<Flood<P>>;

const _: () = {
    /// Fails to compile if best-effort broadcast ever puts something else on the wire.
    fn _beb_msg_is_what_we_say_it_is<P: Clone>(
        m: BebMsg<P>,
    ) -> <BestEffortBroadcast<Flood<P>> as Protocol>::Msg {
        m
    }
};

/// The wire type, multiplexing the two children. Typed, so a mis-route cannot compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = "P: Ord + Deserialize<'de>"))]
pub enum Wire<P> {
    Broadcast(BebMsg<P>),
    Detector(Heartbeat),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    /// Begin failure detection. Without it no round can complete once a process has crashed.
    Start,
    Propose(P),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    Decide(P),
}

/// Timers, which are the children's re-wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    Broadcast(beb::Timer),
    Detector(pfd::Tick),
}

/// Regular consensus in the fail-stop model, over best-effort broadcast and a failure detector.
#[derive(Debug)]
pub struct FloodingConsensus<P> {
    /// Every process believed correct. Shrinks on a crash indication and never grows.
    correct: BTreeSet<NodeId>,
    round: u64,
    decision: Option<P>,
    proposed: bool,
    /// Who this process has heard from in each round. `receivedfrom[0]` is the full membership.
    receivedfrom: BTreeMap<u64, BTreeSet<NodeId>>,
    /// The proposals accumulated in each round.
    proposals: BTreeMap<u64, BTreeSet<P>>,
    beb: BestEffortBroadcast<Flood<P>>,
    detector: PerfectFailureDetector,
    beb_inbox: Vec<beb::Ind<Flood<P>>>,
    det_inbox: Vec<pfd::Ind>,
    /// Sending re-enters the child while its own inbox is in use, so it needs a buffer of its
    /// own. By construction it stays empty; the assertion in `send` records why.
    send_inbox: Vec<beb::Ind<Flood<P>>>,
}

impl<P> FloodingConsensus<P> {
    /// Consensus among `members`, which must include `me`.
    ///
    /// `detect_after` must exceed `heartbeat` plus the network's delivery bound, or the detector
    /// will accuse correct processes and agreement can break — which is the whole subject of this
    /// module's documentation.
    pub fn new(
        me: NodeId,
        members: impl IntoIterator<Item = NodeId>,
        retransmit: Duration,
        heartbeat: Duration,
        detect_after: Duration,
    ) -> Self {
        let mut members: BTreeSet<NodeId> = members.into_iter().collect();
        members.insert(me);
        // `receivedfrom[0] := Π` — which is what makes a first-round decision require having
        // heard from every process, not merely from everyone still believed correct.
        let receivedfrom = BTreeMap::from([(0, members.clone())]);
        FloodingConsensus {
            correct: members.clone(),
            round: 1,
            decision: None,
            proposed: false,
            receivedfrom,
            proposals: BTreeMap::new(),
            beb: BestEffortBroadcast::new(me, members.clone(), retransmit),
            detector: PerfectFailureDetector::new(me, members, heartbeat, detect_after),
            beb_inbox: Vec::new(),
            det_inbox: Vec::new(),
            send_inbox: Vec::new(),
        }
    }

    /// The processes still believed correct, in a stable order.
    pub fn correct(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.correct.iter().copied()
    }

    /// The round this process is currently in.
    pub fn round(&self) -> u64 {
        self.round
    }

    /// What this process decided, if it has.
    pub fn decision(&self) -> Option<&P> {
        self.decision.as_ref()
    }

    /// Who this process heard from in `round`, for tests watching the guard form.
    pub fn heard_from(&self, round: u64) -> impl Iterator<Item = NodeId> + '_ {
        self.receivedfrom.get(&round).into_iter().flatten().copied()
    }

    /// How many rounds hold state. Bounded by the membership; see the space note above.
    pub fn rounds_recorded(&self) -> usize {
        self.receivedfrom.len().max(self.proposals.len())
    }

    /// Every entry held across every round — the measure a bounded-space test asserts on.
    pub fn state_entries(&self) -> usize {
        let heard: usize = self.receivedfrom.values().map(|s| s.len()).sum();
        let props: usize = self.proposals.values().map(|s| s.len()).sum();
        heard + props + self.correct.len()
    }
}

impl<P: Clone + Ord> FloodingConsensus<P> {
    /// Run the broadcast child, then act on what it reported.
    fn with_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut BestEffortBroadcast<Flood<P>>,
            &mut ProtoCx<'_, BestEffortBroadcast<Flood<P>>>,
        ),
    ) {
        let mut inbox = core::mem::take(&mut self.beb_inbox);
        inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(Wire::Broadcast, Timer::Broadcast, absurd, &mut inbox, |ccx| {
                f(beb, ccx)
            });
        }
        for ind in inbox.drain(..) {
            let beb::Ind::Deliver { from, msg } = ind;
            self.on_beb_deliver(from, msg, cx);
        }
        self.beb_inbox = inbox;
        self.check_round(cx);
    }

    /// Run the detector child, then act on what it reported.
    fn with_detector(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut PerfectFailureDetector, &mut ProtoCx<'_, PerfectFailureDetector>),
    ) {
        let mut inbox = core::mem::take(&mut self.det_inbox);
        inbox.clear();
        {
            let detector = &mut self.detector;
            cx.with_child_consuming(Wire::Detector, Timer::Detector, absurd, &mut inbox, |ccx| {
                f(detector, ccx)
            });
        }
        for ind in inbox.drain(..) {
            let pfd::Ind::Crash { node } = ind;
            // `upon event ⟨ P, Crash | p ⟩ do correct := correct \ {p}`. Permanent: this
            // detector has no Restore, and nothing here would act on one if it did.
            self.correct.remove(&node);
        }
        self.det_inbox = inbox;
        // A crash alone can complete a round, by shrinking `correct` to a set already heard
        // from. That is why the guard is checked here and not only on the message path.
        self.check_round(cx);
    }

    /// `upon event ⟨ beb, Deliver | p, ... ⟩`.
    fn on_beb_deliver(&mut self, from: NodeId, msg: Flood<P>, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Flood::Proposal { round, proposals } => {
                self.receivedfrom.entry(round).or_default().insert(from);
                self.proposals.entry(round).or_default().extend(proposals);
            }
            // `such that p ∈ correct ∧ decision = ⊥`. A process that wrongly suspects the
            // decider discards this, which is half of why a false suspicion is unrecoverable.
            Flood::Decided(v) if self.correct.contains(&from) && self.decision.is_none() => {
                self.decide(v, cx);
            }
            Flood::Decided(_) => {}
        }
    }

    /// `upon correct ⊆ receivedfrom[round] ∧ decision = ⊥`.
    ///
    /// A standing condition over state, not an event handler: its inputs change both when a
    /// message arrives and when `correct` shrinks, so it is evaluated from both paths.
    ///
    /// Looped, because advancing a round can immediately satisfy the guard again — messages for
    /// a later round may already have arrived. It terminates because the guard requires this
    /// process to be in `receivedfrom[round]`, and this process only appears there once its own
    /// broadcast for that round has come back to it.
    ///
    /// Calling it from the detector path is load-bearing and easy to think redundant. It is not:
    /// after a crash, no further consensus message need ever arrive, so the message path may
    /// never run again. Under this stack it *looks* redundant, because the stubborn link
    /// retransmits for ever and its timer re-enters the broadcast child often enough to
    /// re-evaluate the guard by accident. That is a property of the link, not of this layer, and
    /// a link that does not retransmit — the session link, for one — would remove it. The test
    /// `a_round_completes_on_a_crash_indication_alone` drives the protocol directly rather than
    /// through a run, for exactly this reason.
    fn check_round(&mut self, cx: &mut ProtoCx<'_, Self>) {
        while self.decision.is_none() && self.round_complete() {
            if self.heard(self.round) == self.heard(self.round - 1) {
                // Nobody new crashed during the round, so every process that reaches its end
                // holds the same proposal set, and `min` needs no further communication.
                let Some(v) = self.proposals.get(&self.round).and_then(|p| p.first()).cloned()
                else {
                    // Vacuous: the guard is only satisfiable once this process has heard its own
                    // proposal for the round, which carries at least one value.
                    debug_assert!(false, "a completed round must hold at least one proposal");
                    return;
                };
                self.decide(v, cx);
            } else {
                self.round += 1;
                // `[PROPOSAL, round, proposals[round − 1]]` — the *previous* round's set, under
                // the new round's number. Rendering this as `proposals[round]` compiles and is
                // silently wrong until a crash cascade exposes it.
                let carried = self.heard_proposals(self.round - 1);
                self.send(Flood::Proposal { round: self.round, proposals: carried }, cx);
            }
        }
    }

    fn round_complete(&self) -> bool {
        let heard = self.receivedfrom.get(&self.round);
        self.correct.iter().all(|p| heard.is_some_and(|h| h.contains(p)))
    }

    fn heard(&self, round: u64) -> BTreeSet<NodeId> {
        self.receivedfrom.get(&round).cloned().unwrap_or_default()
    }

    fn heard_proposals(&self, round: u64) -> BTreeSet<P> {
        self.proposals.get(&round).cloned().unwrap_or_default()
    }

    fn decide(&mut self, v: P, cx: &mut ProtoCx<'_, Self>) {
        self.decision = Some(v.clone());
        self.send(Flood::Decided(v.clone()), cx);
        cx.indicate(Ind::Decide(v));
    }

    fn send(&mut self, msg: Flood<P>, cx: &mut ProtoCx<'_, Self>) {
        let mut send_inbox = core::mem::take(&mut self.send_inbox);
        send_inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(
                Wire::Broadcast,
                Timer::Broadcast,
                absurd,
                &mut send_inbox,
                |ccx| beb.on_cmd(beb::Cmd::Broadcast(msg), ccx),
            );
        }
        debug_assert!(
            send_inbox.is_empty(),
            "sending must not deliver synchronously; if it does, check_round can recurse"
        );
        self.send_inbox = send_inbox;
    }
}

impl<P: Clone + Ord> Protocol for FloodingConsensus<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    type Timer = Timer;
    /// No session beneath, so no scope end can be constructed — as for both children.
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Durable = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            Cmd::Start => self.with_detector(cx, |d, ccx| d.on_cmd(pfd::Cmd::Start, ccx)),
            // One proposal per process, one decision per instance; a second is not a second
            // consensus and is ignored.
            Cmd::Propose(_) if self.proposed => {}
            Cmd::Propose(v) => {
                self.proposed = true;
                self.proposals.entry(1).or_default().insert(v);
                let ps = self.heard_proposals(1);
                self.send(Flood::Proposal { round: 1, proposals: ps }, cx);
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Broadcast(m) => self.with_beb(cx, |beb, ccx| beb.on_msg(from, m, ccx)),
            Wire::Detector(h) => self.with_detector(cx, |d, ccx| d.on_msg(from, h, ccx)),
        }
    }

    fn on_timer(&mut self, token: Timer, cx: &mut ProtoCx<'_, Self>) {
        match token {
            Timer::Broadcast(t) => self.with_beb(cx, |beb, ccx| beb.on_timer(t, ccx)),
            Timer::Detector(t) => self.with_detector(cx, |d, ccx| d.on_timer(t, ccx)),
        }
    }
}
