//! Stubborn point-to-point links.
//!
//! Cachin, Guerraoui & Rodrigues, Module 2.2 and Algorithm 2.1 ("Retransmit Forever").
//!
//! **Status: academic. Space: unbounded.** This is how a perfect link is built when the only
//! thing underneath is a lossy datagram service — the simulator's situation, and not a
//! deployment's, where TCP and QUIC retransmit already. It stays because everything above needs a
//! perfect link and the simulator offers only fair-loss.
//!
//! `sent` grows with every transmission and nothing retires an entry unless the layer above stops
//! it, which Algorithm 2.2 never does. Every entry is re-sent on every tick, so the cost grows
//! with everything ever sent. See `docs/bounded-space.md`; the fix is not to bound this but to
//! ship a session link instead.
//!
//! Turns a fair-loss network into one where a message sent between correct processes is
//! eventually delivered, by retransmitting it at a fixed interval. The cost is unbounded
//! duplication: the recipient delivers the message infinitely often. Suppressing that is the
//! perfect link's job, not this one's.
//!
//! ```text
//! upon event ⟨ sl, Send | q, m ⟩ do
//!     trigger ⟨ fll, Send | q, m ⟩;
//!     sent := sent ∪ {(q, m)};
//!
//! upon event ⟨ Timeout ⟩ do
//!     forall (q, m) ∈ sent do trigger ⟨ fll, Send | q, m ⟩;
//!     starttimer(Δ);
//!
//! upon event ⟨ fll, Deliver | p, m ⟩ do
//!     trigger ⟨ sl, Deliver | p, m ⟩;
//! ```

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol};
use std::collections::BTreeMap;

/// Identifies one stubborn transmission, so it can later be stopped.
///
/// The book retransmits forever and never stops. A running system needs a way to let go, or
/// `sent` grows without bound — so the caller names each transmission and can retire it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SendId(pub u64);

/// Requests from the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<M> {
    /// Transmit `msg` to `to`, and keep transmitting it until stopped.
    Send { id: SendId, to: NodeId, msg: M },
    /// Stop retransmitting the transmission named `id`.
    Stop { id: SendId },
}

/// Indications to the layer above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<M> {
    /// A message arrived. May be raised many times for one transmission.
    Deliver { from: NodeId, msg: M },
}

/// This protocol's only timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retransmit;

/// Retransmits until told to stop.
///
/// Adds nothing to the wire: what it transmits is exactly the payload it was given. That is why
/// a three-layer stack built on it carries only one header.
#[derive(Debug)]
pub struct StubbornLink<M> {
    interval: Duration,
    sent: BTreeMap<SendId, (NodeId, M)>,
    armed: bool,
}

impl<M> StubbornLink<M> {
    /// Retransmit everything outstanding every `interval`.
    pub fn new(interval: Duration) -> Self {
        StubbornLink { interval, sent: BTreeMap::new(), armed: false }
    }

    /// How many transmissions are still being retried.
    pub fn outstanding(&self) -> usize {
        self.sent.len()
    }

    /// The retransmission interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

impl<M: Clone> StubbornLink<M> {
    /// Arm the retransmission timer if there is anything to retransmit.
    ///
    /// The book starts a timer at initialisation and runs it forever. Arming lazily is
    /// equivalent in behaviour and leaves no timer running when nothing is outstanding — which
    /// also means a run reaches quiescence instead of ticking indefinitely.
    fn arm(&mut self, cx: &mut ProtoCx<'_, Self>) {
        if !self.armed && !self.sent.is_empty() {
            cx.set_timer(self.interval, Retransmit);
            self.armed = true;
        }
    }
}

impl<M: Clone> Protocol for StubbornLink<M> {
    type Cmd = Cmd<M>;
    type Ind = Ind<M>;
    type Msg = M;
    type Timer = Retransmit;
    /// No scope conditions: this protocol's guarantees do not lapse.
    type Scope = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<M>, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            Cmd::Send { id, to, msg } => {
                cx.send(to, msg.clone());
                self.sent.insert(id, (to, msg));
                self.arm(cx);
            }
            Cmd::Stop { id } => {
                self.sent.remove(&id);
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, msg: M, cx: &mut ProtoCx<'_, Self>) {
        // No creation: what is delivered upward is exactly what arrived, attributed to whoever
        // the network says sent it.
        cx.indicate(Ind::Deliver { from, msg });
    }

    fn on_timer(&mut self, Retransmit: Retransmit, cx: &mut ProtoCx<'_, Self>) {
        for (to, msg) in self.sent.values() {
            cx.send(*to, msg.clone());
        }
        self.armed = false;
        self.arm(cx);
    }
}
