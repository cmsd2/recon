//! The Multi-Paxos replica: slots become positions in a log.
//!
//! **Status: implementation. Space: the bookkeeping is bounded — `decisions` and `performed` by a
//! retention window, `requests` and `proposals` by `WINDOW` — and the ordered sequence is
//! deliberately not, because it is the data rather than the bookkeeping. On disk: one appended
//! record per applied entry, and one rewritten record holding the incarnation and the consensus
//! core's own; the sequence grows with the run, and a snapshot is what would bound it.**
//!
//! §4.2 is applied, and it is worth being clear which half. The section separates state that is
//! *unnecessary* — the leader's and acceptor's, collected below a watermark once `f + 1` replicas
//! have applied past it, which is [`crate::multi_paxos_synod`]'s — from state that is
//! **unavoidable**, which is this module's:
//!
//! > because commands may be decided in multiple slots, replicas each maintain a set of all
//! > decisions to filter out such duplicates. In practice, it is often sufficient if such
//! > information is only kept for a certain amount of time, making the probability of duplicate
//! > execution negligible.
//!
//! So the filter is bounded by a **retention window** rather than by the watermark beneath it, and
//! the reason is worth stating because using the watermark is the obvious wrong move: that
//! watermark says `f + 1` replicas have *applied* up to a slot, which says nothing whatever about
//! whether a command decided below it may be decided again above it. A duplicate filter has to
//! outlive every slot a duplicate could span, and no watermark bounds that.
//!
//! **What that weakens, stated rather than discovered.** "A command decided in two slots takes one
//! position" holds *within the retention window*. A duplicate arriving more than [`RETAIN`] slots
//! after the first would take a second position. As things stand that is a weakening of something
//! unreachable — a command is minted at one replica and occupies one slot at a time, so a duplicate
//! needs the client-retry deployment this port does not have — but it is the honest bound and it is
//! in the specification.
//!
//! **The ordered sequence is exempt and is not collected.** A log grows with what is appended to
//! it; that is the data, and a log that discarded its entries would not be one. What would bound it
//! is a snapshot, which is outside the paper and is also the point past which a lagging replica
//! can no longer be caught up — see the catch-up below.
//!
//! # Source
//!
//! van Renesse, R. and Altinbuken, D. (2015) 'Paxos Made Moderately Complex', *ACM Computing
//! Surveys*, 47(3), pp. 1–36 — §2.1 and **Figure 1**, "Pseudocode for a replica", page 42:6.
//!
//! **Not** van Renesse, R. (2011), the Cornell technical report of the same title. The two differ
//! here more than anywhere else in the algorithm: the report's replica keeps a single `slot num`
//! and its `propose(p)` searches for the lowest slot not already proposed or decided, with no
//! window at all. The survey splits that into `slot_in` and `slot_out` and adds `WINDOW`, which is
//! the version transcribed below. A reader checking this code against a page needs to know which
//! page.
//!
//! ```text
//! process Replica(leaders, initial_state)
//!   var state := initial_state, slot_in := 1, slot_out := 1;
//!   var requests := ∅, proposals := ∅, decisions := ∅;
//!
//!   function propose()
//!     while slot_in < slot_out + WINDOW ∧ ∃c : c ∈ requests do
//!       if ∃op : ⟨slot_in − WINDOW, ⟨·, ·, op⟩⟩ ∈ decisions ∧ isreconfig(op) then
//!         leaders := op.leaders;
//!       end if
//!       if ∄c' : ⟨slot_in, c'⟩ ∈ decisions then
//!         requests := requests \ {c};
//!         proposals := proposals ∪ {⟨slot_in, c⟩};
//!         ∀λ ∈ leaders : send(λ, ⟨propose, slot_in, c⟩);
//!       end if
//!       slot_in := slot_in + 1;
//!     end while
//!   end function
//!
//!   function perform(⟨κ, cid, op⟩)
//!     if (∃s : s < slot_out ∧ ⟨s, ⟨κ, cid, op⟩⟩ ∈ decisions) ∨ isreconfig(op) then
//!       slot_out := slot_out + 1;
//!     else
//!       ⟨next, result⟩ := op(state);
//!       atomic
//!         state := next; slot_out := slot_out + 1;
//!       end atomic
//!       send(κ, ⟨response, cid, result⟩);
//!     end if
//!   end function
//!
//!   for ever
//!     switch receive()
//!       case ⟨request, c⟩ :
//!         requests := requests ∪ {c};
//!       end case
//!       case ⟨decision, s, c⟩ :
//!         decisions := decisions ∪ {⟨s, c⟩};
//!         while ∃c' : ⟨slot_out, c'⟩ ∈ decisions do
//!           if ∃c'' : ⟨slot_out, c''⟩ ∈ proposals then
//!             proposals := proposals \ {⟨slot_out, c''⟩};
//!             if c'' ≠ c' then
//!               requests := requests ∪ {c''};
//!             end if
//!           end if
//!           perform(c');
//!         end while
//!       end case
//!     end switch
//!     propose();
//!   end for
//! end process
//! ```
//!
//! # The invariants, quoted
//!
//! - **R1**: "There are no two different commands decided for the same slot:
//!   `∀s, ρ1, ρ2, c1, c2 : ⟨s, c1⟩ ∈ ρ1.decisions ∧ ⟨s, c2⟩ ∈ ρ2.decisions ⇒ c1 = c2`." Held by the
//!   child, not here: it is the Synod protocol's S1, and this layer relies on it rather than
//!   enforcing it. What this layer does is not break it — `decisions` never
//!   replaces an entry.
//! - **R2**: "All commands up to `slot out` are in the set of decisions:
//!   `∀ρ, s : 1 ≤ s < ρ.slot out ⇒ ∃c : ⟨s, c⟩ ∈ ρ.decisions`." Held because `slot_out` advances
//!   only inside `perform`, which the drain loop calls only for a slot that has a decision.
//! - **R3**: "For all replicas ρ, `ρ.state` is the result of applying the commands
//!   `⟨s, cs⟩ ∈ ρ.decisions` to `initial state` for all `s` up to `slot out`, in order of slot
//!   number." Here `state` is the ordered sequence — see the departures — so R3 is the statement
//!   that the sequence is exactly the decided commands below `slot_out`, in slot order, minus the
//!   ones `perform` skipped as already applied.
//! - **R4**: "For each ρ, the variable `ρ.slot out` cannot decrease over time." Structural: the
//!   only assignment is `+= 1`.
//! - **R5**: "A replica proposes commands only for slots for which it knows the configuration:
//!   `∀ρ : ρ.slot in < ρ.slot out + WINDOW`." The `while` guard in `propose`, kept although the
//!   reason for it is not — see below.
//!
//!   **The formula and the sentence beside it do not say quite the same thing, and the code
//!   follows the sentence.** `propose`'s loop tests `slot_in < slot_out + WINDOW` at the top and
//!   increments `slot_in` at the bottom, so an exit with requests still queued leaves
//!   `slot_in = slot_out + WINDOW` exactly — the strict inequality does not hold of the variable
//!   between the loop ending and `slot_out` next advancing. What does hold, always, is the English:
//!   every slot ever *proposed for* is below `slot_out + WINDOW` at the moment of proposing, since
//!   the guard is checked before the slot is used. The suite asserts that form over
//!   [`MultiPaxosReplica::proposed_slots`] and the loop's own `≤` over the variable, rather than
//!   asserting a formula the page's own pseudocode breaks.
//!
//! # Where each part of Figure 1 went
//!
//! | Figure 1 | Here |
//! |---|---|
//! | `var state := initial_state` | the ordered sequence; there is no application state machine, because the port is a log |
//! | `slot_in`, `slot_out` | fields, counting slots |
//! | `requests`, `proposals`, `decisions` | fields; `decisions` is append-only, as the page has it |
//! | `function propose()` | `transfer` plus the send loop in `pump` |
//! | the `isreconfig` branch in `propose()` | absent — reconfiguration is a later change; the `WINDOW` guard around it is kept |
//! | `function perform()` | `perform`, minus `op(state)` and the client response |
//! | `case ⟨request, c⟩` | [`Cmd::Append`], the port's own |
//! | `case ⟨decision, s, c⟩` | the child's [`crate::multi_paxos_synod::Ind::Decision`] |
//! | `∀λ ∈ leaders : send(λ, ⟨propose, …⟩)` | a call into the child, per §4.4 |
//! | `send(κ, ⟨response, cid, result⟩)` | absent, with `state` |
//!
//! # Departures from the page
//!
//! - **`state` is a sequence, and there is no `op(state)`.** The port this satisfies is
//!   [`crate::total_order_log::TotalOrderLog`]: a log, not a replicated state machine. `perform`
//!   therefore appends the command to the sequence where the page applies it, and the client
//!   response goes with the result it would have carried. What survives is the part R1–R4 are about
//!   — that every replica applies the same commands in the same order — and the part that goes is
//!   the application on top of it.
//!
//! - **`∀λ ∈ leaders : send(λ, ⟨propose, s, c⟩)` is a call into the child.** §4.4: "each machine
//!   that runs a replica also runs a leader… the replica can send a proposal for a particular slot
//!   to its local leader". So `leaders` is not a set here, it is one child, and the fan-out to
//!   remote leaders is the child's forwarding rather than this layer's broadcast. That is what lets
//!   this module hold no link, no broadcast and no wire of its own; the cost is in
//!   [`crate::multi_paxos_synod`]'s own documentation, and it is that proposal *delivery* now rests
//!   on the leader detector.
//!
//! - **The already-decided check is indexed rather than scanned.** The page writes
//!   `∃s : s < slot_out ∧ ⟨s, ⟨κ, cid, op⟩⟩ ∈ decisions`, a scan of every decision below
//!   `slot_out`. `performed` is exactly that set — `perform` runs once per slot in slot order, so
//!   what it has seen *is* `{decisions[s] : s < slot_out}` — and membership answers the same
//!   question in log time. It grows the same unbounded way `decisions` does, so it changes nothing
//!   about the space statement above.
//!
//! - **`requests` is a queue, where the page says "any command".** `∃c : c ∈ requests` picks an
//!   arbitrary member. FIFO is a refinement rather than a departure in the strict sense, but it is
//!   worth naming because it is what stops a command being passed over for ever while later ones
//!   are proposed — the page leaves that to whoever implements the choice.
//!
//! - **`WINDOW` is kept and its reason is deferred.** In the source the window exists because a
//!   reconfiguration decided in slot `s` takes effect at `s + WINDOW`, so a replica may not propose
//!   past the last slot whose configuration it knows. Reconfiguration is not built here and the
//!   membership is fixed for the run, so the guard is a pipeline cap and nothing more. It is kept
//!   rather than dropped so that R5 reads against the page, and this note is here so that a reader
//!   does not take the cap for the whole reason.
//!
//! - **§4.2's periodic report, and the catch-up that pays for collecting.** A replica tells the
//!   consensus beneath it how far it has applied, periodically and not as a consequence of doing
//!   work — a replica applying nothing is the one whose position others most need, and a report
//!   riding its own traffic would fall silent exactly then.
//!
//!   Collecting at `f + 1` means `f` correct replicas may be behind, and what would have helped
//!   them is what was collected. §4.2's own remedy is that "replicas can learn decisions […] from
//!   one another", so a replica whose proposal is refused as collected asks a peer, and the peer
//!   answers from *its* `decisions` — the one place that still holds them. A replica further behind
//!   than [`RETAIN`] cannot be caught up, because nobody holds those decisions any more.
//!
//! - **The fourth liveness violation, and its fix.** Liu, Y.A., Chand, S. and Stoller, S.D. (2019)
//!   'Moderately Complex Paxos Made Simple', PPDP '19, is the cross-check this module's source is
//!   read against. Its fourth liveness violation is this layer's: if no decision arrives for a
//!   slot, `slot_out` stops moving, `WINDOW` fills, `slot_in` stops advancing and the replica
//!   wedges — with nothing on the page to get it out, because Figure 1 acts only on messages that
//!   arrive. The fix has two halves, and the other one is the leader's: a proposal outstanding
//!   longer than `repropose_after` is proposed **again for the same slot**,
//!   and a leader that has seen the slot decided answers with the decision rather than dropping
//!   the repeat. Re-proposing into a *new* slot instead would leave the old one unfilled for ever,
//!   which is the wedge itself.
//!
//!   The replica does not try to tell why a decision has not come. The proposal may have been
//!   forwarded to a process that has crashed, the decision may have been lost at a session ending,
//!   or consensus for the slot may simply still be running. Every case is answered by the same
//!   message and none is made unsafe by asking: where the slot is open the leader's `∄c'` guard
//!   drops the repeat, where the proposal was lost the repeat is the first the leader hears of it,
//!   and where the slot is decided the leader answers. So the threshold can be generous and wrong
//!   without being unsafe. The one thing it must not be is absent.
//!
//! # A position is not a slot
//!
//! [`Position`] and [`Slot`] are both `u64` and are **not** interchangeable. Slots count from 1;
//! `Position::START` is 0. More importantly `perform` skips a command already applied at a lower
//! slot, so a slot can pass without a position being taken — the two diverge by exactly the number
//! of commands decided more than once. `Position(slot)` is right until the first duplicate and then
//! silently wrong, which is why `a_command_decided_in_two_slots_takes_one_position` drives that case
//! deliberately rather than waiting for a run to produce it.
//!
//! # Scope
//!
//! This layer bridges nothing. A session ending reaches it from the child and is propagated in its
//! own indications: its redundancy is the child's, and the child's is the other processes rather
//! than anything that outlives a session. See `docs/conditional-guarantees.md`.
//!
//! # §4.3: the sequence survives a restart
//!
//! A process that returns with every write its storage acknowledged resumes the sequence it had
//! served: each applied entry is appended to the durable sequence **before** `Ordered` is
//! indicated, so a read after a restart extends what was served before it and never shortens it.
//! The consensus core's record is a named part of this one — a [`Slot`] for its rewritten half and a
//! [`SeqSlot`] for its appended accepts — so there is one record and one sequence beneath both, a
//! crash cannot land between the parent's write and the child's, and the order between an applied
//! entry and an accept is the store's rather than one reconstructed at recovery.
//!
//! Recovery derives `slot_out` from the last applied entry, rebuilds the duplicate filter and
//! `decisions` from the retained tail, writes a new **incarnation**, and — once the core beneath
//! reports that every member has answered its announcement — asks a peer for what was decided
//! meanwhile, through the catch-up §4.2 already needed. A slot the replica *skipped* as a duplicate
//! after its last applied entry leaves no record, so `slot_out` may come back low by that many;
//! the decisions for those slots are fetched again and skipped again, within the retention window,
//! which is stated rather than fixed because a record per skip would be a write per duplicate.
//!
//! **The request identifier is scoped by the incarnation.** `cid` is `(incarnation, seq)`, where
//! the incarnation is written once at each recovery and `seq` is a volatile counter. The append
//! path carries no write for the identifier, and a recovered replica cannot mint one a peer's
//! duplicate filter has seen — which would drop its genuinely new append as a repeat. This is the
//! audit's recurring bug, a durable filter keyed by a volatile counter, closed here.
//!
//! What this layer cannot bridge, it propagates: a storage scope ending raised by the core beneath
//! — a member witnessed a write this process no longer has — is raised again as
//! [`Ind::StorageScopeEnded`], and this replica orders nothing further. A gap larger than any peer
//! retains is the snapshot case, outside the paper and named as a later change.

use core::time::Duration;
use recon_core::{
    Child, NodeId, Position, ProtoCx, Protocol, SeqSlot, Slot as MetaSlot, Time, TimerId, slot,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::Timing;
use crate::link::{Boundary, VolatileLink};
use crate::multi_paxos_synod::{self as synod, Accepted, MultiPaxosSynod, Slot, SynodMsg};
use crate::session_link::SessionLink;
use crate::total_order_log::{LogInd, TotalOrderLog};

/// How many decided slots a replica keeps for the duplicate filter — §4.2's retention window.
///
/// Generously more than [`WINDOW`], because the filter has to outlive every slot a duplicate could
/// span and the window only bounds how far ahead proposals may run. See
/// `MultiPaxosReplica::retain`.
pub const RETAIN: Slot = 64;

/// How far `slot_in` may run ahead of `slot_out` — the source's `WINDOW`.
///
/// Large enough that several commanders run at once, which is what makes out-of-order decisions
/// ordinary rather than incidental, and small enough that a stalled slot is visibly a stall.
pub const WINDOW: Slot = 8;

/// The source's `c = ⟨κ, cid, op⟩`: who asked, which request of theirs it is, and what it says.
///
/// Both identifying halves are load-bearing. `from` is what the port's
/// [`LogInd::Ordered`] reports, and the page carries it for the same reason — the reply goes back
/// to `κ`. `cid` is what keeps two appends of the same value from collapsing into one: `perform`
/// skips a command it has already applied, and without `cid` a client appending `7` twice would see
/// one entry.
///
/// **`cid` is scoped by a durable incarnation.** An identifier that crosses the wire outlives the
/// handler that minted it, so its generator is state with a scope; a volatile counter alone would
/// be re-minted after a restart and a genuinely new request taken for one already applied. The
/// incarnation is written once per recovery — see the module documentation — and the counter within
/// it costs no write at all.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Command<V> {
    /// `κ` — the process that appended it.
    pub from: NodeId,
    /// `cid` — which request of that process's this is, across its restarts.
    pub cid: RequestId,
    /// `op` — what the command says. A value rather than an operation; see the departures.
    pub value: V,
}

/// `cid`, as `⟨incarnation, seq⟩`: the incarnation is durable and written once per recovery, the
/// sequence number within it is volatile. Ordered lexicographically, so a later incarnation's
/// requests sort after an earlier one's, which nothing here relies on but which reads correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct RequestId {
    pub incarnation: u64,
    pub seq: u64,
}

/// This replica's rewritten record — §4.3: its incarnation, and the consensus core's record as a
/// named part of it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Durable {
    /// Written once per recovery, so that a request identifier is never minted twice.
    pub incarnation: u64,
    /// The core's slot. Written by the core, through [`synod_slot`], and carried across untouched
    /// when this layer writes its own part.
    pub synod: Option<synod::Durable>,
}

/// One record in the durable sequence: an entry this replica applied, or an accept the core
/// beneath made. **One sequence**, so the order between them is the store's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Entry<V> {
    /// `perform` applied `command` at `slot`. Appended before `Ordered` is indicated.
    Applied { slot: Slot, command: Command<V> },
    /// The core's accept record, in this layer's vocabulary.
    Synod(Accepted<Command<V>>),
}

/// Where the core's rewritten record sits inside this one.
pub fn synod_slot() -> MetaSlot<Durable, synod::Durable> {
    slot!(Durable, synod)
}

fn project_synod<V>(entry: &Entry<V>) -> Option<&Accepted<Command<V>>> {
    match entry {
        Entry::Synod(accepted) => Some(accepted),
        Entry::Applied { .. } => None,
    }
}

/// Where the core's accept records sit inside this sequence.
pub fn synod_entries<V>() -> SeqSlot<Entry<V>, Accepted<Command<V>>> {
    SeqSlot { wrap: Entry::Synod, project: project_synod }
}

/// What the Synod protocol beneath carries for this layer.
pub type Carried<V> = SynodMsg<Command<V>>;

/// The consensus this layer runs over. A type alias rather than a parameter: the replica is the
/// half of Multi-Paxos that Figure 1 describes, and the other half is what it is.
pub type Synod<V, L> = MultiPaxosSynod<Command<V>, L>;

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<V> {
    /// `case ⟨request, c⟩` — append `value` to the log.
    Append(V),
    /// Read the ordered sequence from `from` onwards. The page has no such request; see the port's
    /// own documentation for why it exists and what it does not promise.
    Read { from: Position },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<V> {
    /// A command took its place in the agreed sequence — the page's `op(state)`, in the vocabulary
    /// of a log.
    Ordered { position: Position, from: NodeId, value: V },
    /// The answer to a [`Cmd::Read`].
    Contents { from: Position, entries: Vec<V> },
    /// The scope with `peer` ended at `epoch`, as the Synod protocol beneath reported it.
    ///
    /// Propagated rather than absorbed: this layer holds no redundancy that outlives a session.
    SessionEnded { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`.
    SessionEstablished { peer: NodeId, epoch: u64 },
    /// `peer`'s storage scope ended: a member witnessed a write it no longer has. If `peer` is this
    /// process, it has stopped. Propagated from the core beneath, never absorbed.
    StorageScopeEnded { peer: NodeId },
}

/// A totally ordered log, built on Multi-Paxos: one consensus per slot, under a stable leader that
/// keeps phase one across all of them.
#[derive(Debug)]
pub struct MultiPaxosReplica<V: Clone + Ord, L: VolatileLink<Carried<V>> = SessionLink<Carried<V>>>
{
    me: NodeId,
    /// `slot_in` — the next slot this replica will propose for.
    slot_in: Slot,
    /// `slot_out` — the next slot this replica will apply. Never decreases; R4.
    slot_out: Slot,
    /// `requests` — appended and not yet given a slot. A queue rather than a set; see the
    /// departures.
    requests: VecDeque<Command<V>>,
    /// `proposals` — given a slot and not yet applied.
    proposals: BTreeMap<Slot, Command<V>>,
    /// `decisions` — **append-only**. A slot's command never changes, because R1 says there is only
    /// one, and an entry is never removed, because R2 reads `decisions` for every slot below
    /// `slot_out` and R3 for the whole sequence. §4.2's watermark is what may remove one, and it is
    /// a later change.
    decisions: BTreeMap<Slot, Command<V>>,
    /// `state`, as a log: the commands applied, in the order they were applied.
    sequence: Vec<Command<V>>,
    /// `{decisions[s] : s < slot_out}`, indexed. See the departures.
    performed: BTreeSet<Command<V>>,
    /// `cid`'s durable half, written once per recovery — see [`RequestId`].
    incarnation: u64,
    /// `cid`'s volatile half, within the incarnation.
    seq: u64,
    /// `WINDOW`.
    window: Slot,
    /// Recovered, and not yet told by the core beneath that every member has answered; or told,
    /// and still asking a peer for what was decided meanwhile.
    catching_up: bool,
    /// The core beneath reported that a member witnessed a write this process no longer has. This
    /// replica orders nothing further.
    stopped: bool,

    // ---- the fourth liveness violation ----
    /// When each outstanding proposal was last handed to the child.
    proposed_at: BTreeMap<Slot, Time>,
    /// How long a proposal may be outstanding before it is proposed again for the same slot.
    repropose_after: Duration,
    /// How often the outstanding set is swept.
    sweep_every: Duration,
    /// How often this replica tells the consensus beneath how far it has applied — §4.2's periodic
    /// update, which is the only thing that lets anything below collect.
    report_every: Duration,
    /// When it last did.
    last_report: Time,
    /// How many decided slots this replica keeps for the duplicate filter — §4.2's "kept for a
    /// certain amount of time, making the probability of duplicate execution negligible", measured
    /// in slots rather than in seconds.
    ///
    /// Slots rather than time because a duplicate's risk is a function of how far apart two slots
    /// deciding one command can be, not of how long the run has lasted: a fast run and a slow one
    /// with the same slots in flight need the same window, and only the slot count says that.
    retain: Slot,
    /// The sweep's handle. Compared before acting, because an expiry is offered to every layer.
    tick: Option<TimerId>,

    synod: Child<Synod<V, L>>,
}

impl<V: Clone + Ord> MultiPaxosReplica<V> {
    /// A replica among `peers`, over the session link the Synod protocol defaults to.
    ///
    /// The re-proposal threshold is derived rather than passed: it must exceed the time a decision
    /// legitimately takes, whose worst case is a leadership change — `detect_after` for Ω to move,
    /// then phase one, then phase two. `detect_after * 3` clears that with room, and it is far below
    /// what filling a `WINDOW` of eight slots would take.
    pub fn new(me: NodeId, peers: impl IntoIterator<Item = NodeId>, timing: Timing) -> Self {
        let peers: Vec<NodeId> = peers.into_iter().collect();
        MultiPaxosReplica {
            me,
            slot_in: 1,
            slot_out: 1,
            requests: VecDeque::new(),
            proposals: BTreeMap::new(),
            decisions: BTreeMap::new(),
            sequence: Vec::new(),
            performed: BTreeSet::new(),
            incarnation: 0,
            seq: 0,
            window: WINDOW,
            catching_up: false,
            stopped: false,
            proposed_at: BTreeMap::new(),
            repropose_after: timing.detect_after * 3,
            sweep_every: timing.retransmit,
            report_every: timing.heartbeat,
            last_report: Time::ZERO,
            retain: RETAIN,
            tick: None,
            synod: Child::new(MultiPaxosSynod::new(me, peers, timing)),
        }
    }

    /// The window, for a test that needs to fill one without driving a hundred slots.
    pub fn with_window(mut self, window: Slot) -> Self {
        self.window = window;
        self
    }

    /// The re-proposal threshold, for a test that needs to reach it inside a settle window.
    pub fn with_repropose_after(mut self, after: Duration) -> Self {
        self.repropose_after = after;
        self
    }

    /// The retention window, for a test that needs to fill one without deciding sixty-four slots.
    pub fn with_retain(mut self, retain: Slot) -> Self {
        self.retain = retain;
        self
    }
}

impl<V: Clone + Ord, L: VolatileLink<Carried<V>>> MultiPaxosReplica<V, L> {
    /// The ordered sequence as this process holds it.
    pub fn entries(&self) -> impl Iterator<Item = &V> {
        self.sequence.iter().map(|c| &c.value)
    }

    /// How many commands this process has applied — the next [`Position`], as a count.
    pub fn len(&self) -> usize {
        self.sequence.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sequence.is_empty()
    }

    /// `slot_out`: the next slot to apply. Diverges from [`MultiPaxosReplica::len`] by exactly the
    /// number of commands decided in more than one slot.
    pub fn slot_out(&self) -> Slot {
        self.slot_out
    }

    /// `slot_in`: the next slot to propose for. R5 keeps it below `slot_out + WINDOW`.
    pub fn slot_in(&self) -> Slot {
        self.slot_in
    }

    /// How many appends are waiting for a slot.
    pub fn waiting(&self) -> usize {
        self.requests.len()
    }

    /// How many slots this replica has proposed for and not yet applied.
    pub fn outstanding(&self) -> usize {
        self.proposals.len()
    }

    /// The slots this replica has proposed for and not yet applied. R5's substantive form is
    /// asserted over these — see the module's note on where the page's `<` reads as `≤`.
    pub fn proposed_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.proposals.keys().copied()
    }

    /// How many decisions this replica holds. Grows with commands handled, and is never collected —
    /// the measurement `docs/bounded-space.md` wants of a transcription.
    pub fn decisions_held(&self) -> usize {
        self.decisions.len()
    }

    /// The command decided for `slot`, if this replica has heard.
    pub fn decision(&self, slot: Slot) -> Option<&Command<V>> {
        self.decisions.get(&slot)
    }

    /// How far this replica has told the consensus beneath it that it has applied.
    pub fn reported_slot_out(&self) -> Slot {
        self.slot_out
    }

    /// The Synod protocol beneath, for a test that asks who leads.
    pub fn synod(&self) -> &Synod<V, L> {
        &self.synod
    }

    /// The incarnation this replica's request identifiers carry. Zero until a first recovery.
    pub fn incarnation(&self) -> u64 {
        self.incarnation
    }

    /// Whether this replica has stopped: the core beneath reported its storage scope ended.
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// `function propose()`, minus the sending: move what it can from `requests` into `proposals`,
    /// and hand back what the caller must give the child.
    ///
    /// Split in two because the send in Figure 1 goes to a set of leaders and here it goes into a
    /// child, which needs `&mut self` for the duration — see `pump`.
    ///
    /// ```text
    /// while slot_in < slot_out + WINDOW ∧ ∃c : c ∈ requests do
    ///   if ∄c' : ⟨slot_in, c'⟩ ∈ decisions then
    ///     requests := requests \ {c};
    ///     proposals := proposals ∪ {⟨slot_in, c⟩};
    ///     ∀λ ∈ leaders : send(λ, ⟨propose, slot_in, c⟩);
    ///   end if
    ///   slot_in := slot_in + 1;
    /// end while
    /// ```
    ///
    /// Note where `slot_in` advances: **outside** the `∄c'` arm, so a slot already decided is
    /// stepped over without spending a request. And note the `while` guard is R5.
    fn transfer(&mut self, now: Time, cx: &mut ProtoCx<'_, Self>) -> Vec<(Slot, Command<V>)> {
        let mut out = Vec::new();
        while !self.stopped && !self.requests.is_empty() {
            if self.slot_in >= self.slot_out + self.window {
                // R5 refusing to go further. Nothing at all reaches the trace from this: a replica
                // holding requests it may not propose for looks exactly like an idle one.
                cx.note(crate::Note::WindowFull { slot_in: self.slot_in, slot_out: self.slot_out });
                break;
            }
            if !self.decisions.contains_key(&self.slot_in) {
                let command = self.requests.pop_front().expect("the loop guard");
                self.proposals.insert(self.slot_in, command.clone());
                self.proposed_at.insert(self.slot_in, now);
                out.push((self.slot_in, command));
            }
            self.slot_in += 1;
        }
        out
    }

    /// `function perform(⟨κ, cid, op⟩)`, minus `op(state)` and the client response.
    ///
    /// ```text
    /// if (∃s : s < slot_out ∧ ⟨s, ⟨κ, cid, op⟩⟩ ∈ decisions) ∨ isreconfig(op) then
    ///   slot_out := slot_out + 1;
    /// else
    ///   ⟨next, result⟩ := op(state);
    ///   atomic
    ///     state := next; slot_out := slot_out + 1;
    ///   end atomic
    ///   send(κ, ⟨response, cid, result⟩);
    /// end if
    /// ```
    ///
    /// The already-decided arm is what makes a position not a slot: the slot passes, the sequence
    /// does not grow. `isreconfig` is absent with reconfiguration.
    fn perform(&mut self, command: Command<V>, cx: &mut ProtoCx<'_, Self>) {
        if self.performed.contains(&command) {
            self.slot_out += 1;
            return;
        }
        let position = Position(self.sequence.len() as u64);
        let Command { from, value, .. } = command.clone();
        // §4.3: the entry is durable before the indication that reveals it. In the handler's own
        // text, and one appended record — the sequence is the data, and a rewrite of it per entry
        // would be the `O(n²)` the store's own documentation warns of.
        cx.storage().append(Entry::Applied { slot: self.slot_out, command: command.clone() });
        self.sequence.push(command.clone());
        self.performed.insert(command);
        // The page updates `state` and `slot_out` atomically and only then answers. Here the
        // indication is what reveals the entry, so the sequence and `slot_out` both move first.
        self.slot_out += 1;
        cx.indicate(Ind::Ordered { position, from, value });
    }

    /// `case ⟨decision, s, c⟩`, in full.
    ///
    /// ```text
    /// decisions := decisions ∪ {⟨s, c⟩};
    /// while ∃c' : ⟨slot_out, c'⟩ ∈ decisions do
    ///   if ∃c'' : ⟨slot_out, c''⟩ ∈ proposals then
    ///     proposals := proposals \ {⟨slot_out, c''⟩};
    ///     if c'' ≠ c' then
    ///       requests := requests ∪ {c''};
    ///     end if
    ///   end if
    ///   perform(c');
    /// end while
    /// ```
    ///
    /// The `while` is why a decision arriving out of order is held rather than dropped: slot 4
    /// deciding before slot 3 leaves `slot_out` at 3, and slot 3's decision then drains both.
    fn decided(&mut self, slot: Slot, command: Command<V>, cx: &mut ProtoCx<'_, Self>) {
        if self.stopped {
            return;
        }
        // `decisions := decisions ∪ {⟨s, c⟩}` — a union, so a repeat writes nothing. The child
        // announces a decision again whenever a later ballot re-commands the slot, and answers a
        // re-proposal for a decided one deliberately; R1 makes both the same command.
        self.decisions.entry(slot).or_insert(command);
        self.proposed_at.remove(&slot);
        while let Some(decided) = self.decisions.get(&self.slot_out).cloned() {
            if let Some(mine) = self.proposals.remove(&self.slot_out) {
                self.proposed_at.remove(&self.slot_out);
                if mine != decided {
                    // Somebody else's command took the slot. Ours goes back and is proposed again
                    // at a later one — nothing at all reaches the trace from this decision, and a
                    // replica that dropped it instead would lose an append silently.
                    cx.note(crate::Note::ProposalDisplaced { slot: self.slot_out });
                    self.requests.push_back(mine);
                }
            }
            self.perform(decided, cx);
        }
        self.forget_below();
    }

    /// Liu et al.'s fourth violation, swept: anything outstanding past the threshold is proposed
    /// again **for the same slot**.
    ///
    /// Undecided slots only. A slot whose decision has arrived is not stalled even if `slot_out`
    /// has not reached it, because the drain loop will pass over it as soon as the slots below
    /// decide.
    fn resweep(&mut self, cx: &mut ProtoCx<'_, Self>) -> Vec<(Slot, Command<V>)> {
        let now = cx.now();
        let due: Vec<Slot> = self
            .proposed_at
            .iter()
            .filter(|(slot, at)| {
                **at + self.repropose_after <= now && !self.decisions.contains_key(slot)
            })
            .map(|(slot, _)| *slot)
            .collect();
        let mut out = Vec::new();
        for slot in due {
            let Some(command) = self.proposals.get(&slot).cloned() else { continue };
            self.proposed_at.insert(slot, now);
            // The child's `Propose` is a call when this process leads, so a re-proposal can reach
            // the trace as nothing whatever. This is what says it happened.
            cx.note(crate::Note::SlotReproposed { slot });
            out.push((slot, command));
        }
        out
    }

    fn arm(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.tick = Some(cx.set_timer(self.sweep_every));
    }

    /// §4.2's periodic update: tell the consensus beneath how far this replica has applied, so that
    /// leaders and acceptors can discard what enough replicas already hold.
    ///
    /// **Periodic, and not a consequence of doing work.** A replica applying nothing is exactly the
    /// one whose position the others most need — a run in which one replica is idle is a run in
    /// which the watermark is pinned by it — and a report carried on this replica's own traffic
    /// would fall silent precisely then. It rides the sweep's timer at its own coarser interval,
    /// so it costs one message per member per `report_every` and nothing per entry: the cost
    /// identity counts it as its own kind for that reason.
    fn maybe_report(&mut self, cx: &mut ProtoCx<'_, Self>) {
        let now = cx.now();
        if self.last_report + self.report_every > now && now != Time::ZERO {
            return;
        }
        self.last_report = now;
        let slot_out = self.slot_out;
        self.through_synod(cx, |s, ccx| s.on_cmd(synod::Cmd::Applied { slot_out }, ccx));
    }

    /// §4.2's retention window on the duplicate filter, which is the *other* half of that section
    /// and a different mechanism from the watermark beneath.
    ///
    /// The source calls this state unavoidable and bounds it by time rather than by a watermark:
    /// "it is often sufficient if such information is only kept for a certain amount of time,
    /// making the probability of duplicate execution negligible".
    ///
    /// **Not the collection watermark, and this is the obvious wrong move.** That watermark says
    /// `f + 1` replicas have *applied* up to a slot, which says nothing whatever about whether a
    /// command decided below it may be decided again above it. A duplicate filter has to outlive
    /// every slot a duplicate could span, and no watermark bounds that.
    ///
    /// What it costs is stated in the module: the no-duplication guarantee is scoped to the window,
    /// and a command decided again more than `retain` slots later would take a second position.
    fn forget_below(&mut self) {
        let keep_from = self.slot_out.saturating_sub(self.retain);
        if keep_from == 0 {
            return;
        }
        let dropped: Vec<Command<V>> =
            self.decisions.range(..keep_from).map(|(_, command)| command.clone()).collect();
        self.decisions.retain(|slot, _| *slot >= keep_from);
        for command in dropped {
            self.performed.remove(&command);
        }
    }

    /// Run `f` against the child, then handle everything that falls out of it — including the
    /// proposals this replica makes in response, which go back into the same child.
    ///
    /// Figure 1 calls `propose()` at the bottom of its `for ever` loop, after every message, and
    /// that is what the tail of this loop is. It iterates rather than recursing because handling a
    /// decision can produce a proposal and a proposal could in principle produce an indication;
    /// today it cannot — a decision needs a round trip — and this survives the day it can.
    fn pump(
        &mut self,
        mut pending: Vec<synod::Ind<Command<V>>>,
        cx: &mut ProtoCx<'_, Self>,
        mut extra: Vec<synod::Cmd<Command<V>>>,
    ) {
        loop {
            for ind in pending.drain(..) {
                match ind {
                    synod::Ind::Decision { slot, command } => self.decided(slot, command, cx),
                    // §4.2: the slot is decided and the consensus beneath has collected it, so it
                    // cannot answer. "Replicas can learn decisions … from one another" — ask one.
                    synod::Ind::Collected { slot } => {
                        cx.note(crate::Note::CaughtUpFrom { slot });
                        extra.push(synod::Cmd::CatchUp { from_slot: slot });
                    }
                    // The other side of it. What this replica still holds is what the asker needs,
                    // and it is the only place left that holds it.
                    // Everything this replica still holds from that slot on, in one answer.
                    //
                    // Bounded by the retention window rather than by the proposal window: a
                    // capped answer leaves the asker behind with nothing to ask again *with*,
                    // because it asks only when a proposal of its own is refused and it has no
                    // proposal for a slot somebody else filled. Measured — a `WINDOW`-sized answer
                    // left the asker one entry short for ever.
                    //
                    // **A replica more than the retention window behind cannot be caught up**, and
                    // nothing here can change that: nobody holds those decisions any more. That is
                    // where a real deployment takes a snapshot, which is outside the paper.
                    synod::Ind::CatchUpWanted { peer, from_slot } => {
                        let teach: Vec<(Slot, Command<V>)> = self
                            .decisions
                            .range(from_slot..)
                            .map(|(slot, command)| (*slot, command.clone()))
                            .collect();
                        for (slot, command) in teach {
                            extra.push(synod::Cmd::Teach { to: peer, slot, command });
                        }
                    }
                    // The child bridges no session ending and neither does this layer: its
                    // redundancy is the other processes, which a session ending does not restore.
                    synod::Ind::SessionEnded { peer, epoch } => {
                        cx.indicate(Ind::SessionEnded { peer, epoch });
                    }
                    synod::Ind::SessionEstablished { peer, epoch } => {
                        cx.indicate(Ind::SessionEstablished { peer, epoch });
                    }
                    // §4.3: every member answered, none contradicted. Now ask for what was decided
                    // while this process was down — its own sequence says how far it got, and a
                    // peer's `decisions` is the one place the rest still is.
                    synod::Ind::Recovered => {
                        self.catching_up = true;
                        extra.push(synod::Cmd::CatchUp { from_slot: self.slot_out });
                    }
                    // Nobody has reported being ahead of where this replica asked from. With no
                    // reports at all that says nothing and the next sweep asks again; with any, it
                    // says the catch-up is done.
                    synod::Ind::NobodyAhead { from_slot } => {
                        if from_slot >= self.slot_out && self.synod.reports_held() > 1 {
                            self.catching_up = false;
                        }
                    }
                    // Propagated, never absorbed. If it is this process, it orders nothing further:
                    // a sequence served under an identity whose storage lied is one nobody should
                    // read from again.
                    synod::Ind::StorageScopeEnded { peer } => {
                        if peer == self.me {
                            self.stopped = true;
                            self.requests.clear();
                            self.proposals.clear();
                            self.proposed_at.clear();
                        }
                        cx.indicate(Ind::StorageScopeEnded { peer });
                    }
                }
            }
            // `propose();`
            let now = cx.now();
            let mut outgoing: Vec<synod::Cmd<Command<V>>> = self
                .transfer(now, cx)
                .into_iter()
                .map(|(slot, command)| synod::Cmd::Propose { slot, command })
                .collect();
            outgoing.append(&mut extra);
            if outgoing.is_empty() {
                break;
            }
            for cmd in outgoing {
                let mut inds = self.synod.run_appending(
                    cx,
                    |m| m,
                    synod_slot(),
                    synod_entries(),
                    |s, ccx| s.on_cmd(cmd, ccx),
                );
                pending.append(&mut inds);
                self.synod.reclaim(inds);
            }
            if pending.is_empty() {
                break;
            }
        }
        self.synod.reclaim(pending);
    }

    /// The composition, in the transforming form: a slot decision becomes a position in a sequence,
    /// so the child's indications come back for this layer to handle rather than passing through.
    fn through_synod(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut Synod<V, L>, &mut ProtoCx<'_, Synod<V, L>>),
    ) {
        // The wrap is the identity: this layer adds no header, so its wire is the child's. The
        // core's record is a slot of this one and its accepts go into this sequence: one record,
        // one sequence, one store beneath both.
        let inds = self.synod.run_appending(cx, |m| m, synod_slot(), synod_entries(), f);
        self.pump(inds, cx, Vec::new());
    }
}

impl<V: Clone + Ord, L: VolatileLink<Carried<V>>> Protocol for MultiPaxosReplica<V, L> {
    type Cmd = Cmd<V>;
    type Ind = Ind<V>;
    /// The child's. This layer adds no per-hop state, so it adds no wire field.
    type Msg = <Synod<V, L> as Protocol>::Msg;
    /// Whatever the link beneath the child is conditional on. This layer bridges none of it.
    type Scope = <Synod<V, L> as Protocol>::Scope;
    type Note = crate::Note;
    /// The incarnation, and the core's record as a named part — §4.3.
    type Meta = Durable;
    /// The applied sequence, with the core's accepts in the same order.
    type Entry = Entry<V>;

    fn on_cmd(&mut self, cmd: Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            // `case ⟨request, c⟩ : requests := requests ∪ {c};` then `propose()`.
            Cmd::Append(value) => {
                if self.stopped {
                    // Nothing at all reaches the trace from a dropped append, so it is narrated.
                    cx.note(crate::Note::AnswerWithheld { from: self.me });
                    return;
                }
                let cid = RequestId { incarnation: self.incarnation, seq: self.seq };
                let command = Command { from: self.me, cid, value };
                self.seq += 1;
                self.requests.push_back(command);
                self.pump(Vec::new(), cx, Vec::new());
            }
            // The departure the port records. Served from this process's own sequence, so it may
            // lag an append that has completed elsewhere.
            Cmd::Read { from } => {
                let entries: Vec<V> =
                    self.sequence.iter().skip(from.0 as usize).map(|c| c.value.clone()).collect();
                cx.indicate(Ind::Contents { from, entries });
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: Self::Msg, cx: &mut ProtoCx<'_, Self>) {
        self.through_synod(cx, |s, ccx| s.on_msg(from, msg, ccx));
    }

    /// An expiry is offered to every layer, so the child is given it and this layer acts only on
    /// the handle it registered itself.
    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.through_synod(cx, |s, ccx| s.on_timer(id, ccx));
        if self.tick != Some(id) {
            return;
        }
        self.arm(cx);
        self.maybe_report(cx);
        let mut due: Vec<synod::Cmd<Command<V>>> = self
            .resweep(cx)
            .into_iter()
            .map(|(slot, command)| synod::Cmd::Propose { slot, command })
            .collect();
        if self.catching_up {
            // Ask again each sweep until a peer has been asked and nobody is ahead: the first ask
            // after a recovery usually precedes any report, and the core can name no peer to ask.
            due.push(synod::Cmd::CatchUp { from_slot: self.slot_out });
        }
        self.pump(Vec::new(), cx, due);
    }

    /// `⟨ Init ⟩` — arm the sweep, and start the child, whose own `on_init` starts the leader
    /// detector. A protocol is owed exactly one of `on_init` and `on_recovery` before its first
    /// event, and the detector beneath is the one that has gone without twice.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.arm(cx);
        self.through_synod(cx, |s, ccx| s.on_init(ccx));
    }

    /// `⟨ Recovery ⟩` — §4.3. Read the record, write the new incarnation with the core's part carried
    /// across untouched, fold the applied entries back into the sequence, and hand the core its own
    /// recovery. The catch-up waits for the core's [`synod::Ind::Recovered`]: a process about to be
    /// told its storage lied should not be teaching or learning under that identity.
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        let held = cx.storage().get().cloned();
        self.incarnation = held.as_ref().map_or(0, |h| h.incarnation) + 1;
        // One write, this layer's own part changed and the child's slot preserved — the mirror of
        // what the slot does for the child's write.
        cx.storage()
            .set(Durable { incarnation: self.incarnation, synod: held.and_then(|h| h.synod) });

        // The sequence, from the records. `slot_out` is one past the last applied slot; a slot
        // skipped as a duplicate after it leaves no record, and the module documentation says what
        // that costs.
        let applied: Vec<(Slot, Command<V>)> = cx
            .storage()
            .read_from(Position::START)
            .into_iter()
            .filter_map(|e| match e {
                Entry::Applied { slot, command } => Some((*slot, command.clone())),
                Entry::Synod(_) => None,
            })
            .collect();
        for (slot, command) in applied {
            self.sequence.push(command.clone());
            self.performed.insert(command.clone());
            self.decisions.insert(slot, command);
            self.slot_out = slot + 1;
        }
        self.slot_in = self.slot_out;
        // The duplicate filter and `decisions` are bounded by the retention window, on recovery as
        // at any other time.
        self.forget_below();

        self.arm(cx);
        self.through_synod(cx, |s, ccx| s.on_recovery(ccx));
    }

    /// Hand the boundary to the child, which knows what it means. Leaving it to the trait's default
    /// would take a scope event the driver raised and drop it.
    fn on_scope_event(&mut self, scope: Self::Scope, cx: &mut ProtoCx<'_, Self>) {
        self.through_synod(cx, |s, ccx| s.on_scope_event(scope, ccx));
    }
}

impl<V: Clone + Ord, L: VolatileLink<Carried<V>>> TotalOrderLog<V> for MultiPaxosReplica<V, L> {
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
            Ind::StorageScopeEnded { peer } => LogInd::StorageScopeEnded { peer },
        }
    }
}
