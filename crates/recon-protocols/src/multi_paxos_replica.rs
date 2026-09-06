//! The Multi-Paxos replica: slots become positions in a log.
//!
//! **Status: transcription. Space: unbounded — `decisions`, `performed` and the ordered sequence
//! all grow with the number of commands handled, and nothing collects them.** That is the page:
//! §4.2's watermark is what bounds a replica, and it is a later change. `docs/bounded-space.md`
//! is explicit that inheriting the source's omissions is correct of a transcription and
//! disqualifying of an implementation.
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
//! Crash-stop, and one step stronger than the child's. `MultiPaxosSynod` keeps nothing durably, so
//! a returning process has forgotten which ballots it took up; this layer adds that a returning
//! process has forgotten its *sequence*, and would answer a read with a shorter one than it had
//! already served. A total order that shortens is not a total order, so a crashed process here is
//! crashed for good. §4.3 of the source is what changes it, and it is not this change.

use core::time::Duration;
use recon_core::{Child, NodeId, Position, ProtoCx, Protocol, Time, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::Timing;
use crate::link::{Boundary, VolatileLink};
use crate::multi_paxos_synod::{self as synod, MultiPaxosSynod, Slot, SynodMsg};
use crate::session_link::SessionLink;
use crate::total_order_log::{LogInd, TotalOrderLog};

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
/// **`cid`'s scope is this incarnation.** It is a counter in volatile state, exactly as the Synod
/// ballot's round is, and a restarted process re-mints values it has already used. A request
/// carrying a reused `⟨from, cid⟩` can be taken for one already applied and dropped, which is the
/// identity rule's worked example: an identifier that crosses the wire outlives the handler that
/// minted it, so its generator is state with a scope, and this one survives nothing. Making it
/// durable is part of the fail-recovery change, not this one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Command<V> {
    /// `κ` — the process that appended it.
    pub from: NodeId,
    /// `cid` — which request of that process's this is. Scope: this incarnation.
    pub cid: u64,
    /// `op` — what the command says. A value rather than an operation; see the departures.
    pub value: V,
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
    /// `cid`. Volatile, scope this incarnation — see [`Command`].
    cid: u64,
    /// `WINDOW`.
    window: Slot,

    // ---- the fourth liveness violation ----
    /// When each outstanding proposal was last handed to the child.
    proposed_at: BTreeMap<Slot, Time>,
    /// How long a proposal may be outstanding before it is proposed again for the same slot.
    repropose_after: Duration,
    /// How often the outstanding set is swept.
    sweep_every: Duration,
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
            cid: 0,
            window: WINDOW,
            proposed_at: BTreeMap::new(),
            repropose_after: timing.detect_after * 3,
            sweep_every: timing.retransmit,
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

    /// The Synod protocol beneath, for a test that asks who leads.
    pub fn synod(&self) -> &Synod<V, L> {
        &self.synod
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
        while !self.requests.is_empty() {
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
        mut extra: Vec<(Slot, Command<V>)>,
    ) {
        loop {
            for ind in pending.drain(..) {
                match ind {
                    synod::Ind::Decision { slot, command } => self.decided(slot, command, cx),
                    // The child bridges no session ending and neither does this layer: its
                    // redundancy is the other processes, which a session ending does not restore.
                    synod::Ind::SessionEnded { peer, epoch } => {
                        cx.indicate(Ind::SessionEnded { peer, epoch });
                    }
                    synod::Ind::SessionEstablished { peer, epoch } => {
                        cx.indicate(Ind::SessionEstablished { peer, epoch });
                    }
                }
            }
            // `propose();`
            let now = cx.now();
            let mut outgoing = self.transfer(now, cx);
            outgoing.append(&mut extra);
            if outgoing.is_empty() {
                break;
            }
            for (slot, command) in outgoing {
                let mut inds = self.synod.run(
                    cx,
                    |m| m,
                    |s, ccx| s.on_cmd(synod::Cmd::Propose { slot, command }, ccx),
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
        // The wrap is the identity: this layer adds no header, so its wire is the child's.
        let inds = self.synod.run(cx, |m| m, f);
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
    /// Keeps nothing durably, which is what makes the crash-stop boundary in the module
    /// documentation the one that applies. §4.3 is the change that alters it.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<V>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            // `case ⟨request, c⟩ : requests := requests ∪ {c};` then `propose()`.
            Cmd::Append(value) => {
                let command = Command { from: self.me, cid: self.cid, value };
                self.cid += 1;
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
        let due = self.resweep(cx);
        self.pump(Vec::new(), cx, due);
    }

    /// `⟨ Init ⟩` — arm the sweep, and start the child, whose own `on_init` starts the leader
    /// detector. A protocol is owed exactly one of `on_init` and `on_recovery` before its first
    /// event, and the detector beneath is the one that has gone without twice.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.arm(cx);
        self.through_synod(cx, |s, ccx| s.on_init(ccx));
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
        }
    }
}
