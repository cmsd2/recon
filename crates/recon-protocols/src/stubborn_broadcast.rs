//! Stubborn best-effort broadcast.
//!
//! Cachin, Guerraoui & Rodrigues, §3.5.
//!
//! **Status: deployable in the fail-recovery model. Space: bounded by membership and by what is
//! outstanding.** It holds the process set and the messages it is still transmitting, and nothing
//! per delivery.
//!
//! ```text
//! upon event ⟨ sbeb, Broadcast | m ⟩ do
//!     forall q ∈ Π do trigger ⟨ sl, Send | q, m ⟩;
//!
//! upon event ⟨ sl, Deliver | p, m ⟩ do
//!     trigger ⟨ sbeb, Deliver | p, m ⟩;
//! ```
//!
//! # Why the repeats are the point
//!
//! [`crate::best_effort_broadcast`] fans out over perfect links, which deduplicate and stop
//! retransmitting once a message has arrived. That is right in the crash-stop model and wrong in
//! the fail-recovery one: a process that was **down** when a message was sent has no record of it
//! and no way to ask, so the only thing that reaches it is a sender that never stopped trying.
//!
//! So this layer does not deduplicate, and must not. The repeats are what the layer above is for:
//! [`crate::logged_uniform_reliable_broadcast`] checks its own durable log before acting, and is
//! idempotent by construction. Deduplicating here would suppress exactly the retransmission a
//! recovered process depends on.
//!
//! # Departures from the page
//!
//! - The message carries no identifier, because nothing here deduplicates. That leaves the layer
//!   above to name its own messages, which is what it already does.
//! - `Stop` is offered so a caller that knows a message is everywhere can retire it. The book has
//!   no such request and never lets go; what is outstanding is the whole of the space claim
//!   above, so a `Stop` nobody can name would make that claim rest on nothing. The caller names
//!   the broadcast, and one name retires the fan-out of `N` link transmissions it became.
//!
//!   Nothing in this repository calls it yet: [`crate::logged_uniform_reliable_broadcast`] never
//!   stops, because retransmission for ever is what reaches a recovered process, and its space is
//!   unbounded for that reason and says so.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol};
use std::collections::{BTreeMap, BTreeSet};

use crate::stubborn_link::{self as sl, SendId, StubbornLink};

/// Names one broadcast, so the caller can later retire it.
///
/// Distinct from the [`SendId`]s beneath: one broadcast becomes `N` stubborn transmissions, and
/// the caller has no way to name those — they are minted here. The layer above allocates this,
/// and the same rule applies as to a `SendId`: it must not name a broadcast that is still live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BroadcastId(pub u64);

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    /// Broadcast `msg` as `id`, and keep transmitting it until stopped.
    Broadcast { id: BroadcastId, msg: P },
    /// Stop retransmitting the broadcast named `id`, on every link it went out over.
    Stop { id: BroadcastId },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    /// A message arrived. Raised many times for one broadcast, by design.
    Deliver { from: NodeId, msg: P },
}

/// Fan-out that never gives up.
#[derive(Debug)]
pub struct StubbornBroadcast<P> {
    peers: BTreeSet<NodeId>,
    seq: u64,
    /// What each live broadcast became: one stubborn transmission per peer. Bounded by
    /// membership times what is outstanding, and this is what `Stop` needs in order to work.
    outstanding: BTreeMap<BroadcastId, Vec<SendId>>,
    link: StubbornLink<P>,
    inbox: Vec<sl::Ind<P>>,
}

impl<P> StubbornBroadcast<P> {
    /// Broadcast among `members`, which must include `me`, retransmitting every `interval`.
    pub fn new(me: NodeId, members: impl IntoIterator<Item = NodeId>, interval: Duration) -> Self {
        let mut peers: BTreeSet<NodeId> = members.into_iter().collect();
        peers.insert(me);
        StubbornBroadcast {
            peers,
            seq: 0,
            outstanding: BTreeMap::new(),
            link: StubbornLink::new(interval),
            inbox: Vec::new(),
        }
    }

    /// The process set. This layer's whole state, besides what is still being transmitted.
    pub fn peers(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.peers.iter().copied()
    }

    /// How many broadcasts are still being retransmitted. Nothing retires one but [`Cmd::Stop`].
    pub fn outstanding_count(&self) -> usize {
        self.outstanding.len()
    }
}

impl<P: Clone> StubbornBroadcast<P> {
    fn with_link(
        &mut self,
        cx: &mut ProtoCx<'_, Self>,
        f: impl FnOnce(&mut StubbornLink<P>, &mut ProtoCx<'_, StubbornLink<P>>),
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
            // Straight through. No deduplication, deliberately.
            cx.indicate(Ind::Deliver { from, msg });
        }
        self.inbox = inbox;
    }
}

impl<P: Clone> Protocol for StubbornBroadcast<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = P;
    type Timer = sl::Retransmit;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: what it is transmitting is rebuilt by the layer above on recovery.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            Cmd::Broadcast { id, msg } => {
                debug_assert!(!self.outstanding.contains_key(&id), "BroadcastId {id:?} is live");
                let peers: Vec<NodeId> = self.peers.iter().copied().collect();
                let mut sends = Vec::with_capacity(peers.len());
                for q in peers {
                    self.seq += 1;
                    let send = SendId(self.seq);
                    sends.push(send);
                    let msg = msg.clone();
                    self.with_link(cx, |link, ccx| {
                        link.on_cmd(sl::Cmd::Send { id: send, to: q, msg }, ccx)
                    });
                }
                self.outstanding.insert(id, sends);
            }
            Cmd::Stop { id } => {
                // One name, `N` transmissions. The link cannot do this itself: it never learns
                // that the fan-out was one broadcast.
                for send in self.outstanding.remove(&id).unwrap_or_default() {
                    self.with_link(cx, |link, ccx| link.on_cmd(sl::Cmd::Stop { id: send }, ccx));
                }
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: P, cx: &mut ProtoCx<'_, Self>) {
        self.with_link(cx, |link, ccx| link.on_msg(from, msg, ccx));
    }

    fn on_timer(&mut self, token: sl::Retransmit, cx: &mut ProtoCx<'_, Self>) {
        self.with_link(cx, |link, ccx| link.on_timer(token, ccx));
    }
}
