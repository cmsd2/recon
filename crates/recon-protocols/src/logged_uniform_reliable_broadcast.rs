//! Logged uniform reliable broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.6 and Algorithm 3.8 ("Logged Majority-Ack Uniform
//! Reliable Broadcast").
//!
//! **Status: transcription. Space: unbounded — and on disk. Write cost: a fixed number of appends
//! per message.** `pending` and `delivered` grow with every message handled, as in
//! [`crate::majority_ack_uniform_reliable_broadcast`], except that here they are written down.
//! Each is recorded by appending one entry, so the cost of recording a message does not depend on
//! how many preceded it; the record itself is still unbounded. See `docs/bounded-space.md`.
//!
//! **Assumption: a correct majority, `N > 2f`** — where in this model a *correct* process is one
//! that always recovers from its crashes, and what it knows after recovering is what it wrote.
//!
//! ```text
//! upon event ⟨ lurb, Init ⟩ do
//!     delivered := ∅; pending := ∅;
//!     forall m do ack[m] := ∅;
//!     store(pending, delivered);
//!
//! upon event ⟨ lurb, Recovery ⟩ do
//!     retrieve(pending, delivered);
//!     trigger ⟨ lurb, Deliver | delivered ⟩;
//!     forall (s, m) ∈ pending do
//!         trigger ⟨ sbeb, Broadcast | [DATA, s, m] ⟩;
//!
//! upon event ⟨ lurb, Broadcast | m ⟩ do
//!     pending := pending ∪ {(self, m)};
//!     store(pending);
//!     trigger ⟨ sbeb, Broadcast | [DATA, self, m] ⟩;
//!
//! upon event ⟨ sbeb, Deliver | p, [DATA, s, m] ⟩ do
//!     if (s, m) ∉ pending then
//!         pending := pending ∪ {(s, m)};
//!         store(pending);
//!         trigger ⟨ sbeb, Broadcast | [DATA, s, m] ⟩;
//!     if p ∉ ack[m] then
//!         ack[m] := ack[m] ∪ {p};
//!         if #(ack[m]) > N/2 ∧ (s, m) ∉ delivered then
//!             delivered := delivered ∪ {(s, m)};
//!             store(delivered);
//!             trigger ⟨ lurb, Deliver | delivered ⟩;
//! ```
//!
//! # `ack` is deliberately not durable
//!
//! The book: *"Variable `ack` is not logged because it will be reconstructed upon recovery."*
//! Getting this wrong in the direction of storing more looks safer and is worse — it would cost a
//! write per acknowledgement to save work that retransmission does anyway, and would make the
//! durable state grow with *traffic* rather than with messages.
//!
//! What rebuilds it is the recovery clause: a recovered process re-broadcasts everything still
//! pending, and acknowledgements accumulate as the answers arrive. The stubborn broadcast beneath
//! never stops retransmitting, so a process that was down when something was sent gets it anyway.
//!
//! # Why the child is stubborn broadcast and not the logged link
//!
//! The book's logged abstractions do not stack: Algorithm 2.3 is over stubborn links, and this one is
//! over stubborn *broadcast*. Each keeps its own log. A perfect link's deduplication is volatile,
//! so after a restart it would re-deliver anyway — the deduplication buys nothing a logged layer
//! above does not already do for itself, and it is the **retransmission** a recovered process
//! needs. Deduplicating beneath would suppress exactly that.
//!
//! # Departures from the page
//!
//! - `⟨ Init ⟩` is here and performs the book's initial store, as [`crate::logged_link`] does.
//! - `pending` and `delivered` share one appended sequence, distinguished by a tag on each entry,
//!   rather than the book's two stores. Recovery replays the sequence and rebuilds both.
//! - Messages are keyed by an identifier carrying the originator and a sequence number rather
//!   than by content, so identical content broadcast twice is delivered twice.
//! - The sequence number is therefore as durable as the log it keys, and recovery recomputes it
//!   rather than resuming from zero. See the note below; the obligation is part of the departure.
//! - No `Stop`: retransmission for ever is what reaches a recovered process.
//!
//! # The sequence number survives a crash, without being written
//!
//! Content-keyed identity, as the book has it, needs nothing restored: a payload names itself. An
//! id-keyed departure owes the counter the same durability as the set it keys, or a recovered
//! process re-mints `(me, 1)` for something *new* and two distinct payloads collide under one
//! identifier — last-write-wins in `pending`, acks for the old counting toward the new, at most
//! one of the two ever delivered locally, and different processes log-delivering different
//! payloads under the same id. No-creation, validity and uniform agreement all fail, in the one
//! module whose model expects a recovered process to keep working.
//!
//! Recovery recomputes it as the greatest `seq` over replayed records originating here, which is
//! sound because every own broadcast appends its `Pending` record in the handler that emits it:
//! a torn write discards that handler's sends, so no broadcast escapes without a record. The
//! alternative — the counter in `Meta` — is also correct and costs a metadata write per
//! broadcast, which is what buys nothing here.

use core::time::Duration;
use recon_core::{Child, NodeId, Position, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::stubborn_broadcast::{self as sbeb, StubbornBroadcast};
use crate::uniform_reliable_broadcast::{BroadcastId, Data};

/// What survives a crash: what has been seen, and what has been log-delivered.
///
/// `ack` is absent, deliberately. See the module note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = "P: Ord + Deserialize<'de>"))]
pub struct Logged<P: Ord> {
    pending: BTreeMap<BroadcastId, P>,
    delivered: BTreeSet<(BroadcastId, P)>,
}

impl<P: Ord> Default for Logged<P> {
    fn default() -> Self {
        Logged { pending: BTreeMap::new(), delivered: BTreeSet::new() }
    }
}

impl<P: Ord> Logged<P> {
    /// Everything log-delivered, with the identifier naming its originator.
    pub fn delivered(&self) -> impl Iterator<Item = &(BroadcastId, P)> + '_ {
        self.delivered.iter()
    }

    /// How many messages have been log-delivered.
    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    /// How many messages have been seen and are still being re-broadcast.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// One thing written down. Replaying these in order rebuilds `pending` and `delivered`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record<P> {
    /// Seen, and being re-broadcast until a majority has it.
    Pending(BroadcastId, P),
    /// Log-delivered.
    Delivered(BroadcastId, P),
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Broadcast(P),
}

/// Indications to the layer above: the durable log, not a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P: Ord> {
    Delivered(Logged<P>),
}

/// Uniform agreement over log-delivery, in the fail-recovery model.
#[derive(Debug)]
pub struct LoggedUniformReliableBroadcast<P: Clone + Ord> {
    me: NodeId,
    seq: u64,
    members: usize,
    /// Durable. Written on every change and retrieved on recovery.
    log: Logged<P>,
    /// Volatile, and rebuilt by re-broadcasting on recovery. Not written down.
    ack: BTreeMap<BroadcastId, BTreeSet<NodeId>>,
    /// Names each fan-out to the stubborn broadcast beneath. Volatile, and matched to the child's
    /// own volatile state — a crash takes both, so nothing outlives the counter that keys it.
    /// Not the durable [`BroadcastId`]: nothing here is ever stopped, so the two never meet.
    beb_seq: u64,
    beb: Child<StubbornBroadcast<Data<P>>>,
}

impl<P: Clone + Ord> LoggedUniformReliableBroadcast<P> {
    /// Broadcast among `members`, which must include `me`, retransmitting every `interval`.
    pub fn new(me: NodeId, members: impl IntoIterator<Item = NodeId>, interval: Duration) -> Self {
        let mut members: BTreeSet<NodeId> = members.into_iter().collect();
        members.insert(me);
        let n = members.len();
        LoggedUniformReliableBroadcast {
            me,
            seq: 0,
            members: n,
            log: Logged::default(),
            ack: BTreeMap::new(),
            beb_seq: 0,
            beb: Child::new(StubbornBroadcast::new(me, members, interval)),
        }
    }

    /// The durable log. The same value the layer above is handed.
    pub fn log(&self) -> &Logged<P> {
        &self.log
    }

    /// How many processes must have re-broadcast a message before it is log-delivered.
    pub fn majority(&self) -> usize {
        self.members / 2 + 1
    }

    /// Which processes have been seen to re-broadcast `id`. Volatile, and rebuilt on recovery.
    pub fn acknowledged_by(&self, id: BroadcastId) -> impl Iterator<Item = NodeId> + '_ {
        self.ack.get(&id).into_iter().flatten().copied()
    }
}

impl<P: Clone + Ord> LoggedUniformReliableBroadcast<P> {
    fn with_beb(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut StubbornBroadcast<Data<P>>, &mut ProtoCx<'_, StubbornBroadcast<Data<P>>>),
    ) {
        let mut inds = self.beb.run(cx, core::convert::identity, f);
        for sbeb::Ind::Deliver { from, msg } in inds.drain(..) {
            self.on_arrival(from, msg, cx);
        }
        self.beb.reclaim(inds);
    }

    fn rebroadcast(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        self.beb_seq += 1;
        let id = sbeb::BroadcastId(self.beb_seq);
        // Re-enters the child while its inbox is out on loan, so `run` hands back a fresh one.
        let inds = self.beb.run(cx, core::convert::identity, |beb, ccx| {
            beb.on_cmd(sbeb::Cmd::Broadcast { id, msg: data }, ccx)
        });
        debug_assert!(inds.is_empty(), "broadcasting must not deliver synchronously");
        self.beb.reclaim(inds);
    }

    /// `upon event ⟨ sbeb, Deliver | p, [DATA, s, m] ⟩`.
    ///
    /// Arrives many times for one broadcast — the child never stops retransmitting — so every
    /// branch here is guarded and idempotent.
    fn on_arrival(&mut self, from: NodeId, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        let id = data.id;

        // Re-broadcast only on first sight. An identifier determines its payload, so re-inserting
        // cannot change what is pending — the returned Option is read only to learn whether this
        // was the first time.
        if self.log.pending.insert(id, data.payload.clone()).is_none() {
            // Durable at the point of insertion, in this handler's own text: the re-broadcast is
            // this process's acknowledgement, and under an eager sink it escapes before the
            // handler returns. Everything in `pending` has a durable `Pending` record by
            // construction, not by where control happens to leave.
            cx.storage().append(Record::Pending(id, data.payload.clone()));
            self.rebroadcast(data.clone(), cx);
        }

        if self.ack.entry(id).or_default().insert(from) {
            let acked = self.ack.get(&id).map(|a| a.len()).unwrap_or(0);
            let already = self.log.delivered.iter().any(|(i, _)| *i == id);
            if 2 * acked > self.members && !already {
                self.log.delivered.insert((id, data.payload.clone()));
                // Durable before announced: the layer above must not learn of a delivery a crash
                // could erase.
                cx.storage().append(Record::Delivered(id, data.payload));
                cx.indicate(Ind::Delivered(self.log.clone()));
            }
        }
    }
}

impl<P: Clone + Ord> Protocol for LoggedUniformReliableBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Data<P>;
    type Scope = core::convert::Infallible;
    /// Nothing is rewritten; the metadata is written once so a restart finds something.
    type Meta = ();
    /// One record per message seen or log-delivered. `ack` is not among them, by design.
    type Entry = Record<P>;

    /// `⟨ lurb, Init ⟩ do delivered := ∅; pending := ∅; ...; store(pending, delivered)`.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        cx.storage().set(());
    }

    fn on_cmd(&mut self, Cmd::Broadcast(msg): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let id = BroadcastId { origin: self.me, seq: self.seq };
        self.log.pending.insert(id, msg.clone());
        cx.storage().append(Record::Pending(id, msg.clone()));
        self.rebroadcast(Data { id, payload: msg }, cx);
    }

    fn on_msg(&mut self, from: NodeId, msg: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_timer(id, ccx));
    }

    /// `upon event ⟨ lurb, Recovery ⟩`.
    ///
    /// Re-announce the log, then re-broadcast everything pending — which is what rebuilds `ack`,
    /// and why it need not be durable. The replay also restores the send counter, so a process
    /// that goes on to broadcast something new does not reuse an identifier.
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        // Nothing else is dispatched until this returns, so an empty index a moment ago is safe.
        let records: Vec<Record<P>> =
            cx.storage().read_from(Position::START).into_iter().cloned().collect();
        for r in records {
            let id = match r {
                Record::Pending(id, p) => {
                    self.log.pending.insert(id, p);
                    id
                }
                Record::Delivered(id, p) => {
                    self.log.delivered.insert((id, p));
                    id
                }
            };
            // The counter is as durable as the set it keys. Resuming from zero would re-mint an
            // identifier already in use for a different payload; see the module note.
            if id.origin == self.me {
                self.seq = self.seq.max(id.seq);
            }
        }
        cx.indicate(Ind::Delivered(self.log.clone()));
        let outstanding: Vec<Data<P>> = self
            .log
            .pending
            .iter()
            .map(|(id, payload)| Data { id: *id, payload: payload.clone() })
            .collect();
        for data in outstanding {
            self.rebroadcast(data, cx);
        }
    }
}
