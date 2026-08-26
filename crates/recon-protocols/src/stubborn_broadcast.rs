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
//! - `Stop` is offered so a caller that knows a message is everywhere can retire it. Nothing in
//!   this repository calls it, and the space note above is honest about what that costs.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol};
use std::collections::BTreeSet;

use crate::stubborn_link::{self as sl, SendId, StubbornLink};

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    /// Broadcast `msg`, and keep transmitting it until stopped.
    Broadcast(P),
    /// Stop retransmitting an earlier broadcast.
    Stop { id: SendId },
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
    link: StubbornLink<P>,
    inbox: Vec<sl::Ind<P>>,
}

impl<P> StubbornBroadcast<P> {
    /// Broadcast among `members`, which must include `me`, retransmitting every `interval`.
    pub fn new(me: NodeId, members: impl IntoIterator<Item = NodeId>, interval: Duration) -> Self {
        let mut peers: BTreeSet<NodeId> = members.into_iter().collect();
        peers.insert(me);
        StubbornBroadcast { peers, seq: 0, link: StubbornLink::new(interval), inbox: Vec::new() }
    }

    /// The process set. This layer's whole state, besides what is still being transmitted.
    pub fn peers(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.peers.iter().copied()
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
            Cmd::Broadcast(msg) => {
                let peers: Vec<NodeId> = self.peers.iter().copied().collect();
                for q in peers {
                    self.seq += 1;
                    let id = SendId(self.seq);
                    let msg = msg.clone();
                    self.with_link(cx, |link, ccx| {
                        link.on_cmd(sl::Cmd::Send { id, to: q, msg }, ccx)
                    });
                }
            }
            Cmd::Stop { id } => {
                self.with_link(cx, |link, ccx| link.on_cmd(sl::Cmd::Stop { id }, ccx))
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
