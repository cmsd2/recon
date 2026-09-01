//! The link port: what a layer above the link may depend on, and the whole of what it may.
//!
//! `docs/conditional-guarantees.md` states the seam this project is built around: *layers above the
//! link may depend on its `Cmd` and `Ind` types and nothing else, so a session-aware or logged
//! implementation can be swapped through*. This module is that sentence made checkable. A layer
//! above bounds on [`Link`]; an implementation below satisfies it; neither names the other.
//!
//! # Why the vocabulary is not pinned to one pair of types
//!
//! The obvious port is a bound pinning the types exactly —
//! `Protocol<Cmd = pl::Cmd<P>, Ind = pl::Ind<P>>`. It works for the perfect link and fails for the
//! session link, whose `Ind` has three variants rather than one, and admitting both is the entire
//! point: the four `session_*` broadcast modules exist because a layer written against one pair of
//! types cannot compose over the other.
//!
//! So a link keeps its own `Cmd` and `Ind` — they are its vocabulary, not the port's — and the port
//! supplies the two translations a layer above actually needs: build a send, and recognise a
//! delivery. Nothing else about the link is visible.
//!
//! # Scope boundaries, and why there is no second trait for them
//!
//! A session link reports that a session ended or was established. A perfect link cannot: it has no
//! means of observing either, and `docs/scope-annotated-modules.md` forbids a module declaring a
//! scope it cannot observe (Definition 2a, Corollary 8.1). So [`Link::classify`] returns
//! [`LinkInd::Boundary`] only for a link that can actually see one, and that is the whole of the
//! mechanism.
//!
//! There was a `ScopedLink` marker trait here, so that a layer whose liveness depends on being told
//! about a re-establishment could bound on it, making it a compile error to compose that layer over
//! a perfect link. It was deleted, because once the four `session_*` broadcast modules had
//! collapsed into their base modules — the job it was introduced to do — **nothing bounded on it**.
//! Uniform reliable broadcast, the one layer that should have, could not: its resend is reached
//! from the arm handling the child's indications, which lives in the `Link` impl, so the tighter
//! bound would have fallen on every link including the perfect one.
//!
//! What keeps that resend honest is this module's own guarantee rather than a bound: a boundary is
//! never classified for a link that cannot observe one, so over a perfect link the path is
//! unreachable rather than merely unused. `tests/link_port.rs` pins both halves — that the perfect
//! link's classification never yields a boundary, and that the session link's yields one for each
//! variant that reports it.
//!
//! Reintroduce it when a layer genuinely cannot be written without the bound. Until then it is an
//! abstraction ahead of its consumer, which is what `CLAUDE.md` constraint 4 warns against and what
//! this change's own `design.md` recorded as a risk against this very trait.

use recon_core::{NodeId, Protocol};

/// What a layer above the link may depend on, and the whole of what it may.
///
/// `P` is the payload the layer above sends. A link is free to wrap it — the perfect link adds a
/// message identifier — which is why [`Link::send`] builds the request rather than the layer above
/// constructing one, and why [`Link::classify`] takes the payload back out.
///
/// Satisfying the port is a decision, not an accident of shape. An earlier draft made it a blanket
/// impl over every `Protocol` with the right associated types, which meant a protocol became a link
/// by coincidence; a link now says so. A protocol that has not is rejected when the project is
/// built:
///
/// ```compile_fail
/// # struct NotALink;
/// # #[derive(Debug, Clone, PartialEq, Eq)]
/// # struct Whatever;
/// # impl recon_core::Protocol for NotALink {
/// #     type Cmd = Whatever;
/// #     type Ind = Whatever;
/// #     type Msg = Whatever;
/// #     type Scope = core::convert::Infallible;
/// #     type Meta = core::convert::Infallible;
/// #     type Entry = core::convert::Infallible;
/// #     fn on_cmd(&mut self, _: Whatever, _: &mut recon_core::ProtoCx<'_, Self>) {}
/// #     fn on_msg(&mut self, _: recon_core::NodeId, _: Whatever,
/// #               _: &mut recon_core::ProtoCx<'_, Self>) {}
/// #     fn on_timer(&mut self, _: recon_core::TimerId,
/// #                 _: &mut recon_core::ProtoCx<'_, Self>) {}
/// # }
/// fn requires_a_link<L: recon_protocols::link::Link<u32>>() {}
/// requires_a_link::<NotALink>();
/// ```
pub trait Link<P>: Protocol {
    /// The request that sends `msg` to `to`.
    ///
    /// A constructor rather than a fixed type, because the request is the link's own vocabulary.
    fn send(to: NodeId, msg: P) -> Self::Cmd;

    /// What this indication means to the layer above.
    ///
    /// Total, and deliberately so: a layer above maps its child's indications with one function, so
    /// a link that could report something unclassifiable would leave that layer with a case it
    /// could only drop — and silently absorbing a scope end is this project's cardinal sin.
    fn classify(ind: Self::Ind) -> LinkInd<P>;
}

/// What an indication from any link amounts to, in the port's own vocabulary.
///
/// The layer above matches on this rather than on the link's own indication type, which is how one
/// implementation of a broadcast serves every link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkInd<P> {
    /// A message arrived.
    Deliver { from: NodeId, msg: P },
    /// The scope the link's guarantees hold within changed. Only a link that can observe one
    /// ever reports this.
    Boundary(Boundary),
}

/// A boundary of the scope within which a link's guarantees hold.
///
/// Named here rather than in any one link, because a layer above reacts to the boundary without
/// caring which implementation raised it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    /// The scope with `peer` ended at `epoch`. Anything sent to that peer and not yet delivered may
    /// have been lost, and the link cannot say which.
    Ended { peer: NodeId, epoch: u64 },
    /// A scope with `peer` is in force at `epoch`. This is the moment on which anything that must
    /// be resent can be.
    Established { peer: NodeId, epoch: u64 },
}

/// A link that keeps nothing durable *the layer above has to know about*.
///
/// Every layer in this crate composes over one, because none of them declares a storage vocabulary
/// on their child's behalf. It is a conjunction of bounds rather than a capability — hence the
/// blanket impl, which is the opposite of what [`Link`] does deliberately — and it exists so that
/// the conjunction is written once instead of at every composing layer.
///
/// A logged link keeps a great deal durable and does not satisfy this. Composing a broadcast over
/// one is not possible today for that reason, and the bound is where the limitation lives, so it is
/// the one place to revisit when it is wanted.
pub trait VolatileLink<P>:
    Link<P, Meta = core::convert::Infallible, Entry = core::convert::Infallible>
{
}

impl<P, L> VolatileLink<P> for L where
    L: Link<P, Meta = core::convert::Infallible, Entry = core::convert::Infallible>
{
}
