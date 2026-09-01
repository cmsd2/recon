//! Paxos that survives a restart.
//!
//! **Status: implementation. Space: bounded by membership, plus what the stubborn children hold
//! outstanding — inherited from [`crate::logged_epoch_change`] and
//! [`crate::logged_epoch_consensus`], and retired for the consensus by each epoch's `Abort`.**
//!
//! Cachin, Guerraoui & Rodrigues, Module 5.5 (`LoggedUniformConsensus`) and Algorithms 5.10–5.11
//! ("Logged Leader-Driven Consensus"), quoted from the book:
//!
//! ```text
//! Algorithm 5.10: Logged Leader-Driven Consensus (part 1)
//! Implements: LoggedUniformConsensus, instance luc.
//! Uses:
//!     LoggedEpochChange, instance lec;
//!     LoggedEpochConsensus (multiple instances).
//!
//! upon event ⟨ luc, Init ⟩ do
//!     val := ⊥; decision := ⊥; aborted := FALSE; proposed := FALSE;
//!     Obtain the initial leader ℓ0 from the logged epoch-change instance lec;
//!     Initialize a new instance lep.0 of logged epoch consensus with timestamp 0, leader ℓ0,
//!         and state (0, ⊥);
//!     (ets, ℓ) := (0, ℓ0);
//!     store(ets, ℓ, decision);
//!
//! upon event ⟨ luc, Recovery ⟩ do
//!     retrieve(ets, ℓ, decision);
//!     retrieve(startts, start) of instance lec;
//!     (newts, newℓ) := (startts, start);
//!     retrieve(epochdecision) of instance lep.ets;
//!     if epochdecision ≠ ⊥ ∧ decision = ⊥ then
//!         decision := epochdecision;
//!         store(decision);
//!         trigger ⟨ luc, Decide | decision ⟩;
//!     aborted := FALSE;
//!
//! upon event ⟨ luc, Propose | v ⟩ do
//!     val := v;
//!
//! Algorithm 5.11: Logged Leader-Driven Consensus (part 2)
//!
//! upon event ⟨ lec, StartEpoch | startts, start ⟩ do
//!     retrieve(startts, start) of instance lec;
//!     (newts, newℓ) := (startts, start);
//!
//! upon (ets, ℓ) ≠ (newts, newℓ) ∧ aborted = FALSE do
//!     aborted := TRUE;
//!     trigger ⟨ lep.ets, Abort ⟩;
//!
//! upon event ⟨ lep.ts, Aborted | state ⟩ such that ts = ets do
//!     (ets, ℓ) := (newts, newℓ);
//!     store(ets, ℓ);
//!     aborted := FALSE;
//!     proposed := FALSE;
//!     Initialize a new instance lep.ets of logged epoch consensus with timestamp ets, leader ℓ,
//!         and state state;
//!
//! upon ℓ = self ∧ val ≠ ⊥ ∧ proposed = FALSE do
//!     proposed := TRUE;
//!     trigger ⟨ lep.ets, Propose | val ⟩;
//!
//! upon event ⟨ lep.ts, Decide | epochdecision ⟩ such that ts = ets do
//!     retrieve(epochdecision) of instance lep.ets;
//!     if decision = ⊥ then
//!         decision := epochdecision;
//!         store(decision);
//!         trigger ⟨ luc, Decide | decision ⟩;
//! ```
//!
//! # Two durable children under one durable parent
//!
//! This is the first protocol here that keeps a record of its own *and* composes children that keep
//! records of theirs, and it is the reason [`recon_core::Slot`] exists. Every other composition
//! hands its children a `NoStore`, because a parent and a child sharing one store would each
//! overwrite the other's metadata — and would do it silently, with nothing failing until a recovery
//! read back half of what it wrote.
//!
//! A slot names the part of [`Durable`] that belongs to a child. The child's `set` becomes a
//! read-modify-write of this record: **one write, not two**, so a crash cannot land between the
//! parent's record and its child's.
//!
//! The book writes `retrieve(startts, start) of instance lec` and `retrieve(epochdecision) of
//! instance lep.ets` — a parent reading its children's records by name. Here it does that by
//! handing each child its slot and letting the child's own `Recovery` read it, which is the same
//! thing said in the direction the composition already runs.
//!
//! # One slot for a child that is replaced every epoch
//!
//! The book has one logged epoch consensus instance per timestamp, each with its own record. There
//! is one slot here, holding whichever instance is live. That is not a loss: the only instance ever
//! read is `lep.ets`, and `ets` is in this record too, so a slot holding the current instance is
//! exactly what `retrieve(...) of instance lep.ets` asks for.
//!
//! A crash can land between `store(ets, ℓ)` and the new instance's own `Init` write, and both
//! outcomes are safe. Land before it, and recovery reads the *previous* epoch's `epochdecision`
//! against the *new* `ets` — but a value that epoch decided is, by lock-in, the value every later
//! epoch decides, so deciding it is right. Land after it, and recovery reads a fresh record against
//! the old `ets` and has simply not decided yet.
//!
//! # Reading two lines the page does not quite give
//!
//! `upon (ets, ℓ) = (newts, newℓ) ∧ aborted = FALSE do … trigger ⟨ lep.ets, Abort ⟩` is printed
//! with `=`, and must be `≠`: aborting the epoch you are in because it *is* the one you want would
//! abort every epoch immediately and decide nothing. The same OCR class as `if v ≠ ⊥ then tmpval :=
//! v` in Algorithm 5.6, which [`crate::epoch_consensus`] records for the same reason.
//!
//! `Init` does not print an assignment to `(newts, newℓ)`. It must be `(0, ℓ0)` — the same pair as
//! `(ets, ℓ)` — or the standing condition above is true from the first event and the initial epoch
//! is aborted before it does anything. Algorithm 5.7 sets `(newts, newℓ) := (0, ⊥)`, which works
//! there because its condition is written on the `Aborted` handler rather than as a standing one.
//!
//! # The decision is announced again after a recovery, and Module 5.5 has no integrity property
//!
//! `⟨ luc, Decide | decision ⟩` is specified as "notifies the upper layer that variable `decision`
//! in stable storage contains the decided value of consensus" — a pointer to a record, not a
//! one-shot event. Module 5.5 lists **three** properties where the fail-noisy Module 5.2 lists
//! four: termination, validity and uniform agreement, with *no* integrity clause. The book dropped
//! it, and the reason is exactly this: a logged indication may be raised again, and the layer above
//! reads storage and must be idempotent. `logged_link` and `logged_uniform_reliable_broadcast`
//! already work that way, and `README.md` states it as the rule for this whole model.
//!
//! So a process that decided, crashed, and came back announces its decision once more. Algorithm
//! 5.10's `Recovery` handler as printed does not — it announces only when `epochdecision ≠ ⊥ ∧
//! decision = ⊥`, the case where the child's record survived and this layer's did not. Re-announcing
//! the other case is the departure, and it is the module's own indication wording taken at face
//! value: a layer above that crashed with this one never saw the first indication, and there is no
//! other event that would tell it.
//!
//! ```text
//! LUC1 [conditional] Termination — every correct process that never crashes eventually
//!                    log-decides, provided a majority is correct and the leader detector settles.
//!                    "Correct" here means eventually up and staying up, so a process that keeps
//!                    crashing is not owed a decision
//! LUC2 [always]      Validity — a log-decided value was proposed by some process
//! LUC3 [always]      Uniform agreement — no two processes log-decide differently, **including
//!                    across crashes and recoveries, and while the leader detector is wrong**
//! ```
//!
//! There is deliberately no integrity clause, for the reason above. What replaces it is that the
//! *value* never changes: a process announces the same decision every time, which
//! [`LoggedLeaderDrivenConsensus::decision`] is the durable statement of.

use recon_core::{Child, NodeId, ProtoCx, Protocol, Slot, TimerId, slot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::Timing;
use crate::logged_epoch_change::{self as lec, LoggedEpochChange};
use crate::logged_epoch_consensus::{self as lep, LoggedEpochConsensus, State};

/// The wire, multiplexing the epoch-change child and whichever epoch-consensus instance is live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<V> {
    /// The logged epoch-change child's traffic.
    Change(lec::Wire),
    /// The live logged epoch-consensus instance's traffic.
    ///
    /// The epoch tag that makes `lep.ts` addressable lives inside the child — see
    /// [`lep::Tagged`] — because the epoch is the instance's own identity.
    Consensus(lep::Wire<V>),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<V> {
    /// `⟨ luc, Propose | v ⟩`.
    Propose(V),
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<V> {
    /// `⟨ luc, Decide | v ⟩`. Raised at most once, and only after the decision is durable.
    Decide(V),
}

/// Everything this stack keeps durably, as one rewritten value.
///
/// The two `Option` fields are the children's slots. They are `Option` because a child may not have
/// written yet, and because [`Slot::write`] has to be able to build this record from nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Durable<V> {
    /// `ets`.
    pub ets: u64,
    /// `ℓ`. `None` before this process's own `Init` has written, which is when it is `ℓ0`.
    pub leader: Option<NodeId>,
    /// `decision`.
    pub decision: Option<V>,
    /// `lec`'s record: `(startts, start)`.
    pub lec: Option<lec::Started>,
    /// `lep.ets`'s record: `(valts, val)` and `epochdecision`.
    pub lep: Option<lep::Durable<V>>,
}

impl<V> Default for Durable<V> {
    fn default() -> Self {
        Durable { ets: 0, leader: None, decision: None, lec: None, lep: None }
    }
}

/// Paxos in the fail-recovery model.
pub struct LoggedLeaderDrivenConsensus<V: Clone> {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    timing: Timing,
    /// `ℓ0` — the initial leader, re-derived from the membership rather than stored.
    l0: NodeId,
    /// `val`.
    val: Option<V>,
    /// `proposed`.
    proposed: bool,
    /// `decision` — durable.
    decision: Option<V>,
    /// `aborted` — whether an abort is outstanding.
    aborted: bool,
    /// `(ets, ℓ)` — durable.
    ets: u64,
    leader: NodeId,
    /// `(newts, newℓ)` — the epoch the change child has told this process to enter.
    newts: u64,
    newleader: NodeId,
    lec: Child<LoggedEpochChange>,
    /// `lep.ets`. One instance, replaced on each epoch change, sharing one slot.
    lep: Child<LoggedEpochConsensus<V>>,
}

/// Where `lec`'s record sits inside this one.
fn lec_slot<V: Clone>() -> Slot<Durable<V>, lec::Started> {
    slot!(Durable<V>, lec)
}

/// Where the live `lep` instance's record sits inside this one.
fn lep_slot<V: Clone>() -> Slot<Durable<V>, lep::Durable<V>> {
    slot!(Durable<V>, lep)
}

impl<V: Clone + PartialEq> LoggedLeaderDrivenConsensus<V> {
    /// Paxos among `peers`, over stable storage.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, timing: Timing) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        // "Obtain the initial leader ℓ0 from the logged epoch-change instance lec" — `maxrank(Π)`,
        // which is what Ω trusts first with nobody suspected, so the two agree before anything
        // moves. A function of the membership, so it survives a restart without being stored.
        let l0 = peers.iter().next_back().copied().expect("Π contains at least this process");
        LoggedLeaderDrivenConsensus {
            me,
            peers: peers.clone(),
            timing,
            l0,
            val: None,
            proposed: false,
            decision: None,
            aborted: false,
            ets: 0,
            leader: l0,
            // `(newts, newℓ) := (0, ℓ0)`. See the module's note on the two lines the page does not
            // quite give: `(0, ⊥)` would abort the initial epoch before it did anything.
            newts: 0,
            newleader: l0,
            lec: Child::new(LoggedEpochChange::new(me, peers.clone(), timing)),
            lep: Child::new(LoggedEpochConsensus::new(
                me,
                peers,
                0,
                l0,
                State::default(),
                timing.retransmit,
            )),
        }
    }

    /// The epoch now live at this process.
    pub fn epoch(&self) -> u64 {
        self.ets
    }

    /// Who leads the epoch now live.
    pub fn leader(&self) -> NodeId {
        self.leader
    }

    /// What this process decided, if it has.
    pub fn decision(&self) -> Option<&V> {
        self.decision.as_ref()
    }

    /// `(valts, val)` as the epoch now live holds it — what the abort handshake carries forward.
    pub fn state(&self) -> &State<V> {
        self.lep.state()
    }

    /// This process's own part of the durable record, with the children's slots carried across
    /// unchanged.
    ///
    /// The parent writes the whole record, so it has to preserve what the children put in it —
    /// the mirror of what [`Slot`] does for a child's write.
    fn record(&self, cx: &mut ProtoCx<'_, Self>) -> Durable<V> {
        let held = cx.storage().get().cloned();
        Durable {
            ets: self.ets,
            leader: Some(self.leader),
            decision: self.decision.clone(),
            lec: held.as_ref().and_then(|d| d.lec),
            lep: held.and_then(|d| d.lep),
        }
    }

    fn store(&mut self, cx: &mut ProtoCx<'_, Self>) {
        let record = self.record(cx);
        cx.storage().set(record);
    }

    /// `upon ℓ = self ∧ val ≠ ⊥ ∧ proposed = FALSE` — a standing condition, re-evaluated whenever
    /// any of the three could have changed.
    fn maybe_propose(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.leader == self.me
            && !self.proposed
            && let Some(v) = self.val.clone()
        {
            self.proposed = true;
            self.through_lep(cx, |e, ccx| e.on_cmd(lep::Cmd::Propose(v), ccx));
        }
    }

    /// `upon (ets, ℓ) ≠ (newts, newℓ) ∧ aborted = FALSE do aborted := TRUE; trigger ⟨ Abort ⟩`.
    fn maybe_abort(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if (self.ets, self.leader) != (self.newts, self.newleader) && !self.aborted {
            self.aborted = true;
            self.through_lep(cx, |e, ccx| e.on_cmd(lep::Cmd::Abort, ccx));
        }
    }

    /// `upon event ⟨ lep.ts, Aborted | state ⟩ such that ts = ets`.
    ///
    /// **`store(ets, ℓ)` precedes the new instance**, and the new instance's own `Init` write
    /// follows it. Neither order is safe on its own; what makes both safe is that the pair is
    /// read back together — see the module's note on one slot for a replaced child.
    fn on_aborted(&mut self, state: State<V>, cx: &mut ProtoCx<'_, Self>) {
        if !self.aborted {
            // The book's `such that ts = ets`: an answer from an instance already superseded.
            return;
        }
        self.ets = self.newts;
        self.leader = self.newleader;
        self.aborted = false;
        self.proposed = false;
        self.store(cx);
        self.lep.replace(LoggedEpochConsensus::new(
            self.me,
            self.peers.clone(),
            self.ets,
            self.leader,
            state,
            self.timing.retransmit,
        ));
        self.through_lep(cx, |e, ccx| e.on_init(ccx));
        self.maybe_propose(cx);
    }

    /// `upon event ⟨ lep.ts, Decide | epochdecision ⟩ such that ts = ets`.
    ///
    /// The child has already made `epochdecision` durable before raising this — that is
    /// [`crate::logged_epoch_consensus`]'s own obligation — and this handler makes `decision`
    /// durable before reporting it, which is this one's.
    fn on_epoch_decision(&mut self, v: V, cx: &mut ProtoCx<'_, Self>) {
        if self.decision.is_some() {
            return;
        }
        self.decision = Some(v.clone());
        self.store(cx);
        cx.indicate(Ind::Decide(v));
    }

    fn through_lec(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut LoggedEpochChange, &mut ProtoCx<'_, LoggedEpochChange>),
    ) {
        let mut inds = self.lec.run_durable(cx, Wire::Change, lec_slot(), f);
        for lec::Ind::StartEpoch { ts, leader } in inds.drain(..) {
            self.newts = ts;
            self.newleader = leader;
            self.maybe_abort(cx);
        }
        self.lec.reclaim(inds);
    }

    fn through_lep(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut LoggedEpochConsensus<V>, &mut ProtoCx<'_, LoggedEpochConsensus<V>>),
    ) {
        let mut inds = self.lep.run_durable(cx, Wire::Consensus, lep_slot(), f);
        for ind in inds.drain(..) {
            match ind {
                lep::Ind::Decide(v) => self.on_epoch_decision(v, cx),
                lep::Ind::Aborted(state) => self.on_aborted(state, cx),
            }
        }
        self.lep.reclaim(inds);
    }
}

impl<V: Clone + PartialEq> Protocol for LoggedLeaderDrivenConsensus<V> {
    type Cmd = Cmd<V>;
    type Ind = Ind<V>;
    type Msg = Wire<V>;
    type Scope = core::convert::Infallible;
    type Meta = Durable<V>;
    /// Nothing accumulates: one epoch, one leader, one decision, and one record per child.
    type Entry = core::convert::Infallible;

    /// `upon event ⟨ luc, Propose | v ⟩ do val := v`.
    fn on_cmd(&mut self, Cmd::Propose(v): Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        self.val = Some(v);
        self.maybe_propose(cx);
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<V>, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Change(m) => self.through_lec(cx, |lec, ccx| lec.on_msg(from, m, ccx)),
            // The instance guard is inside the child, which drops anything not stamped with its
            // own epoch. See `lep::Tagged`.
            Wire::Consensus(m) => self.through_lep(cx, |lep, ccx| lep.on_msg(from, m, ccx)),
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_lec(cx, |lec, ccx| lec.on_timer(id, ccx));
        self.through_lep(cx, |lep, ccx| lep.on_timer(id, ccx));
    }

    /// `upon event ⟨ luc, Init ⟩ … store(ets, ℓ, decision)`.
    ///
    /// This process's own record goes down first, then each child's, so the record exists before
    /// anything writes into a slot of it.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.store(cx);
        self.through_lec(cx, |lec, ccx| lec.on_init(ccx));
        self.through_lep(cx, |lep, ccx| lep.on_init(ccx));
    }

    /// `upon event ⟨ luc, Recovery ⟩`.
    ///
    /// The book reads its children's records by name — `retrieve(startts, start) of instance lec`,
    /// `retrieve(epochdecision) of instance lep.ets`. Here each child reads its own slot in its own
    /// `Recovery`, which is the same statement in the direction the composition runs.
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        // `retrieve(ets, ℓ, decision)`
        if let Some(held) = cx.storage().get().cloned() {
            self.ets = held.ets;
            self.leader = held.leader.unwrap_or(self.l0);
            self.decision = held.decision;
        }

        // `retrieve(startts, start) of instance lec; (newts, newℓ) := (startts, start)`
        self.through_lec(cx, |lec, ccx| lec.on_recovery(ccx));
        self.newts = self.lec.last_timestamp();
        self.newleader = self.lec.last_leader();

        // `retrieve(epochdecision) of instance lep.ets`. The instance is rebuilt at the epoch just
        // read back, and reads its own slot.
        self.lep.replace(LoggedEpochConsensus::new(
            self.me,
            self.peers.clone(),
            self.ets,
            self.leader,
            State::default(),
            self.timing.retransmit,
        ));
        self.through_lep(cx, |lep, ccx| lep.on_recovery(ccx));

        // `if epochdecision ≠ ⊥ ∧ decision = ⊥ then decision := epochdecision; store(decision);
        //  trigger ⟨ luc, Decide | decision ⟩`
        //
        // This is what makes `UC3` hold across a restart from the *other* side: a process that
        // decided and crashed before anything above it saw the indication is told again.
        let epoch_decision = self.lep.epoch_decision().cloned();
        if let Some(v) = epoch_decision
            && self.decision.is_none()
        {
            self.decision = Some(v.clone());
            self.store(cx);
            cx.indicate(Ind::Decide(v));
        } else if self.decision.is_some() {
            // Decided before the crash, and the record says so. Re-announced for the same reason:
            // the layer above may never have seen the first indication.
            let v = self.decision.clone().expect("just checked");
            cx.indicate(Ind::Decide(v));
        }

        // `aborted := FALSE`
        self.aborted = false;
        self.proposed = false;
        self.maybe_abort(cx);
    }
}
