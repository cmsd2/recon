//! Logged perfect point-to-point links.
//!
//! Cachin, Guerraoui & Rodrigues, Module 2.4 and Algorithm 2.3 ("Log Delivered").
//!
//! **Status: transcription. Space: unbounded — and on disk. Write cost: one append per message.**
//! `delivered` grows with every distinct message log-delivered and nothing retires an entry, as
//! [`crate::perfect_link`] does, except that here the growth is a file rather than a heap. See
//! `docs/bounded-space.md`; the fix is a delivered *cursor* rather than a delivered *set*, and it
//! is a change with a proposal.
//!
//! # The indication carries the log, not the message
//!
//! This is the whole of what the fail-recovery model changes about an interface, and the reason
//! this protocol sits at the bottom of the stack where it can be seen.
//!
//! A crash-stop protocol notifies the layer above by triggering `⟨ Deliver | m ⟩` once. A
//! crash-recovery protocol cannot. It may crash immediately afterwards, and then neither it nor
//! the layer above nor anyone else will ever know the indication happened — the message is lost
//! in a notification that no longer exists. So the module writes the message into a set in stable
//! storage, and the indication says only that *the set may have changed*:
//!
//! ```text
//! upon event ⟨ lpl, Init ⟩ do
//!     delivered := ∅;
//!     store(delivered);
//!
//! upon event ⟨ lpl, Recovery ⟩ do
//!     retrieve(delivered);
//!     trigger ⟨ lpl, Deliver | delivered ⟩;
//!
//! upon event ⟨ lpl, Send | q, m ⟩ do
//!     trigger ⟨ sl, Send | q, m ⟩;
//!
//! upon event ⟨ sl, Deliver | p, m ⟩ do
//!     if not exists (p′, m′) ∈ delivered such that m′ = m then
//!         delivered := delivered ∪ {(p, m)};
//!         store(delivered);
//!         trigger ⟨ lpl, Deliver | delivered ⟩;
//! ```
//!
//! The layer above reads the set rather than receiving a message, and must be idempotent: the
//! same set arrives again after every restart.
//!
//! # Reliable delivery is weaker here, and necessarily
//!
//! Module 2.3 promises delivery if a *correct* process sends to a correct process. Module 2.4
//! promises it only if a process that **never crashes** does. The difference is not fussiness: a
//! sender that crashes immediately after being asked to send may have no record that it was ever
//! asked, and in the crash-recovery model a process that crashes and recovers is still correct.
//! There is nothing left in the system to retransmit.
//!
//! # What the durable record buys
//!
//! [`crate::perfect_link`] keeps its `delivered` set in memory, so a restart forgets it and the
//! sender's next retransmission is delivered a second time — `no_duplication_does_not_survive_the_recipient_restarting`
//! records exactly that. Here the record survives, so LPL2 holds across incarnations rather than
//! within one. That is the entire purchase, and it is what stable storage is for.
//!
//! # Departures from the page
//!
//! - None worth the name for initialisation: `⟨ Init ⟩` and `⟨ Recovery ⟩` are both here, exactly
//!   one fires, and `Init` performs the book's initial `store(∅)`. That write is what makes the
//!   branch real rather than emergent — after it, storage holds something, so every later restart
//!   recovers. The constructor does volatile setup only, because it runs in both cases and cannot
//!   emit effects.
//! - `delivered` is keyed by sender and a per-sender sequence number rather than by message
//!   content, for the reason [`crate::perfect_link`] gives: identical content sent twice is two
//!   messages and must be delivered twice.
//! - No `Stop`: the stubborn link beneath retransmits for ever, which in this model is a feature —
//!   it is how a process that was down when a message was sent receives it after recovering.
//!
//! # The write cost is linear, not quadratic
//!
//! One append per message log-delivered, and the metadata written once at first start. Rewriting
//! the whole set on each arrival would cost `O(n²)` over a run, which is the failure mode
//! `docs/bounded-space.md` calls unbounded *work*: small per item, and growing without limit.
//!
//! The record still grows without limit, so this remains a transcription. What appending changes
//! is the cost of adding to it, not its size.

use core::time::Duration;
use recon_core::{NodeId, Position, ProtoCx, Protocol};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::perfect_link::MsgId;
use crate::stubborn_link::{self as sl, SendId, StubbornLink};

/// What goes on the wire: the payload and the identifier that names it.
///
/// The same shape [`crate::perfect_link`] uses, and for the same reason — deduplication is by
/// identifier, so that identical content sent twice is two messages.
pub type Wire<P> = crate::perfect_link::Wire<P>;

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Send { to: NodeId, msg: P },
}

/// Indications to the layer above.
///
/// One variant, carrying the durable log rather than a message. See the module note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P: Ord> {
    /// The durable set of log-delivered messages may have changed. Here it is.
    Delivered(Log<P>),
}

/// The set of messages log-delivered, in stable storage.
///
/// Ordered, so that reading it is deterministic and two processes holding the same set see it the
/// same way — the ordered-maps rule applies to what is written down as much as to what is held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = "P: Ord + Deserialize<'de>"))]
pub struct Log<P: Ord> {
    entries: BTreeSet<(MsgId, P)>,
}

impl<P: Ord> Default for Log<P> {
    fn default() -> Self {
        Log { entries: BTreeSet::new() }
    }
}

impl<P: Ord> Log<P> {
    /// Every message log-delivered, with the identifier naming its sender.
    pub fn entries(&self) -> impl Iterator<Item = &(MsgId, P)> + '_ {
        self.entries.iter()
    }

    /// How many messages have been log-delivered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether `id` has been log-delivered.
    pub fn contains(&self, id: MsgId) -> bool {
        self.entries.iter().any(|(i, _)| *i == id)
    }
}

/// Perfect-link guarantees over log-delivery, so that they hold across a restart.
#[derive(Debug)]
pub struct LoggedLink<P: Ord> {
    me: NodeId,
    seq: u64,
    /// The durable set. Volatile here, written down on every change, and retrieved on recovery.
    delivered: Log<P>,
    link: StubbornLink<Wire<P>>,
    inbox: Vec<sl::Ind<Wire<P>>>,
}

impl<P: Ord> LoggedLink<P> {
    /// Log-deliver for `me`, retransmitting every `retransmit`.
    pub fn new(me: NodeId, retransmit: Duration) -> Self {
        LoggedLink {
            me,
            seq: 0,
            delivered: Log::default(),
            link: StubbornLink::new(retransmit),
            inbox: Vec::new(),
        }
    }

    /// What has been log-delivered. The same value the layer above is handed.
    pub fn log(&self) -> &Log<P> {
        &self.delivered
    }
}

impl<P: Clone + Ord> LoggedLink<P> {
    fn with_link(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut StubbornLink<Wire<P>>, &mut ProtoCx<'_, StubbornLink<Wire<P>>>),
    ) {
        let mut inbox = core::mem::take(&mut self.inbox);
        inbox.clear();
        {
            let link = &mut self.link;
            cx.with_child_consuming(
                core::convert::identity,
                core::convert::identity,
                &mut inbox,
                |ccx| f(link, ccx),
            );
        }
        for sl::Ind::Deliver { from, msg } in inbox.drain(..) {
            self.on_arrival(from, msg, cx);
        }
        self.inbox = inbox;
    }

    /// `upon event ⟨ sl, Deliver | p, m ⟩`.
    fn on_arrival(&mut self, _from: NodeId, wire: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        if self.delivered.contains(wire.id) {
            // Seen before, in this incarnation or an earlier one. Nothing changed, so nothing is
            // written and nothing is announced.
            return;
        }
        let entry = (wire.id, wire.payload);
        self.delivered.entries.insert(entry.clone());
        // One append, not a rewrite — and durable before the layer above is told, so a crash in
        // between leaves the message in the record rather than in a lost notification.
        cx.storage().append(entry);
        cx.indicate(Ind::Delivered(self.delivered.clone()));
    }
}

impl<P: Clone + Ord> Protocol for LoggedLink<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    type Timer = sl::Retransmit;
    type Scope = core::convert::Infallible;
    /// Written once, at first start, so a later restart finds something.
    type Meta = ();
    /// One log-delivered message; appended, so the write cost is linear rather than quadratic.
    type Entry = (MsgId, P);

    /// `⟨ lpl, Init ⟩ do delivered := ∅; store(delivered)`.
    ///
    /// The initial write is what makes the branch real: after it, storage holds something, so
    /// every later restart takes the recovery path rather than starting afresh.
    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        cx.storage().set(());
    }

    fn on_cmd(&mut self, Cmd::Send { to, msg }: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let id = MsgId { src: self.me, seq: self.seq };
        let wire = Wire { id, payload: msg };
        // Stubbornly, and never stopped: retransmission is what reaches a process that was down.
        self.with_link(cx, |link, ccx| {
            link.on_cmd(sl::Cmd::Send { id: SendId(id.seq), to, msg: wire }, ccx)
        });
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_link(cx, |link, ccx| link.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, token: sl::Retransmit, cx: &mut ProtoCx<'_, Self>) {
        self.with_link(cx, |link, ccx| link.on_timer(token, ccx));
    }

    /// `upon event ⟨ lpl, Recovery ⟩ do retrieve(delivered); trigger ⟨ lpl, Deliver | delivered ⟩`.
    ///
    /// The record is read here rather than handed over. Nothing else is dispatched until this
    /// returns, which is what makes it safe to have held an empty index a moment ago.
    ///
    /// The layer above is told again: the notification sent before the crash may have been lost
    /// with the incarnation that sent it.
    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        let entries: Vec<(MsgId, P)> =
            cx.storage().read_from(Position::START).into_iter().cloned().collect();
        self.delivered = Log { entries: entries.into_iter().collect() };
        cx.indicate(Ind::Delivered(self.delivered.clone()));
    }
}
