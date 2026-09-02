//! Logged uniform total-order broadcast.
//!
//! **Status: transcription. Space: unbounded — `unordered`, `delivered` and the family of consensus
//! instances all grow with the number of entries handled, and `delivered` and `proposals` grow in
//! stable storage as well.** That is the page. See `docs/bounded-space.md`.
//!
//! Cachin, Guerraoui & Rodrigues, Module `LoggedUniformTotalOrderBroadcast` and Algorithm 6.12,
//! p. 327, quoted from the book:
//!
//! ```text
//! Algorithm 6.12: Logged Uniform Total-Order Broadcast
//! Implements: LoggedUniformTotalOrderBroadcast, instance lutob.
//! Uses:
//!     LoggedUniformReliableBroadcast, instance lurb;
//!     LoggedUniformConsensus (multiple instances).
//!
//! upon event ⟨ lutob, Init ⟩ do
//!     unordered := ∅;
//!     delivered := [];
//!     round := 1;
//!     recovering := FALSE;
//!     wait := FALSE;
//!     forall r > 0 do proposals[r] := ⊥;
//!
//! upon event ⟨ Recovery ⟩ do
//!     unordered := ∅;
//!     delivered := [];
//!     round := 1;
//!     recovering := TRUE;
//!     wait := FALSE;
//!     retrieve(proposals);
//!     if proposals[1] ≠ ⊥ then
//!         trigger ⟨ luc.1, Propose | proposals[1] ⟩;
//!
//! upon event ⟨ lutob, Broadcast | m ⟩ do
//!     trigger ⟨ lurb, Broadcast | m ⟩;
//!
//! upon event ⟨ lurb, Deliver | lurbdelivered ⟩ do
//!     unordered := unordered ∪ lurbdelivered;
//!
//! upon unordered \ delivered ≠ ∅ ∧ wait = FALSE ∧ recovering = FALSE do
//!     wait := TRUE;
//!     Initialize a new instance luc.round of logged uniform consensus;
//!     proposals[round] := unordered \ delivered;
//!     store(proposals);
//!     trigger ⟨ luc.round, Propose | proposals[round] ⟩;
//!
//! upon event ⟨ luc.r, Decide | decided ⟩ such that r = round do
//!     forall (s, m) ∈ sort(decided) do     // by the order in the resulting sorted list
//!         append(delivered, (s, m));
//!     store(delivered);
//!     round := round + 1;
//!     if recovering = TRUE then
//!         if proposals[round] ≠ ⊥ then
//!             trigger ⟨ luc.round, Propose | proposals[round] ⟩;
//!         else
//!             recovering := FALSE;
//!     else
//!         wait := FALSE;
//!     trigger ⟨ lutob, Deliver | delivered ⟩;
//! ```
//!
//! The pair to [`crate::consensus_based_total_order_broadcast`], and held to the same suite. What
//! differs is exactly one thing: the ordered sequence survives a restart. Everything else — one
//! consensus instance per round, propose the unordered set, sort what is decided — is the same
//! shape, which is what makes the comparison worth having.
//!
//! **Why the recovery is not simply "read it back".** A process that proposed for a round and then
//! forgot would, on recovering, propose something *different* for the same round — and a uniform
//! consensus that has already decided cannot accommodate it. So `proposals[r]` is durable before the
//! proposal is visible to anyone, and recovery re-proposes what was recorded, round by round, until
//! it reaches one it never proposed for. That is what `recovering` is counting through.
//!
//! # Departures from the page
//!
//! - **A read**, as the port requires. See [`crate::total_order_log`].
//!
//! - **Consensus instances are a family, and the conditional event handler is discharged here.**
//!   Both as in the crash-stop member, and for the same reasons; that module's header states them.
//!   Unlike that member, this one runs over a consensus assuming no synchrony, so processes can
//!   genuinely drift and the family is doing work rather than standing on faithfulness alone.
//!
//! - **`delivered` and `proposals` are appended, not rewritten.** The page writes
//!   `append(delivered, (s, m)); store(delivered)` and `store(proposals)` — rewriting a whole
//!   growing structure on every change, which costs `O(n²)` bytes over a run. This repository's
//!   storage interface splits the two cases so the choice is visible in the types, and
//!   `docs/bounded-space.md` records both logged modules having had exactly this defect and losing
//!   it. So both go into the appended sequence, one entry each, and recovery replays them.
//!
//! - **Consensus instances are re-created here, not by a runtime.** The page says so outright:
//!   "During the recovery operation after a crash, the total-order algorithm runs again through all
//!   rounds executed before the crash and executes the same consensus instances once more. *(We
//!   assume that the runtime environment re-instantiates all instances of consensus that had been
//!   dynamically initialized before the crash.)*" There is no such runtime here — a crash rebuilds
//!   a process from its constructor and nothing else survives but storage — so `on_recovery`
//!   re-creates every instance the durable record names and runs each one's own recovery, all of
//!   them before any decision is acted on: a decided instance announces its decision again from its
//!   record, and an undecided one must have read its state back before recovery re-proposes into
//!   it. The same shape of departure as the conditional event handler: a facility the book assumes,
//!   discharged in the module.
//!
//! - **The decided prefix is replayed from the record, not re-decided.** The page rebuilds
//!   `delivered` by running every round again, which is what its runtime's re-instantiated
//!   instances are for. The appended `Record::Ordered` entries already hold the sequence in order,
//!   so recovery replays them directly and the walk over re-announced decisions advances `round`
//!   without appending or announcing anything twice — the same guard that makes duplicate decisions
//!   harmless in a live run makes the replay idempotent. Each replayed entry *is* announced again,
//!   once, for the reason [`crate::logged_leader_driven_consensus`] gives for re-announcing a
//!   decision: the layer above may have crashed with this process and never seen the first
//!   indication. Positions make the re-announcement idempotent for a client.
//!
//! - **The child that appends is composed through a sequence slot.** `logged_uniform_reliable_
//!   broadcast` keeps an appended record of its own, and until this module nothing composed over a
//!   child that appends — `store.rs` said so, and said what the missing half would be. This is its
//!   second consumer, and [`recon_core::SeqSlot`] is what that paragraph described. Parent and child
//!   append into **one** sequence, so the order between their entries is real rather than invented
//!   at recovery.

use recon_core::{Child, KeyedSlot, NodeId, Position, ProtoCx, Protocol, SeqSlot, Slot, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::Timing;
use crate::consensus_based_total_order_broadcast::{Batch, Slot as OrderedSlot};
use crate::logged_leader_driven_consensus::{self as luc, LoggedLeaderDrivenConsensus};
use crate::logged_uniform_reliable_broadcast::{self as lurb, LoggedUniformReliableBroadcast};
use crate::total_order_log::{LogInd, TotalOrderLog};

/// The consensus one round runs. Not a type parameter, for the reason the crash-stop member gives.
pub type Consensus<V> = LoggedLeaderDrivenConsensus<Batch<V>>;

/// This layer's messages: the broadcast's, and a consensus instance's stamped with its round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire<B, C> {
    Broadcast(B),
    /// Stamped with the round whose instance it belongs to — the round is this layer's concept, not
    /// the consensus's, so this layer stamps.
    Consensus {
        round: u64,
        msg: C,
    },
}

/// What this protocol appends. One sequence, carrying its own entries and its child's.
///
/// The page's `store(delivered)` and `store(proposals)` rewrite whole growing structures; these are
/// appended instead, which is the departure the module header records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record<V: Clone + Ord> {
    /// One entry taking its place in the agreed sequence.
    Ordered(OrderedSlot<V>),
    /// What this process proposed for a round, durable before the proposal was visible.
    Proposed { round: u64, batch: Batch<V> },
    /// The reliable broadcast's own record, in this sequence rather than a second one.
    Broadcast(lurb::Record<V>),
}

/// This protocol's rewritten metadata, and its children's inside it.
///
/// The consensus half is a **family**: one record per round, since one instance per round keeps its
/// own. That is what [`recon_core::KeyedSlot`] is for — a place named as a function of a key, with
/// the key supplied as data so the slot is still one fixed function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Durable<V: Clone + Ord> {
    /// The broadcast's record. `()` — it writes once so a restart finds something.
    broadcast: Option<()>,
    /// Each round's consensus instance's record.
    rounds: BTreeMap<u64, luc::Durable<Batch<V>>>,
}

impl<V: Clone + Ord> Default for Durable<V> {
    fn default() -> Self {
        Durable { broadcast: None, rounds: BTreeMap::new() }
    }
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<V> {
    /// `⟨ lutob, Broadcast | m ⟩`, in the port's terms.
    Append(V),
    Read {
        from: Position,
    },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<V> {
    /// One entry of `⟨ lutob, Deliver | delivered ⟩`, with the position it took.
    ///
    /// The page hands up the whole list each round; the port's vocabulary is one entry at a time,
    /// and the list is [`LoggedUniformTotalOrderBroadcast::entries`].
    Ordered {
        position: Position,
        from: NodeId,
        value: V,
    },
    Contents {
        from: Position,
        entries: Vec<V>,
    },
}

/// A totally ordered log whose sequence survives a restart.
pub struct LoggedUniformTotalOrderBroadcast<V: Clone + Ord> {
    me: NodeId,
    peers: BTreeSet<NodeId>,
    timing: Timing,
    /// `unordered`.
    unordered: Batch<V>,
    /// `delivered`, as the agreed sequence.
    delivered: Vec<OrderedSlot<V>>,
    ordered: BTreeSet<OrderedSlot<V>>,
    /// `proposals[r]`, durable.
    proposals: BTreeMap<u64, Batch<V>>,
    /// `round`.
    round: u64,
    /// `wait`.
    wait: bool,
    /// `recovering`.
    recovering: bool,
    lurb: Child<LoggedUniformReliableBroadcast<V>>,
    consensus: BTreeMap<u64, Child<Consensus<V>>>,
    decisions: BTreeMap<u64, Batch<V>>,
}

fn broadcast_slot<V: Clone + Ord>() -> Slot<Durable<V>, ()> {
    Slot {
        read: |d| d.broadcast.as_ref(),
        write: |d, c| {
            let mut whole = d.cloned().unwrap_or_default();
            whole.broadcast = Some(c);
            whole
        },
    }
}

/// One round's consensus record, inside this protocol's. The round is the key.
fn round_slot<V: Clone + Ord>() -> KeyedSlot<Durable<V>, luc::Durable<Batch<V>>, u64> {
    KeyedSlot {
        read: |d, r| d.rounds.get(r),
        write: |d, r, c| {
            let mut whole = d.cloned().unwrap_or_default();
            whole.rounds.insert(*r, c);
            whole
        },
    }
}

fn broadcast_entries<V: Clone + Ord>() -> SeqSlot<Record<V>, lurb::Record<V>> {
    SeqSlot {
        wrap: Record::Broadcast,
        project: |r| match r {
            Record::Broadcast(inner) => Some(inner),
            _ => None,
        },
    }
}

impl<V: Clone + Ord> LoggedUniformTotalOrderBroadcast<V> {
    /// A totally ordered log among `peers` whose sequence survives a restart.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, timing: Timing) -> Self {
        let mut peers: BTreeSet<NodeId> = peers.into_iter().collect();
        peers.insert(me);
        LoggedUniformTotalOrderBroadcast {
            me,
            peers: peers.clone(),
            timing,
            unordered: BTreeSet::new(),
            delivered: Vec::new(),
            ordered: BTreeSet::new(),
            proposals: BTreeMap::new(),
            round: 1,
            wait: false,
            recovering: false,
            lurb: Child::new(LoggedUniformReliableBroadcast::new(me, peers, timing.retransmit)),
            consensus: BTreeMap::new(),
            decisions: BTreeMap::new(),
        }
    }

    /// The agreed sequence as this process holds it.
    pub fn entries(&self) -> &[OrderedSlot<V>] {
        &self.delivered
    }

    pub fn len(&self) -> usize {
        self.delivered.len()
    }

    pub fn is_empty(&self) -> bool {
        self.delivered.is_empty()
    }

    pub fn round(&self) -> u64 {
        self.round
    }

    pub fn instances(&self) -> usize {
        self.consensus.len()
    }

    /// `upon unordered \ delivered ≠ ∅ ∧ wait = FALSE ∧ recovering = FALSE do`.
    fn maybe_propose(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if self.wait || self.recovering {
            return;
        }
        let batch: Batch<V> =
            self.unordered.difference(&self.ordered).cloned().collect::<BTreeSet<_>>();
        if batch.is_empty() {
            return;
        }
        self.wait = true;
        let round = self.round;
        self.proposals.insert(round, batch.clone());
        // `store(proposals)` — durable **before** the proposal is visible to anyone. A process that
        // proposed and then forgot would propose something different for the same round on
        // recovering, and a decided uniform consensus cannot accommodate that.
        cx.storage().append(Record::Proposed { round, batch: batch.clone() });
        self.through_consensus(round, cx, |c, ccx| c.on_cmd(luc::Cmd::Propose(batch), ccx));
    }

    /// The decide handler, with the buffering the book's run-time system would have done.
    fn drain_decisions(&mut self, cx: &mut ProtoCx<'_, Self>) {
        while let Some(decided) = self.decisions.remove(&self.round) {
            for (from, value) in &decided {
                if self.ordered.contains(&(*from, value.clone())) {
                    continue;
                }
                let position = Position(self.delivered.len() as u64);
                self.delivered.push((*from, value.clone()));
                self.ordered.insert((*from, value.clone()));
                // `append(delivered, (s, m)); store(delivered)` — appended rather than rewritten.
                cx.storage().append(Record::Ordered((*from, value.clone())));
                cx.indicate(Ind::Ordered { position, from: *from, value: value.clone() });
            }
            self.round += 1;
            if self.recovering {
                // Re-propose what was recorded for the next round, or stop recovering when this
                // process never got that far.
                match self.proposals.get(&self.round).cloned() {
                    Some(batch) => {
                        let round = self.round;
                        self.through_consensus(round, cx, |c, ccx| {
                            c.on_cmd(luc::Cmd::Propose(batch), ccx)
                        });
                    }
                    None => {
                        self.recovering = false;
                        self.wait = false;
                    }
                }
            } else {
                self.wait = false;
            }
        }
        self.maybe_propose(cx);
    }

    fn through_lurb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(
            &mut LoggedUniformReliableBroadcast<V>,
            &mut ProtoCx<'_, LoggedUniformReliableBroadcast<V>>,
        ),
    ) {
        let mut inds = self.lurb.run_appending(
            cx,
            Wire::Broadcast,
            broadcast_slot::<V>(),
            broadcast_entries::<V>(),
            f,
        );
        for lurb::Ind::Delivered(log) in inds.drain(..) {
            // `upon event ⟨ lurb, Deliver | lurbdelivered ⟩ do unordered := unordered ∪ lurbdelivered`
            for (id, msg) in log.delivered() {
                self.unordered.insert((id.origin, msg.clone()));
            }
        }
        self.lurb.reclaim(inds);
        self.maybe_propose(cx);
    }

    fn through_consensus(
        &mut self,
        r: u64,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut Consensus<V>, &mut ProtoCx<'_, Consensus<V>>),
    ) {
        // "Initialize a new instance luc.round" — an event, run before whatever provoked the
        // creation; the crash-stop member's departures say what skipping it cost. An instance
        // re-created by `on_recovery` takes the recovery branch there instead, and this path then
        // finds it present.
        let created = !self.consensus.contains_key(&r);
        self.consensus.entry(r).or_insert_with(|| {
            Child::new(LoggedLeaderDrivenConsensus::new(self.me, self.peers.clone(), self.timing))
        });
        let child = self.consensus.get_mut(&r).expect("just inserted");
        let mut inds = child.run_keyed(
            cx,
            move |m| Wire::Consensus { round: r, msg: m },
            round_slot::<V>(),
            r,
            |c, ccx| {
                if created {
                    c.on_init(ccx);
                }
                f(c, ccx)
            },
        );
        for luc::Ind::Decide(decided) in inds.drain(..) {
            self.decisions.entry(r).or_insert(decided);
        }
        if let Some(child) = self.consensus.get_mut(&r) {
            child.reclaim(inds);
        }
        self.drain_decisions(cx);
    }
}

impl<V: Clone + Ord> Protocol for LoggedUniformTotalOrderBroadcast<V> {
    type Cmd = Cmd<V>;
    type Ind = Ind<V>;
    type Msg = Wire<<LoggedUniformReliableBroadcast<V> as Protocol>::Msg, luc::Wire<Batch<V>>>;
    type Scope = core::convert::Infallible;
    type Note = crate::Note;
    type Meta = Durable<V>;
    type Entry = Record<V>;

    fn on_cmd(&mut self, cmd: Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            Cmd::Append(v) => {
                self.through_lurb(cx, |b, ccx| b.on_cmd(lurb::Cmd::Broadcast(v), ccx));
            }
            Cmd::Read { from } => {
                let entries: Vec<V> =
                    self.delivered.iter().skip(from.0 as usize).map(|(_, v)| v.clone()).collect();
                cx.indicate(Ind::Contents { from, entries });
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>) {
        match msg {
            Wire::Broadcast(m) => self.through_lurb(cx, |b, ccx| b.on_msg(from, m, ccx)),
            Wire::Consensus { round, msg } => {
                self.through_consensus(round, cx, |c, ccx| c.on_msg(from, msg, ccx));
            }
        }
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_lurb(cx, |b, ccx| b.on_timer(id, ccx));
        let rounds: Vec<u64> = self.consensus.keys().copied().collect();
        for r in rounds {
            self.through_consensus(r, cx, |c, ccx| c.on_timer(id, ccx));
        }
    }

    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.through_lurb(cx, |b, ccx| b.on_init(ccx));
    }

    /// `upon event ⟨ Recovery ⟩`, with the two facilities the page assumes discharged here: the
    /// runtime that re-instantiates consensus instances, and the buffering behind `such that`.
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.recovering = true;

        // `retrieve(proposals)` — and the ordered entries, which the page rebuilds by re-running
        // every round and this module replays from its appended record instead; the departures say
        // why, and why each replayed entry is announced again.
        let records: Vec<Record<V>> =
            cx.storage().read_from(Position::START).into_iter().cloned().collect();
        for record in records {
            match record {
                Record::Ordered((from, value)) => {
                    if self.ordered.contains(&(from, value.clone())) {
                        continue;
                    }
                    let position = Position(self.delivered.len() as u64);
                    self.delivered.push((from, value.clone()));
                    self.ordered.insert((from, value.clone()));
                    cx.indicate(Ind::Ordered { position, from, value });
                }
                Record::Proposed { round, batch } => {
                    self.proposals.insert(round, batch);
                }
                // The child's, replayed by the child below through its own filtered view.
                Record::Broadcast(_) => {}
            }
        }

        // The broadcast re-announces its log — which rebuilds `unordered` — and re-sends what was
        // still pending. `recovering` holds the proposal this would otherwise trigger.
        self.through_lurb(cx, |b, ccx| b.on_recovery(ccx));

        // Re-instantiate every consensus instance the durable record names, and recover all of
        // them before acting on any decision. Not `through_consensus`: that drains after each
        // instance, and the walk's re-proposal must never reach an instance that has not read its
        // record back — a process that proposed and then forgot is the exact failure the durable
        // proposal exists to prevent.
        let rounds: Vec<u64> =
            cx.storage().get().map(|d| d.rounds.keys().copied().collect()).unwrap_or_default();
        for r in rounds {
            self.consensus.entry(r).or_insert_with(|| {
                Child::new(LoggedLeaderDrivenConsensus::new(
                    self.me,
                    self.peers.clone(),
                    self.timing,
                ))
            });
            let child = self.consensus.get_mut(&r).expect("just inserted");
            let mut inds = child.run_keyed(
                cx,
                move |m| Wire::Consensus { round: r, msg: m },
                round_slot::<V>(),
                r,
                |c, ccx| c.on_recovery(ccx),
            );
            for luc::Ind::Decide(decided) in inds.drain(..) {
                self.decisions.entry(r).or_insert(decided);
            }
            if let Some(child) = self.consensus.get_mut(&r) {
                child.reclaim(inds);
            }
        }

        // Walk the re-announced decisions forward. Replayed entries are already in `ordered`, so
        // the walk advances `round` without appending or announcing anything twice, and its
        // recovering branch re-proposes for a round that was proposed and never decided.
        let before = self.round;
        self.drain_decisions(cx);

        // The page's own Recovery handler: `if proposals[1] ≠ ⊥ then trigger ⟨ luc.1, Propose ⟩`.
        // Needed only when the walk did not run — no round had decided — since the walk's
        // recovering branch otherwise made this same choice at the round it stopped at. No recorded
        // proposal means the crash landed before this process proposed anything still undecided,
        // and recovery is over.
        if self.recovering && self.round == before {
            match self.proposals.get(&self.round).cloned() {
                Some(batch) => {
                    let round = self.round;
                    self.through_consensus(round, cx, |c, ccx| {
                        c.on_cmd(luc::Cmd::Propose(batch), ccx)
                    });
                }
                None => {
                    self.recovering = false;
                    self.wait = false;
                    self.maybe_propose(cx);
                }
            }
        }
    }
}

impl<V: Clone + Ord> TotalOrderLog<V> for LoggedUniformTotalOrderBroadcast<V> {
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
        }
    }
}
