//! Perfect point-to-point links.
//!
//! Cachin, Guerraoui & Rodrigues, Module 2.3 and Algorithm 2.2 ("Eliminate Duplicates").
//!
//! **Status: academic as written. Space: unbounded.** Within a TCP or QUIC session, PL1–PL3 come
//! from the transport. The deployable equivalent is a *session link*: those guarantees from the
//! session, plus an event when the session changes and an unknown suffix may have been lost. It
//! needs less state than this, not more — within a session the transport does not duplicate, so
//! there is nothing to deduplicate.
//!
//! `delivered` here grows with every message received. See `docs/bounded-space.md`.
//!
//! Built over the stubborn link, which delivers a message infinitely often. This layer keeps
//! the first copy of each message and discards the rest, yielding reliable delivery with no
//! duplication and no creation.
//!
//! ```text
//! upon event ⟨ pl, Send | q, m ⟩ do
//!     trigger ⟨ sl, Send | q, m ⟩;
//!
//! upon event ⟨ sl, Deliver | p, m ⟩ do
//!     if m ∉ delivered then
//!         delivered := delivered ∪ {m};
//!         trigger ⟨ pl, Deliver | p, m ⟩;
//! ```
//!
//! **One deliberate departure.** The book deduplicates on the message *content*, `m`, which
//! silently assumes every message is distinct. Send the same bytes twice on purpose and the
//! second is swallowed. This implementation tags each transmission with an identifier — the
//! sender and a sequence number — and deduplicates on that instead, so a genuine resend of
//! identical content is delivered twice, as the layer above expects.
//!
//! That identifier is the only thing this stack puts on the wire: the stubborn link below adds
//! nothing, and best-effort broadcast above adds nothing.
//!
//! # The counter lives exactly as long as the set it keys
//!
//! Both are volatile, and a crash takes both, so the pairing holds: a restarted sender re-mints
//! `(me, 1)` at exactly the point where every recipient's `delivered` has also been forgotten,
//! and the recipient re-delivers the old messages anyway —
//! `no_duplication_does_not_survive_the_recipient_restarting` records that. PL2 is scoped to an
//! incarnation here, which is the crash-stop model's premise: a crashed process is not correct,
//! and nothing is promised across the restart the simulator can nevertheless perform.
//!
//! The hazard to watch for is the mismatched pair — a durable set keyed by a volatile counter,
//! where the recipient remembers what the sender has forgotten and discards new messages as
//! duplicates. [`crate::logged_link`] is that configuration, and makes the counter durable.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::stubborn_link::{self as sl, StubbornLink};

/// Names one transmission uniquely across the system.
///
/// The sender plus a per-sender sequence number. Deduplicating on this rather than on message
/// content is what lets identical payloads be sent twice and delivered twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MsgId {
    pub src: NodeId,
    pub seq: u64,
}

/// What crosses the wire: the identifier, and the payload it belongs to.
///
/// The single header in the three-layer stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire<P> {
    pub id: MsgId,
    pub payload: P,
}

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Send { to: NodeId, msg: P },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    Deliver { from: NodeId, msg: P },
}

/// Reliable delivery, exactly once, over a stubborn link.
#[derive(Debug)]
pub struct PerfectLink<P> {
    me: NodeId,
    seq: u64,
    delivered: BTreeSet<MsgId>,
    stubborn: StubbornLink<Wire<P>>,
    /// Indications the child raised during a handler, awaiting this protocol's attention.
    /// Reused across events.
    inbox: Vec<sl::Ind<Wire<P>>>,
}

impl<P> PerfectLink<P> {
    /// A perfect link for process `me`, retransmitting every `interval` underneath.
    pub fn new(me: NodeId, interval: Duration) -> Self {
        PerfectLink {
            me,
            seq: 0,
            delivered: BTreeSet::new(),
            stubborn: StubbornLink::new(interval),
            inbox: Vec::new(),
        }
    }

    /// How many distinct messages have been delivered upward.
    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    /// How many transmissions the layer below is still retrying.
    pub fn outstanding(&self) -> usize {
        self.stubborn.outstanding()
    }
}

impl<P: Clone> PerfectLink<P> {
    /// Run the child, then apply what it reported: keep the first copy of each identifier and
    /// drop the rest.
    ///
    /// Every handler is this, differing only in which child method it calls. Collecting the
    /// ceremony here rather than repeating it three times is what keeps the handlers readable —
    /// and it leaves the borrow structure visible in one place instead of hiding it in a macro.
    fn with_stubborn(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut StubbornLink<Wire<P>>, &mut ProtoCx<'_, StubbornLink<Wire<P>>>),
    ) {
        let mut inbox = core::mem::take(&mut self.inbox);
        {
            let stubborn = &mut self.stubborn;
            cx.with_child_consuming(core::convert::identity, &mut inbox, |ccx| f(stubborn, ccx));
        }
        for ind in inbox.drain(..) {
            let sl::Ind::Deliver { from, msg: Wire { id, payload } } = ind;
            if self.delivered.insert(id) {
                cx.indicate(Ind::Deliver { from, msg: payload });
            }
        }
        self.inbox = inbox;
    }
}

impl<P: Clone> Protocol for PerfectLink<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = Wire<P>;
    /// No scope conditions: this protocol's guarantees do not lapse.
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Send { to, msg }: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        self.seq += 1;
        let id = MsgId { src: self.me, seq: self.seq };
        let wire = Wire { id, payload: msg };
        self.with_stubborn(cx, |sl, ccx| {
            sl.on_cmd(sl::Cmd::Send { id: sl::SendId(id.seq), to, msg: wire }, ccx)
        });
    }

    fn on_msg(&mut self, from: NodeId, msg: Wire<P>, cx: &mut ProtoCx<'_, Self>) {
        self.with_stubborn(cx, |sl, ccx| sl.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.with_stubborn(cx, |sl, ccx| sl.on_timer(id, ccx));
    }
}
