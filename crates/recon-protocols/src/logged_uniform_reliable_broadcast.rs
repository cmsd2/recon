//! Logged uniform reliable broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, Module 3.6 and Algorithm 3.8 ("Logged Majority-Ack Uniform
//! Reliable Broadcast").
//!
//! **Status: transcription. Space: unbounded — and on disk.** `pending` and `delivered` grow with
//! every message handled, as in [`crate::majority_ack_uniform_reliable_broadcast`], except that
//! here they are written down. See `docs/bounded-space.md`.
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
//! - `pending` and `delivered` are written together as one value, because the durable state is
//!   one value in full — see [`recon_core::Protocol::Durable`]. The book stores them separately;
//!   nothing here depends on the difference.
//! - Messages are keyed by an identifier carrying the originator and a sequence number rather
//!   than by content, so identical content broadcast twice is delivered twice.
//! - No `Stop`: retransmission for ever is what reaches a recovered process.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, absurd};
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
pub struct LoggedUniformReliableBroadcast<P: Ord> {
    me: NodeId,
    seq: u64,
    members: usize,
    /// Durable. Written on every change and retrieved on recovery.
    log: Logged<P>,
    /// Volatile, and rebuilt by re-broadcasting on recovery. Not written down.
    ack: BTreeMap<BroadcastId, BTreeSet<NodeId>>,
    beb: StubbornBroadcast<Data<P>>,
    inbox: Vec<sbeb::Ind<Data<P>>>,
    send_inbox: Vec<sbeb::Ind<Data<P>>>,
}

impl<P: Ord> LoggedUniformReliableBroadcast<P> {
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
            beb: StubbornBroadcast::new(me, members, interval),
            inbox: Vec::new(),
            send_inbox: Vec::new(),
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
        let mut inbox = core::mem::take(&mut self.inbox);
        inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(
                core::convert::identity,
                core::convert::identity,
                absurd,
                &mut inbox,
                |ccx| f(beb, ccx),
            );
        }
        for sbeb::Ind::Deliver { from, msg } in inbox.drain(..) {
            self.on_arrival(from, msg, cx);
        }
        self.inbox = inbox;
    }

    fn rebroadcast(&mut self, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        let mut send_inbox = core::mem::take(&mut self.send_inbox);
        send_inbox.clear();
        {
            let beb = &mut self.beb;
            cx.with_child_consuming(
                core::convert::identity,
                core::convert::identity,
                absurd,
                &mut send_inbox,
                |ccx| beb.on_cmd(sbeb::Cmd::Broadcast(data), ccx),
            );
        }
        debug_assert!(send_inbox.is_empty(), "broadcasting must not deliver synchronously");
        self.send_inbox = send_inbox;
    }

    /// `upon event ⟨ sbeb, Deliver | p, [DATA, s, m] ⟩`.
    ///
    /// Arrives many times for one broadcast — the child never stops retransmitting — so every
    /// branch here is guarded and idempotent.
    fn on_arrival(&mut self, from: NodeId, data: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        let id = data.id;
        let mut wrote = false;

        // Re-broadcast only on first sight. An identifier determines its payload, so re-inserting
        // cannot change what is pending — the returned Option is read only to learn whether this
        // was the first time.
        if self.log.pending.insert(id, data.payload.clone()).is_none() {
            wrote = true;
            self.rebroadcast(data.clone(), cx);
        }

        if self.ack.entry(id).or_default().insert(from) {
            let acked = self.ack.get(&id).map(|a| a.len()).unwrap_or(0);
            let already = self.log.delivered.iter().any(|(i, _)| *i == id);
            if 2 * acked > self.members && !already {
                self.log.delivered.insert((id, data.payload));
                // Durable first, announced second: the layer above must not learn of a delivery
                // that a crash could erase.
                cx.store(self.log.clone());
                cx.indicate(Ind::Delivered(self.log.clone()));
                return;
            }
        }

        if wrote {
            cx.store(self.log.clone());
        }
    }
}

impl<P: Clone + Ord> Protocol for LoggedUniformReliableBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Data<P>;
    type Timer = crate::stubborn_link::Retransmit;
    type Scope = core::convert::Infallible;
    /// `pending` and `delivered`, in one value. `ack` is not here, by design.
    type Durable = Logged<P>;

    /// `⟨ lurb, Init ⟩ do delivered := ∅; pending := ∅; ...; store(pending, delivered)`.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        cx.store(self.log.clone());
    }

    fn on_cmd(&mut self, Cmd::Broadcast(msg): Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let id = BroadcastId { origin: self.me, seq: self.seq };
        self.log.pending.insert(id, msg.clone());
        cx.store(self.log.clone());
        self.rebroadcast(Data { id, payload: msg }, cx);
    }

    fn on_msg(&mut self, from: NodeId, msg: Data<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, token: crate::stubborn_link::Retransmit, cx: &mut ProtoCx<'_, Self>) {
        self.with_beb(cx, |beb, ccx| beb.on_timer(token, ccx));
    }

    /// `upon event ⟨ lurb, Recovery ⟩`.
    ///
    /// Re-announce the log, then re-broadcast everything pending — which is what rebuilds `ack`,
    /// and why it need not be durable.
    fn on_recovery(&mut self, log: Logged<P>, cx: &mut ProtoCx<'_, Self>) {
        self.log = log;
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
