//! The Synod protocol of Multi-Paxos: ballots, acceptors, scouts, commanders and leaders.
//!
//! **Status: transcription. Space: unbounded — see the section on it below.**
//!
//! van Renesse, R. and Altinbuken, D. (2015) 'Paxos Made Moderately Complex', *ACM Computing
//! Surveys*, 47(3), pp. 1–36, §2. Figures 4, 6 and 7 are quoted above the code that implements
//! them. **The edition matters**: the 2011 Cornell technical report of the same title numbers its
//! figures differently and its acceptor differs materially from this one, so a reader checking the
//! code against the page needs to know which page. See `CLAUDE.md`, Reference material.
//!
//! The cross-check is Liu, Y.A., Chand, S. and Stoller, S.D. (2019) 'Moderately Complex Paxos Made
//! Simple', PPDP '19 — the same algorithm specified in DistAlgo with machine-checked TLA+ safety
//! proofs. It reports four liveness violations in this specification **when messages can be lost**,
//! which is this repository's setting rather than a hypothetical one, and one further issue in the
//! acceptor. Three of the four and the acceptor issue are fixed here; each is cited where it
//! departs from the survey's pseudocode.
//!
//! # What it guarantees, and what it does not
//!
//! ```text
//! S1 [always]           At most one proposal is ever chosen for a slot
//! S2 [always]           A chosen proposal is one some process proposed
//! S3 [Ω settles]        Progress — every proposed slot eventually has a proposal chosen
//! ```
//!
//! S1 and S2 hold whatever the schedule, whatever the ballots in flight and whatever has crashed.
//! S3 does not, and the source is blunt about it: §3 opens by observing that duelling leaders can
//! preempt one another for ever and that the Synod protocol guarantees nothing about progress
//! "even in the absence of any failure whatsoever".
//!
//! # Roles, and where each one lives
//!
//! §4.4 says the roles are co-located on one machine in practice, and that is the shape built here:
//! one protocol per process holding **both** the acceptor and the leader. Neither has a vocabulary
//! the other does not, and both send and receive on the same wire, so composing them as children
//! would buy two wrap functions and a message split that carries nothing.
//!
//! Scouts and commanders are the leader's own bookkeeping rather than child protocols, for the
//! question `link.rs` asks of everything: does this thing have a vocabulary of its own? It does
//! not. What a scout is, concretely, is a `waitfor` set and a union of pvalues; what a commander
//! is, is a `waitfor` set and the pvalue it is responsible for. The source models them as threads
//! because it is written in a language where a thread is the cheapest way to say "wait for a
//! majority". Here they are the leader's own `scout` and `commanders` fields, and the mapping
//! from each figure's `for ever / switch receive` arm to where it went is stated above the
//! handler that took it.
//!
//! The source's constraints on their number are in the types rather than in a comment: at most one
//! scout, "and only for its own ballots", is an `Option`; at most one commander per `⟨ballot,
//! slot⟩` (Invariant C1) is a map keyed by slot, cleared when the ballot changes.
//!
//! # Departure: a `p2b` names its slot
//!
//! Figure 4 answers a `p2a` with `⟨p2b, self(), ballot_num⟩` — no slot. It does not need one:
//! the reply goes to the commander thread that sent the request, and the thread's identity is what
//! routes it. Folding commanders into the leader removes that identity, and a leader running
//! commanders for several slots at once cannot tell from `⟨p2b, α, b⟩` which of them answered. So
//! the slot travels in the message.
//!
//! This is the structural decision above paying for itself, and it is the whole of the price: no
//! guarantee changes, because the slot was already determined by the request the reply answers.
//! `⟨p1b, α, b, r⟩` needs no such addition — a leader runs at most one scout, so its ballot routes
//! it.
//!
//! # Departure: a reply naming a ballot *below* the attempt's is stale, and is discarded
//!
//! The same cause, and it is the half that bites. Figures 6(a) and 6(b) branch on `b' = b` and
//! treat everything else as a preemption, which is sound for a thread: a reply to a request the
//! thread did not send cannot reach it, because the thread that sent that request has exited and
//! the reply is addressed to it. A scout that is a *field* has no such address, so a late answer
//! to the **previous** attempt arrives at the current one.
//!
//! Taking the figure literally there is a liveness bug, and it was measured before this guard
//! existed. A leader is preempted, climbs, and starts a scout for the new ballot; the second
//! acceptor's answer to the *old* ballot then arrives, fails `b' = b`, and takes the `else` arm —
//! which discards the running scout and reports a preemption the leader correctly ignores, because
//! the ballot in it is below the one it now holds. The leader is left with no scout, not active,
//! and nothing in the sweep to restart it. It stops for ever, while Ω goes on trusting it.
//!
//! So a reply is classified against the attempt it reaches: **above** its ballot is a preemption,
//! **equal** is an answer, and **below** is an answer to an attempt that has already exited and is
//! discarded. Under Figure 6's own model the third case cannot arise, which is why the figure does
//! not name it.
//!
//! # Departure: the acceptor accepts under `b ≥ ballot_num`, and adopts what it accepts
//!
//! Figure 4 accepts a pvalue only when `b = ballot_num`, and Figure 6(a)'s commander treats any
//! reply naming a different ballot as a preemption. The two compose only because §2.3 asserts that
//! every `p2b` carries `b' ≥ b`, and **that assertion rests on the acceptor having seen phase 1
//! before phase 2** — which the link beneath this module does not guarantee.
//!
//! The case is concrete rather than theoretical. An acceptor's `p1a` dies at a session ending; the
//! scout completes with a majority that excludes it; the retransmitted `p2a` then reaches an
//! acceptor whose `ballot_num` is below `b`. Under the survey's text that acceptor refuses,
//! answers with its own lower ballot, and the commander reads the mismatch as a preemption and
//! exits — while the leader ignores the preemption, because the ballot in it is *below* its own.
//! The slot then has no commander, so the retry sweep no longer covers it, and nothing moves until
//! the phase-two escalation notices a whole timeout later.
//!
//! So the acceptor here takes the 2011 report's condition — `if b ≥ ballot_num then ballot_num :=
//! b; accepted := accepted ∪ {⟨b,s,c⟩}` — which is also the fix Liu et al. give for what they name
//! the useless-replies issue in this acceptor. Every `p2b` then names a ballot at least as high as
//! the request's, the commander's else-arm is a genuine preemption again, and the property §2.3
//! asserts without support holds here by construction.
//!
//! **Safety is unaffected**, and this is the one place to be sure of it. Invariant A1 — an acceptor
//! adopts strictly increasing ballots — still holds: `ballot_num` only ever moves up. A2 becomes
//! "accepts `⟨b,s,c⟩` only where `b = ballot_num` **after** the adoption in the same transition",
//! which is what the argument in §2.4 actually uses: what matters is that an acceptor which has
//! adopted `b` never afterwards accepts below `b`, and adopting on the way in makes that *more*
//! true rather than less. A4 is unaffected because it is enforced by the leader (C1), not the
//! acceptor.
//!
//! # Departure: liveness comes from Ω, not from the source's pinging
//!
//! §3 has a preempted leader monitor the preempting one by pinging it on a regular basis, backing
//! off with an AIMD timeout, and says outright that "this concept is called failure detection".
//! This module composes [`EventualLeaderDetector`] instead and acts on `Trust`: it starts a scout
//! for its next ballot when trusted, and stays passive otherwise. A preemption moves `ballot_num`
//! past the ballot that beat it but starts no scout unless this process is still trusted, which is
//! what stops the duel §3 opens by describing.
//!
//! What differs: Ω names **one** leader, where the source's scheme lets any correct leader win a
//! race and merely makes the loser wait longer each time. Ω is the stronger assumption and the
//! cheaper mechanism, and it is conditional on everything its own chain is conditional on —
//! stated at each link in `docs/conditional-guarantees.md` rather than collapsed into one claim
//! here. What it costs is that a correct process Ω does not trust will not lead however long it
//! waits.
//!
//! # Departure: three liveness fixes from the cross-check
//!
//! Each is reachable here because this link loses messages, and each is Liu et al.'s.
//!
//! | Where | What is lost | What happens | What this leader does |
//! |---|---|---|---|
//! | Phase 1 | `p1a` | no `p1b` majority and no preemption ever arrives, so the leader waits for ever | restarts phase one after a timeout |
//! | Phase 2 | `p2b` | no decision for that slot, and if it happens at every leader the layer above stalls too | resends `p2a` for that slot on each tick |
//! | Phase 2 | `preempt` | a majority has moved to a higher ballot, so `p2a` can never reach one — the leader sends for ever and decides nothing | restarts **phase one** after a timeout |
//!
//! The third is the one a naive design gets wrong. Resending `p2a` cannot help once a majority
//! holds a higher ballot; the leader has to go back to phase 1. A single retransmission sweep over
//! unanswered requests recovers from the first two and loops for ever on the third.
//!
//! **Both restarts rerun phase one under the same ballot.** For a lost `p1a` the rerun is
//! idempotent: an acceptor that already adopted the ballot answers again, and the scout recollects.
//! For a lost preemption the rerun is how the leader learns what it missed — acceptors that have
//! moved answer `p1b` naming their higher ballot, the scout reports that as a preemption, and only
//! then does `ballot_num` move. Minting a higher ballot on every timeout would discard phase-two
//! work already accepted under the current ballot and learn nothing a rerun does not.
//!
//! The fourth violation is in the replica, which this module does not have: if no decision arrives
//! for a slot, every replica stops applying from that slot, `slot_out` stops moving, `WINDOW` fills
//! and the system wedges. Its fix is for a replica to re-propose after a timeout. Recorded here so
//! that whoever writes the replica does not have to rediscover it.
//!
//! # Space
//!
//! **Unbounded, and this is a transcription.** An acceptor keeps every pvalue it has ever accepted
//! and sends the whole set in every `p1b`; a leader keeps a proposal for every slot it has been
//! asked about. Both grow with the number of slots handled, which `docs/bounded-space.md` forbids
//! of an implementation.
//!
//! That is the source's §2, which is explicitly the impractical version — §4 opens by saying "the
//! described protocol is not practical" and gives the reductions that bound it: §4.1 has an
//! acceptor keep only the most recently accepted pvalue per slot, and §4.2 collects state below a
//! watermark once at least `f + 1` replicas have learned a decision, carrying the collected slot
//! number in `p1b` so a later leader does not read absence as "nothing was ever accepted". Both
//! belong to a later change, because bounding weakens a guarantee to a scope.
//!
//! What *is* bounded is the retry sweep, and it is worth stating precisely because the obvious
//! claim is wrong. The sweep visits the outstanding `waitfor` sets: each is bounded by membership,
//! but their number is not — it grows with the slots proposed and not yet decided. So the sweep
//! costs membership times the slots in flight. A decision retires its commander and leaves the
//! sweep, so once the work completes the sweep is empty, which is the window
//! `tests/common::assert_send_rate_flat!` measures. Nothing here resends history.
//!
//! # The boundary this module does not cross
//!
//! This is **crash-stop**, which is the source's own model rather than a scope dodge. A crashed
//! state machine there "will make no more transitions and thus its current state is fixed
//! indefinitely", and a process that comes back off disk "is not theoretically considered
//! crashed—it is simply slow for a while". There is no third case, and a process that returns
//! having forgotten what it knew is the first one acting when it is not permitted to.
//!
//! The simulator can produce that case, and Ω will trust such a process again, so the consequence
//! has to be stated rather than assumed away. The round counter is volatile: **its scope is this
//! incarnation.** A process that restarts re-mints a ballot it has already used; an acceptor still
//! holding that ballot accepts a second, different proposal under it; and two proposals accepted at
//! one ballot and slot is exactly what Invariant A4 forbids and what the argument that two
//! majorities agree depends on. A **durable ballot counter**, read back in `on_recovery`, is what
//! makes leading again after a restart legal, and it belongs with the rest of §4.3 (*Keeping State
//! on Disk*) in the fail-recovery change.
//!
//! A returning process could instead lead under a **new identity**, which is sound for the leader
//! role — ballot uniqueness is a proposer-side obligation and the proposer set need not be fixed —
//! and unsound for the acceptor role, because majorities of two different acceptor sets need not
//! intersect. The source's own answer to that is reconfiguration (§5's Cheap Paxos "reconfigures
//! the system replacing the suspected acceptor with a fresh one"), decided in a slot like any other
//! command. There are no slots to decide it in until the replica exists, so the membership here is
//! **fixed for the run**.
//!
//! # The figures
//!
//! ```text
//! process Acceptor()
//!   var ballot_num := ⊥, accepted := ∅;
//!
//!   for ever
//!     switch receive()
//!       case ⟨p1a, λ, b⟩ :
//!         if b > ballot_num then
//!           ballot_num := b;
//!         end if
//!         send(λ, ⟨p1b, self(), ballot_num, accepted⟩);
//!       end case
//!       case ⟨p2a, λ, ⟨b, s, c⟩⟩ :
//!         if b = ballot_num then
//!           accepted := accepted ∪ {⟨b, s, c⟩};
//!         end if
//!         send(λ, ⟨p2b, self(), ballot_num⟩);
//!       end case
//!     end switch
//!   end for
//! end process
//! ```
//! Figure 4. Pseudocode for an acceptor. The `p2a` arm departs — see above.
//!
//! ```text
//! process Commander(λ, acceptors, replicas, ⟨b, s, c⟩)
//!   var waitfor := acceptors;
//!
//!   ∀α ∈ acceptors : send(α, ⟨p2a, self(), ⟨b, s, c⟩⟩);
//!   for ever
//!     switch receive()
//!       case ⟨p2b, α, b'⟩ :
//!         if b' = b then
//!           waitfor := waitfor − {α};
//!           if |waitfor| < |acceptors|/2 then
//!             ∀ρ ∈ replicas :
//!               send(ρ, ⟨decision, s, c⟩);
//!             exit();
//!           end if
//!         else
//!           send(λ, ⟨preempted, b'⟩);
//!           exit();
//!         end if
//!       end case
//!     end switch
//!   end for
//! end process
//!
//! process Scout(λ, acceptors, b)
//!   var waitfor := acceptors, pvalues := ∅;
//!
//!   ∀α ∈ acceptors : send(α, ⟨p1a, self(), b⟩);
//!   for ever
//!     switch receive()
//!       case ⟨p1b, α, b', r⟩ :
//!         if b' = b then
//!           pvalues := pvalues ∪ r;
//!           waitfor := waitfor − {α};
//!           if |waitfor| < |acceptors|/2 then
//!             send(λ, ⟨adopted, b, pvalues⟩);
//!             exit();
//!           end if
//!         else
//!           send(λ, ⟨preempted, b'⟩);
//!           exit();
//!         end if
//!       end case
//!     end switch
//!   end for
//! end process
//! ```
//! Figure 6. (a) a commander, (b) a scout.
//!
//! ```text
//! process Leader(acceptors, replicas)
//!   var ballot_num = (0, self()), active = false, proposals = ∅;
//!
//!   spawn(Scout(self(), acceptors, ballot_num));
//!   for ever
//!     switch receive()
//!       case ⟨propose, s, c⟩ :
//!         if ∄c' : ⟨s, c'⟩ ∈ proposals then
//!           proposals := proposals ∪ {⟨s, c⟩};
//!           if active then
//!             spawn(Commander(self(), acceptors, replicas, ⟨ballot_num, s, c⟩));
//!           end if
//!         end if
//!       end case
//!       case ⟨adopted, ballot_num, pvals⟩ :
//!         proposals := proposals ◁ pmax(pvals);
//!         ∀⟨s, c⟩ ∈ proposals :
//!           spawn(Commander(self(), acceptors, replicas, ⟨ballot_num, s, c⟩));
//!         active := true;
//!       end case
//!       case ⟨preempted, ⟨r', λ'⟩⟩ :
//!         if ⟨r', λ'⟩ > ballot_num then
//!           active := false;
//!           ballot_num := (r' + 1, self());
//!           spawn(Scout(self(), acceptors, ballot_num));
//!         end if
//!       end case
//!     end switch
//!   end for
//! end process
//! ```
//! Figure 7. Pseudocode skeleton for a leader. The `spawn(Scout(…))` calls depart — see above.

use core::marker::PhantomData;
use core::time::Duration;
use recon_core::{Child, NodeId, ProtoCx, Protocol, Time, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::eventual_leader_detector::{self as eld, EventualLeaderDetector};
use crate::link::{Boundary, LinkInd, VolatileLink};

use crate::perfect_failure_detector::Heartbeat;
use crate::session_link::SessionLink;
use crate::{Note, Timing};

/// A slot number. Orthogonal to a ballot number, as Figure 5 has it: one ballot can decide many
/// slots, and one slot may be targeted by many ballots.
pub type Slot = u64;

/// A ballot number: `⟨round, leader⟩`, ordered lexicographically.
///
/// §2.2 makes ballot numbers "lexicographically ordered pairs of an integer and its leader
/// identifier (consequently, leader identifiers need to be totally ordered)". Two consequences the
/// derived ordering gives for free: any two ballots are comparable, and a ballot names its leader,
/// so it is trivial to see who owns one.
///
/// The source's `⊥` is `Option::<Ballot>::None`, whose derived ordering already places it below
/// every `Some` — which is exactly "ordered before any normal ballot number".
///
/// **The round counter is volatile, and its scope is this incarnation.** See the module
/// documentation on the boundary this module does not cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Ballot {
    /// The integer half. Increases when this process is preempted.
    pub round: u64,
    /// The leader half, which makes two processes' ballots distinct however far their rounds drift.
    pub leader: NodeId,
}

impl Ballot {
    /// `(0, self())` — Figure 7's initial ballot number.
    pub fn initial(leader: NodeId) -> Self {
        Ballot { round: 0, leader }
    }

    /// `(r' + 1, self())` — the ballot to take up after being preempted by `beat_by`.
    pub fn above(beat_by: Ballot, me: NodeId) -> Self {
        Ballot { round: beat_by.round + 1, leader: me }
    }
}

impl core::fmt::Display for Ballot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "({},{})", self.round, self.leader)
    }
}

/// `p = ⟨b, s, c⟩` — a ballot number, a slot number and a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pvalue<C> {
    pub ballot: Ballot,
    pub slot: Slot,
    pub command: C,
}

/// A set of pvalues, keyed so that Invariant A4 — at most one command per ballot and slot — is the
/// map's own property rather than something to be checked.
///
/// The book writes this as a set and unions into it. A `BTreeMap` keyed by `⟨ballot, slot⟩` is the
/// same object with the invariant made structural, and it keeps the ordered-map rule without asking
/// the command type for an ordering.
pub type Pvalues<C> = BTreeMap<(Ballot, Slot), C>;

/// What this layer puts on the wire, beneath the link.
///
/// The whole of Figures 4 and 6: the acceptor's two requests and its two replies. `adopted`,
/// `preempted` and `decision` are not here — the first two are a thread reporting to the leader
/// that owns it, which is a function call once the thread is a field, and the third is addressed to
/// a replica this change does not have and is raised as an indication instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SynodMsg<C> {
    /// `⟨p1a, λ, b⟩` — phase one, from a scout.
    P1a { ballot: Ballot },
    /// `⟨p1b, α, ballot_num, accepted⟩` — an acceptor's answer, carrying everything it has accepted.
    P1b { ballot: Ballot, accepted: Vec<Pvalue<C>> },
    /// `⟨p2a, λ, ⟨b, s, c⟩⟩` — phase two, from a commander.
    P2a { pvalue: Pvalue<C> },
    /// `⟨p2b, α, ballot_num⟩`, **plus the slot it answers for**. Figure 4 has no slot, because the
    /// reply goes to a thread whose identity supplies it; a leader holding its commanders as fields
    /// needs it in the message. See the module documentation.
    P2b { ballot: Ballot, slot: Slot },
}

/// The wire, multiplexing the leader detector and the Synod protocol itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<M> {
    /// The leader detector's heartbeats.
    Detector(Heartbeat),
    /// The Synod protocol's traffic, as the link beneath wraps it.
    Synod(M),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<C> {
    /// `⟨propose, s, c⟩` — propose `command` for `slot`.
    Propose { slot: Slot, command: C },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<C> {
    /// `send(ρ, ⟨decision, s, c⟩)` — a majority of acceptors accepted this pvalue, so `command` is
    /// chosen for `slot`. Figure 6(a) addresses this to the replicas; there is no replica here, so
    /// it is raised to whatever is above.
    Decision { slot: Slot, command: C },
    /// The scope with `peer` ended at `epoch`. **Propagated, not absorbed**: this layer holds no
    /// redundancy that outlives a session — what it knows of a ballot is in memory a crash takes,
    /// and its redundancy is the other processes rather than a resend across an ending. A layer
    /// above may need to know that an answer it was waiting for will not arrive.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`.
    SessionEstablished { peer: NodeId, epoch: u64 },
}

/// Figure 6(b)'s scout, as a field rather than a thread.
///
/// `var waitfor := acceptors, pvalues := ∅` is the whole of its state. Its `∀α ∈ acceptors :
/// send(α, ⟨p1a, self(), b⟩)` prologue is in [`MultiPaxosSynod::start_scout`]; its `⟨p1b, α, b',
/// r⟩` arm is in [`MultiPaxosSynod::on_p1b`], whose two branches are the figure's `then` and
/// `else`; and `exit()` is the `Option` becoming `None`.
#[derive(Debug)]
struct Scout<C> {
    /// `b` — the ballot this scout is running phase one for.
    ballot: Ballot,
    /// `waitfor` — the acceptors that have not yet answered.
    waitfor: BTreeSet<NodeId>,
    /// `pvalues` — the union of what the answers carried.
    pvalues: Pvalues<C>,
    /// When phase one began, for the escalation. Not the figure's: the figure waits for ever.
    started: Time,
}

/// Figure 6(a)'s commander, as a field rather than a thread.
///
/// `var waitfor := acceptors`, plus the pvalue it is responsible for, which the figure passes as a
/// constructor argument. Its prologue is in [`MultiPaxosSynod::start_commander`]; its `⟨p2b, α,
/// b'⟩` arm is in [`MultiPaxosSynod::on_p2b`]; `exit()` is removal from the map.
#[derive(Debug)]
struct Commander<C> {
    /// `b` — the ballot this commander is running phase two under.
    ballot: Ballot,
    /// `c` — the command it is trying to get chosen. Held rather than looked up in `proposals`,
    /// because it is the figure's own constructor argument and because a resend needs it.
    command: C,
    /// `waitfor` — the acceptors that have not yet answered.
    waitfor: BTreeSet<NodeId>,
    /// When phase two began for this slot, for the escalation.
    started: Time,
}

/// `pmax(pvalues) ≡ {⟨s, c⟩ | ∃b : ⟨b, s, c⟩ ∈ pvalues ∧ ∀b', c' : ⟨b', s, c'⟩ ∈ pvalues ⇒ b' ≤ b}`
///
/// For each slot, the command of the pvalue with the maximum ballot number. Invariant A4 makes that
/// command unique — there cannot be two different commands for the same ballot and slot — which is
/// why taking one is well defined rather than a choice.
///
/// The map iterates in `⟨ballot, slot⟩` order, so a later entry for a slot always carries a
/// strictly higher ballot than an earlier one and the last one written wins.
fn pmax<C: Clone>(pvalues: &Pvalues<C>) -> BTreeMap<Slot, C> {
    let mut best: BTreeMap<Slot, C> = BTreeMap::new();
    for ((_, slot), command) in pvalues {
        best.insert(*slot, command.clone());
    }
    best
}

/// `|waitfor| < |acceptors|/2` — the figures' majority test.
///
/// Written as a multiplication because the figures' division is over the reals. In Rust
/// `waitfor.len() < acceptors.len() / 2` truncates: with five acceptors it reads `< 2`, which
/// demands four answers rather than three, so a run losing two acceptors would never adopt or
/// decide. Safety would survive that and liveness would not, silently.
fn is_majority(waitfor: usize, acceptors: usize) -> bool {
    waitfor * 2 < acceptors
}

/// The Synod protocol: acceptor and leader in one process, as §4.4 co-locates them.
///
/// `L` is the link beneath, a parameter rather than a fixed type, and it defaults to
/// [`SessionLink`] rather than to the perfect link. That is deliberate: the real-world set's first
/// obligation is running over a session link, and this is the first module here that can meet it
/// before joining rather than after. A boundary is classified and propagated; this layer bridges
/// nothing.
#[derive(Debug)]
pub struct MultiPaxosSynod<C: Clone, L: VolatileLink<SynodMsg<C>> = SessionLink<SynodMsg<C>>> {
    me: NodeId,
    /// The acceptor set, over which majorities are counted. **Fixed for the run** — see the module
    /// documentation on why changing it without deciding the change is unsound.
    acceptors: BTreeSet<NodeId>,

    // ---- the acceptor, Figure 4 ----
    /// `α.ballot_num`, initially `⊥`.
    ballot_num: Option<Ballot>,
    /// `α.accepted`, initially `∅`. Grows with the slots handled; see the module's space section.
    accepted: Pvalues<C>,

    // ---- the leader, Figure 7 ----
    /// `λ.ballot_num`, initially `(0, self())`.
    leader_ballot: Ballot,
    /// `λ.active`.
    active: bool,
    /// `λ.proposals` — at most one entry per slot, which the map makes structural.
    proposals: BTreeMap<Slot, C>,
    /// The scout, if one is running. `Option` rather than a set, because a leader "starts at most
    /// one of these for any ballot `b`, and only for its own ballots".
    scout: Option<Scout<C>>,
    /// The commanders, keyed by slot **within the current ballot** — Invariant C1. Cleared when the
    /// ballot changes, which is what makes the key a slot rather than a `⟨ballot, slot⟩` pair.
    commanders: BTreeMap<Slot, Commander<C>>,
    /// Whether Ω currently trusts this process. The departure from §3: a leader is passive by
    /// default and competes only while trusted.
    trusted: bool,

    // ---- retries ----
    /// The periodic sweep's handle. Compared before acting, since an expiry is offered to every
    /// layer and this one composes two children that register their own.
    tick: Option<TimerId>,
    /// How often the sweep runs, and so how often an unanswered request is resent.
    retry: Duration,
    /// How long an attempt may go without adopting, deciding or being preempted before it restarts
    /// phase one. Taken from the detector's own timeout rather than adding a knob: it is already
    /// configured as "long enough that silence means something is wrong", which is exactly the
    /// question being asked.
    escalate_after: Duration,

    omega: Child<EventualLeaderDetector>,
    link: Child<L>,
    _command: PhantomData<fn() -> C>,
}

impl<C: Clone, L: VolatileLink<SynodMsg<C>>> MultiPaxosSynod<C, L> {
    /// The Synod protocol among `acceptors`, over a link the caller supplies.
    pub fn with_link(
        me: NodeId,
        acceptors: impl IntoIterator<Item = NodeId>,
        timing: Timing,
        link: L,
    ) -> Self {
        let Timing { retransmit, heartbeat, detect_after } = timing;
        let mut acceptors: BTreeSet<NodeId> = acceptors.into_iter().collect();
        acceptors.insert(me);
        MultiPaxosSynod {
            me,
            omega: Child::new(EventualLeaderDetector::new(
                me,
                acceptors.clone(),
                heartbeat,
                detect_after,
            )),
            acceptors,
            ballot_num: None,
            accepted: BTreeMap::new(),
            leader_ballot: Ballot::initial(me),
            active: false,
            proposals: BTreeMap::new(),
            scout: None,
            commanders: BTreeMap::new(),
            trusted: false,
            tick: None,
            retry: retransmit,
            escalate_after: detect_after,
            link: Child::new(link),
            _command: PhantomData,
        }
    }

    /// `α.ballot_num` — the ballot this process has adopted as an acceptor, or `⊥`.
    pub fn adopted_ballot(&self) -> Option<Ballot> {
        self.ballot_num
    }

    /// `λ.ballot_num` — the ballot this process is leading with.
    pub fn leader_ballot(&self) -> Ballot {
        self.leader_ballot
    }

    /// `λ.active` — whether phase one has completed for the current ballot.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether Ω currently trusts this process.
    pub fn is_trusted(&self) -> bool {
        self.trusted
    }

    /// How many pvalues this acceptor holds. Grows with the slots handled — the measurement
    /// `docs/bounded-space.md` wants for a transcription.
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    /// The slots this leader currently has a commander for.
    pub fn commanded_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.commanders.keys().copied()
    }

    /// Whether a scout is running phase one.
    pub fn is_scouting(&self) -> bool {
        self.scout.is_some()
    }
}

impl<C: Clone> MultiPaxosSynod<C, SessionLink<SynodMsg<C>>> {
    /// The Synod protocol among `acceptors`, over the session link this module defaults to.
    pub fn new(me: NodeId, acceptors: impl IntoIterator<Item = NodeId>, timing: Timing) -> Self {
        Self::with_link(me, acceptors, timing, SessionLink::new())
    }
}

impl<C, L> MultiPaxosSynod<C, L>
where
    C: Clone,
    L: VolatileLink<SynodMsg<C>>,
{
    // ---------------------------------------------------------------- the acceptor, Figure 4

    /// `case ⟨p1a, λ, b⟩ : if b > ballot_num then ballot_num := b; end if;`
    /// `send(λ, ⟨p1b, self(), ballot_num, accepted⟩);`
    ///
    /// The reply carries `ballot_num` **after** any adoption, so a scout whose ballot was refused is
    /// told which ballot beat it rather than merely that one did.
    fn on_p1a(&mut self, from: NodeId, ballot: Ballot, cx: &mut ProtoCx<'_, Self>) {
        if Some(ballot) > self.ballot_num {
            self.ballot_num = Some(ballot);
        }
        // Always `Some` here: either it just adopted, or it already held something at least as
        // high, and a real ballot is above `⊥`.
        let held = self.ballot_num.expect("an acceptor answering p1a has adopted something");
        let accepted = self
            .accepted
            .iter()
            .map(|((ballot, slot), command)| Pvalue {
                ballot: *ballot,
                slot: *slot,
                command: command.clone(),
            })
            .collect();
        self.transmit(from, SynodMsg::P1b { ballot: held, accepted }, cx);
    }

    /// `case ⟨p2a, λ, ⟨b, s, c⟩⟩ : if b ≥ ballot_num then ballot_num := b; accepted := accepted ∪
    /// {⟨b, s, c⟩}; end if; send(λ, ⟨p2b, self(), ballot_num⟩);`
    ///
    /// **Departed** from Figure 4's `b = ballot_num`, which does not adopt. The module documentation
    /// gives the failure this repairs, the edition the condition comes from, and why A1 and A2
    /// survive it.
    fn on_p2a(&mut self, from: NodeId, pvalue: Pvalue<C>, cx: &mut ProtoCx<'_, Self>) {
        let Pvalue { ballot, slot, command } = pvalue;
        // `synod-accept-below-promise` is a mutation the safety-evidence guard compiles: an
        // acceptor that accepts under a ballot it has already superseded, and answers as though it
        // had not. It breaks A2 and nothing else, so a suite that stays green under it is not
        // reading agreement. See `scripts/check-safety-tests.sh`.
        let sabotaged = cfg!(feature = "synod-accept-below-promise");
        let admissible = sabotaged || Some(ballot) >= self.ballot_num;
        if admissible {
            // Still monotonic even under the mutation: A1 is a separate claim and stays true, so
            // exactly one invariant is removed at a time.
            self.ballot_num = Some(self.ballot_num.map_or(ballot, |held| held.max(ballot)));
            self.accepted.insert((ballot, slot), command);
        }
        let held = if sabotaged {
            ballot
        } else {
            self.ballot_num.expect("an acceptor answering p2a has adopted something")
        };
        self.transmit(from, SynodMsg::P2b { ballot: held, slot }, cx);
    }

    // ---------------------------------------------------------------- the scout, Figure 6(b)

    /// `∀α ∈ acceptors : send(α, ⟨p1a, self(), b⟩)`, and the state the figure's `var` line declares.
    ///
    /// Called for a new ballot and again to restart phase one under the same one. Restarting
    /// discards a partial `waitfor` and `pvalues` deliberately: an acceptor that already answered
    /// answers again, and re-collecting from scratch is what makes the restart idempotent.
    fn start_scout(&mut self, ballot: Ballot, cx: &mut ProtoCx<'_, Self>) {
        self.scout = Some(Scout {
            ballot,
            waitfor: self.acceptors.clone(),
            pvalues: BTreeMap::new(),
            started: cx.now(),
        });
        for a in self.acceptors.clone() {
            self.transmit(a, SynodMsg::P1a { ballot }, cx);
        }
    }

    /// `case ⟨p1b, α, b', r⟩` — the figure's `then` branch collects, and its `else` reports a
    /// preemption to the leader. Both are here, because the leader is this same object.
    fn on_p1b(
        &mut self,
        from: NodeId,
        ballot: Ballot,
        accepted: Vec<Pvalue<C>>,
        cx: &mut ProtoCx<'_, Self>,
    ) {
        let Some(scout) = self.scout.as_mut() else { return };
        if ballot < scout.ballot {
            // A late answer to a scout that has already exited. Figure 6(b) never sees one: the
            // reply is addressed to a thread that no longer exists. See the module's departure on
            // what a scout being a field costs.
            return;
        }
        if ballot != scout.ballot {
            // `else send(λ, ⟨preempted, b'⟩); exit();`
            self.scout = None;
            self.preempted(ballot, cx);
            return;
        }
        // `pvalues := pvalues ∪ r; waitfor := waitfor − {α};`
        for Pvalue { ballot, slot, command } in accepted {
            scout.pvalues.insert((ballot, slot), command);
        }
        scout.waitfor.remove(&from);
        if is_majority(scout.waitfor.len(), self.acceptors.len()) {
            // `send(λ, ⟨adopted, b, pvalues⟩); exit();`
            let scout = self.scout.take().expect("borrowed above");
            self.adopted(scout.ballot, scout.pvalues, cx);
        }
    }

    // ---------------------------------------------------------------- the commander, Figure 6(a)

    /// `∀α ∈ acceptors : send(α, ⟨p2a, self(), ⟨b, s, c⟩⟩)`, and the figure's `var` line.
    fn start_commander(&mut self, slot: Slot, command: C, cx: &mut ProtoCx<'_, Self>) {
        let ballot = self.leader_ballot;
        // Invariant C1: at most one commander for ⟨b, s⟩. The map key enforces it within a ballot,
        // and the ballot changing clears the map.
        self.commanders.insert(
            slot,
            Commander {
                ballot,
                command: command.clone(),
                waitfor: self.acceptors.clone(),
                started: cx.now(),
            },
        );
        for a in self.acceptors.clone() {
            let pvalue = Pvalue { ballot, slot, command: command.clone() };
            self.transmit(a, SynodMsg::P2a { pvalue }, cx);
        }
    }

    /// `case ⟨p2b, α, b'⟩` — collect toward a majority, or report the preemption.
    ///
    /// The slot comes from the message rather than from the thread's identity; see the module
    /// documentation.
    fn on_p2b(&mut self, from: NodeId, ballot: Ballot, slot: Slot, cx: &mut ProtoCx<'_, Self>) {
        let Some(commander) = self.commanders.get_mut(&slot) else { return };
        if ballot < commander.ballot {
            // A late answer to a commander that has already exited — see `on_p1b`.
            return;
        }
        if ballot != commander.ballot {
            // `else send(λ, ⟨preempted, b'⟩); exit();`
            self.commanders.remove(&slot);
            self.preempted(ballot, cx);
            return;
        }
        commander.waitfor.remove(&from);
        if is_majority(commander.waitfor.len(), self.acceptors.len()) {
            // `∀ρ ∈ replicas : send(ρ, ⟨decision, s, c⟩); exit();`
            let commander = self.commanders.remove(&slot).expect("borrowed above");
            cx.indicate(Ind::Decision { slot, command: commander.command });
        }
    }

    // ---------------------------------------------------------------- the leader, Figure 7

    /// `case ⟨adopted, ballot_num, pvals⟩`.
    ///
    /// `proposals := proposals ◁ pmax(pvals)` — the update operator replaces this leader's proposal
    /// for a slot with the highest-ballot pvalue any acceptor in the majority reported, and keeps
    /// this leader's own proposal for a slot nobody reported. **This is the step the whole safety
    /// argument rests on**: a proposal already accepted by a majority under a lower ballot is what
    /// the new leader proposes, whatever it set out to propose.
    fn adopted(&mut self, ballot: Ballot, pvalues: Pvalues<C>, cx: &mut ProtoCx<'_, Self>) {
        // "If an `adopted` message arrives for an old ballot number, it is ignored."
        if ballot != self.leader_ballot {
            return;
        }
        // `synod-ignore-pmax` is the guard's other mutation: a leader that proposes what it set
        // out to propose rather than what the majority already accepted. It removes the one step
        // the safety argument rests on and leaves everything else working.
        if !cfg!(feature = "synod-ignore-pmax") {
            for (slot, command) in pmax(&pvalues) {
                self.proposals.insert(slot, command);
            }
        }
        self.active = true;
        // `∀⟨s, c⟩ ∈ proposals : spawn(Commander(…))`
        for (slot, command) in self.proposals.clone() {
            self.start_commander(slot, command, cx);
        }
    }

    /// `case ⟨preempted, ⟨r', λ'⟩⟩ : if ⟨r', λ'⟩ > ballot_num then active := false; ballot_num :=
    /// (r' + 1, self()); spawn(Scout(…)); end if;`
    ///
    /// **Departed** in one clause: the scout starts only if Ω still trusts this process. Figure 7
    /// starts one unconditionally, which is the duel §3 opens by describing.
    fn preempted(&mut self, beat_by: Ballot, cx: &mut ProtoCx<'_, Self>) {
        if beat_by <= self.leader_ballot {
            return;
        }
        self.active = false;
        self.leader_ballot = Ballot::above(beat_by, self.me);
        // The ballot changed, so every commander was running under the old one. C1 keys on the
        // ballot; the map keys on the slot, and this is what keeps the two in step.
        self.commanders.clear();
        self.scout = None;
        if self.trusted {
            let ballot = self.leader_ballot;
            self.start_scout(ballot, cx);
        } else {
            // Nothing whatever reaches the trace from here: a leader that stops competing sends no
            // message, sets no timer and raises no indication. That is the shape of silence this
            // repository narrates.
            cx.note(Note::LeadershipYielded { to: beat_by.leader, round: beat_by.round });
        }
    }

    /// `upon event ⟨ Ω, Trust | p ⟩` — the departure §3 is replaced by.
    ///
    /// Figure 7 spawns a scout for its initial ballot at startup, unconditionally. Here a process
    /// scouts when trusted and stays passive otherwise, so a process Ω does not trust starts no
    /// ballot and the duel does not begin.
    fn on_trust(&mut self, leader: NodeId, cx: &mut ProtoCx<'_, Self>) {
        self.trusted = leader == self.me;
        if self.trusted && self.scout.is_none() && !self.active {
            let ballot = self.leader_ballot;
            self.start_scout(ballot, cx);
        }
    }

    /// `case ⟨propose, s, c⟩ : if ∄c' : ⟨s, c'⟩ ∈ proposals then …`
    fn on_propose(&mut self, slot: Slot, command: C, cx: &mut ProtoCx<'_, Self>) {
        if self.proposals.contains_key(&slot) {
            // No effect at all: the proposal is dropped on the floor because this leader already
            // has one for the slot, which is what enforces C1 against a second commander.
            cx.note(Note::ProposalIgnored { slot });
            return;
        }
        self.proposals.insert(slot, command.clone());
        if self.active {
            self.start_commander(slot, command, cx);
        }
        // While passive the proposal is remembered and nothing is sent; `adopted` spawns the
        // commander for it, which is Figure 7's `if active then` arm doing nothing.
    }

    // ---------------------------------------------------------------- retries and escalation

    /// The periodic sweep: resend what is outstanding, and escalate an attempt that has waited too
    /// long. See the module's table of the three liveness fixes.
    fn sweep(&mut self, cx: &mut ProtoCx<'_, Self>) {
        let now = cx.now();
        if let Some(scout) = self.scout.as_ref() {
            let ballot = scout.ballot;
            if now - scout.started >= self.escalate_after {
                // Phase 1, lost `p1a`: neither adopted nor preempted. Restart, same ballot.
                self.start_scout(ballot, cx);
            } else {
                for a in scout.waitfor.clone() {
                    self.transmit(a, SynodMsg::P1a { ballot }, cx);
                }
            }
            return;
        }
        if !self.active {
            return;
        }
        let stalled = self
            .commanders
            .values()
            .any(|commander| now - commander.started >= self.escalate_after);
        if stalled {
            // Phase 2, lost `preempt`: a majority may hold a higher ballot, in which case no `p2a`
            // can ever reach one and resending is a loop. Go back to phase one, same ballot — the
            // `p1b` answers are how the higher ballot is learnt.
            self.active = false;
            self.commanders.clear();
            let ballot = self.leader_ballot;
            self.start_scout(ballot, cx);
            return;
        }
        // Phase 2, lost `p2b`: resend the pvalue to whoever has not answered.
        let outstanding: Vec<(Slot, Ballot, C, Vec<NodeId>)> = self
            .commanders
            .iter()
            .map(|(slot, c)| {
                (*slot, c.ballot, c.command.clone(), c.waitfor.iter().copied().collect())
            })
            .collect();
        for (slot, ballot, command, waitfor) in outstanding {
            for a in waitfor {
                let pvalue = Pvalue { ballot, slot, command: command.clone() };
                self.transmit(a, SynodMsg::P2a { pvalue }, cx);
            }
        }
    }

    /// Arm the sweep, or re-arm it after it fired.
    fn arm(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.tick = Some(cx.set_timer(self.retry));
    }

    // ---------------------------------------------------------------- composition

    /// Put one message on the wire, through the link beneath.
    fn transmit(&mut self, to: NodeId, msg: SynodMsg<C>, cx: &mut ProtoCx<'_, Self>) {
        self.through_link(cx, |l, ccx| l.on_cmd(L::send(to, msg), ccx));
    }

    fn through_omega(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut EventualLeaderDetector, &mut ProtoCx<'_, EventualLeaderDetector>),
    ) {
        let mut inds = self.omega.run(cx, Wire::Detector, f);
        for eld::Ind::Trust { leader } in inds.drain(..) {
            self.on_trust(leader, cx);
        }
        self.omega.reclaim(inds);
    }

    fn through_link(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut L, &mut ProtoCx<'_, L>),
    ) {
        let mut inds = self.link.run(cx, Wire::Synod, f);
        for ind in inds.drain(..) {
            match L::classify(ind) {
                LinkInd::Deliver { from, msg } => self.on_synod_msg(from, msg, cx),
                // Propagated, never absorbed: nothing here bridges a session ending.
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

    /// The four `switch receive()` arms of Figures 4 and 6, dispatched.
    fn on_synod_msg(&mut self, from: NodeId, msg: SynodMsg<C>, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            SynodMsg::P1a { ballot } => self.on_p1a(from, ballot, cx),
            SynodMsg::P1b { ballot, accepted } => self.on_p1b(from, ballot, accepted, cx),
            SynodMsg::P2a { pvalue } => self.on_p2a(from, pvalue, cx),
            SynodMsg::P2b { ballot, slot } => self.on_p2b(from, ballot, slot, cx),
        }
    }
}

impl<C, L> Protocol for MultiPaxosSynod<C, L>
where
    C: Clone,
    L: VolatileLink<SynodMsg<C>>,
{
    type Cmd = Cmd<C>;
    type Ind = Ind<C>;
    type Msg = Wire<L::Msg>;
    /// Whatever the link's guarantees are conditional on. This layer adds no condition of its own
    /// and bridges none of the link's.
    type Scope = L::Scope;
    type Note = crate::Note;
    /// Keeps nothing durably, which is what makes this crash-stop. §4.3 is the change that alters
    /// it, and the module documentation says what a durable ballot counter would buy.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Propose { slot, command }: Cmd<C>, cx: &mut ProtoCx<'_, Self>) {
        self.on_propose(slot, command, cx);
    }

    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Detector(h) => self.through_omega(cx, |o, ccx| o.on_msg(from, h, ccx)),
            Wire::Synod(m) => self.through_link(cx, |l, ccx| l.on_msg(from, m, ccx)),
        }
    }

    /// An expiry is offered to every layer, so both children are given it and this layer acts only
    /// on the handle it registered.
    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_omega(cx, |o, ccx| o.on_timer(id, ccx));
        self.through_link(cx, |l, ccx| l.on_timer(id, ccx));
        if self.tick != Some(id) {
            return;
        }
        self.arm(cx);
        self.sweep(cx);
    }

    /// `⟨ Init ⟩` — start the detector, whose first `Trust` may immediately make this process scout,
    /// and arm the sweep.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.arm(cx);
        self.through_omega(cx, |o, ccx| o.on_init(ccx));
    }

    /// Hand the boundary down to the link, which is the layer that knows what it means. Leaving
    /// this to the trait's default would take a scope event the driver raised and drop it — the
    /// cardinal sin of `docs/conditional-guarantees.md`.
    fn on_scope_event(&mut self, scope: L::Scope, cx: &mut ProtoCx<'_, Self>) {
        self.through_link(cx, |l, ccx| l.on_scope_event(scope, ccx));
    }
}
