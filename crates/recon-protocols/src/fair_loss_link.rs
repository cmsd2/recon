//! Fair-loss point-to-point links — the bottom of the stack, and the weakest link there is.
//!
//! **Status: implementation. Space: none.** It keeps nothing at all, which is what makes it
//! trivially bounded and what makes everything above it responsible for its own redundancy.
//!
//! Cachin, Guerraoui & Rodrigues, Module 2.1 ("FairLossPointToPointLinks"):
//!
//! ```text
//! FLL1  Fair-loss: If a correct process p infinitely often sends a message m to a correct
//!       process q, then q delivers m an infinite number of times.
//! FLL2  Finite duplication: If a correct process p sends a message m a finite number of times
//!       to q, then m cannot be delivered an infinite number of times by q.
//! FLL3  No creation: If some process q delivers a message m with sender p, then m was
//!       previously sent to q by process p.
//! ```
//!
//! # Why this module is nearly empty
//!
//! `recon-sim` **is** the fair-loss network — that is `CLAUDE.md`'s description of it and the whole
//! of constraint 3. Loss, duplication and reordering are its knobs. So a fair-loss link in this
//! codebase has nothing to add: it hands a send to the network and reports what arrives. FLL1
//! through FLL3 are the simulator's properties, and this module's job is to be the port through
//! which a layer above reaches them without naming the simulator.
//!
//! That is not a degenerate case, it is the point. Every other link here is defined by what it adds
//! on top of this: the stubborn link adds retransmission, the perfect link adds deduplication over
//! that, the session link adds ordering within a scope and honesty across one.
//!
//! # What it is for
//!
//! Algorithm 3.9 — eager probabilistic broadcast — says `Uses: FairLossPointToPointLinks`, and it
//! means it. Gossip exists to tolerate loss; running it over a perfect link, which retransmits until
//! delivery, masks the behaviour it is built to provide and leaves its probabilistic guarantee
//! unobservable. It also never falls silent, because the stubborn link beneath the perfect one
//! re-sends everything it has ever sent on every tick, so "a broadcast generates finitely many
//! transmissions" cannot be measured there either.
//!
//! Before this module existed, the only thing in the tree with this shape was a link written for a
//! test — the stand-in for an application's own transport in `tests/foreign_link.rs`. That it was
//! needed twice, once as a demonstration and once as the bottom of the book's own stack, is what
//! made it a module.

use recon_core::{NodeId, ProtoCx, Protocol, TimerId};

use crate::link::{Link, LinkInd};

/// Requests from the layer above.
///
/// Deliberately the same shape as every other link's: one variant, send this to that peer. A layer
/// above reaches it through [`Link::send`] and never names this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd<P> {
    Send { to: NodeId, msg: P },
}

/// Indications to the layer above.
///
/// One variant, and no scope boundary among them: this link has no session to end and no
/// incarnation it can observe, so it declares none. `docs/scope-annotated-modules.md` forbids a
/// module naming a scope it cannot observe, and this one can observe nothing at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ind<P> {
    Deliver { from: NodeId, msg: P },
}

/// The link the book's stack starts from: send it, and hope.
///
/// No retransmission, no deduplication, no timer, no state. What arrives, arrives.
#[derive(Debug, Default)]
pub struct FairLossLink<P>(core::marker::PhantomData<fn() -> P>);

impl<P> FairLossLink<P> {
    pub fn new() -> Self {
        FairLossLink(core::marker::PhantomData)
    }
}

impl<P: Clone> Protocol for FairLossLink<P> {
    type Cmd = Cmd<P>;
    type Ind = Ind<P>;
    type Msg = P;
    /// No scope conditions. FLL1 to FLL3 are stated over correct processes and hold as long as one
    /// is correct; there is no session to end and nothing this link could report about one.
    type Scope = core::convert::Infallible;
    type Note = crate::Note;
    /// Keeps nothing durably, because it keeps nothing at all.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Send { to, msg }: Cmd<P>, cx: &mut ProtoCx<'_, Self>) {
        cx.send(to, msg);
    }

    fn on_msg(&mut self, from: NodeId, msg: P, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(Ind::Deliver { from, msg });
    }

    fn on_timer(&mut self, _id: TimerId, _cx: &mut ProtoCx<'_, Self>) {
        // Registers none, and has no child to pass one to.
    }
}

/// The fair-loss link satisfies the link port, and only its unscoped half.
///
/// It does not report scope boundaries because it cannot observe any — it holds no session, no
/// epoch and no state whatever. A layer that repairs a scope ending gets nothing from this link,
/// which is the honest outcome rather than a boundary invented to satisfy a bound.
impl<P: Clone> Link<P> for FairLossLink<P> {
    fn send(to: NodeId, msg: P) -> Cmd<P> {
        Cmd::Send { to, msg }
    }

    fn classify(Ind::Deliver { from, msg }: Ind<P>) -> LinkInd<P> {
        LinkInd::Deliver { from, msg }
    }
}
