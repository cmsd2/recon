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
//! # Scope reporting is a second trait, not a variant nobody raises
//!
//! A session link reports that a session ended or was established. A perfect link cannot: it has no
//! means of observing either, and `docs/scope-annotated-modules.md` forbids a module declaring a
//! scope it cannot observe (Definition 2a, Corollary 8.1). So the boundary vocabulary lives in
//! [`ScopedLink`], which the session link implements and the perfect link does not.
//!
//! A layer indifferent to boundaries bounds on [`Link`] and composes over both. A layer whose
//! liveness depends on being told about a re-establishment bounds on [`ScopedLink`], and composing
//! it over a link that cannot report is then a compile error rather than a protocol that waits for
//! ever. That is `docs/conditional-guarantees.md`'s *a layer that cannot bridge must propagate*,
//! stated in the type system rather than in prose.

use recon_core::{NodeId, Protocol};

/// What a layer above the link may depend on, and the whole of what it may.
///
/// `P` is the payload the layer above sends. A link is free to wrap it — the perfect link adds a
/// message identifier — which is why [`Link::send`] builds the request rather than the layer above
/// constructing one, and why [`Link::delivered`] takes the payload back out.
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
    /// The scope the link's guarantees hold within changed. Only a [`ScopedLink`] ever reports one.
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

/// A link that reports the boundaries of the scope its guarantees hold within.
///
/// Implementing this is a claim that [`Link::classify`] can return [`LinkInd::Boundary`] — that the
/// link can actually observe a boundary. The session link implements it; the perfect link does not,
/// because it has no means of observing one. A layer that repairs a scope ending requires a link
/// that raises one, and says so by bounding here rather than on [`Link`].
///
/// It carries no methods of its own. The classification is total on [`Link`] because the layer
/// above needs one function over every link, so what this trait adds is the claim, not a
/// capability. That the claim is honest is checked by test rather than by the compiler: the
/// perfect link's classification never returns a boundary, and the session link's returns one for
/// each variant that reports it. See `tests/link_port.rs`.
///
/// What the compiler does enforce is the bound. A layer that repairs a scope ending cannot be
/// composed over a link that never reports one — which is
/// `docs/conditional-guarantees.md`'s *a layer that cannot bridge must propagate*, enforced rather
/// than documented. The alternative is a stack that compiles and then waits for ever for a
/// re-establishment nobody will announce:
///
/// ```compile_fail
/// use recon_protocols::link::ScopedLink;
/// use recon_protocols::perfect_link::PerfectLink;
/// fn requires_scoped<P, L: ScopedLink<P>>() {}
/// requires_scoped::<u32, PerfectLink<u32>>();
/// ```
pub trait ScopedLink<P>: Link<P> {}
