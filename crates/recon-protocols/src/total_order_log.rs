//! The total-order log port: what a layer above a totally ordered log may depend on, and the whole
//! of what it may.
//!
//! Built on the model of [`crate::link`] and [`crate::detector`], and for the same reason. An
//! implementation keeps its own `Cmd` and `Ind` — that vocabulary is the algorithm's, not the
//! port's — and the port supplies the three translations a layer above actually needs: build an
//! append, build a read, and classify an indication. Pinning the port to one pair of types would
//! admit exactly one implementation, which is the failure `link.rs` records: four `session_*`
//! broadcast modules existed because a layer written against one link's vocabulary could not
//! compose over another's.
//!
//! One suite is written against this port and every implementation behind it is held to it, so that
//! where two implementations differ is visible rather than asserted. Here the pair is crash-stop
//! against fail-recovery, and what differs is exactly one thing: whether the ordered sequence
//! survives a restart.
//!
//! # The read is a departure, and this is where it is recorded
//!
//! The book's abstraction is *total-order broadcast* — `⟨ tob, Broadcast | m ⟩` and
//! `⟨ tob, Deliver | p, m ⟩`, with no read at all. Both algorithms behind this port nonetheless
//! maintain `delivered`, the totally ordered sequence, and a log's clients read it. So
//! [`TotalOrderLog::read`] exposes what the page keeps and does not offer: a departure of one
//! method rather than of the algorithm.
//!
//! **A read is served from the reading process's own copy.** That is all either algorithm can
//! honestly do — a read observing every completed append would have to go through consensus or hold
//! a lease, which is not on the page and would change what is being transcribed. So the claim is a
//! **total order**, not linearizability: a process whose round has not yet decided has not yet
//! extended its sequence, and its read says so rather than waiting. What two reads anywhere in a run
//! do guarantee is that one result is a prefix of the other, because both are prefixes of one agreed
//! sequence.

use recon_core::{NodeId, Position, Protocol};

/// What a layer above a totally ordered log may depend on, and the whole of what it may.
///
/// `V` is the value a client appends. An implementation is free to wrap it — both of the ones here
/// carry the originator alongside — which is why [`TotalOrderLog::append`] builds the request rather
/// than the caller constructing one, and why [`TotalOrderLog::classify`] takes the value back out.
///
/// Satisfying the port is a decision, not an accident of shape. `link.rs` records an earlier draft
/// making its port a blanket impl over every protocol with the right associated types, which meant a
/// protocol became a link by coincidence; a log says so. One that has not is rejected when the
/// project is built:
///
/// ```compile_fail
/// # struct NotALog;
/// # #[derive(Debug, Clone, PartialEq, Eq)]
/// # struct Whatever;
/// # impl recon_core::Protocol for NotALog {
/// #     type Cmd = Whatever;
/// #     type Ind = Whatever;
/// #     type Msg = Whatever;
/// #     type Scope = core::convert::Infallible;
/// #     type Note = recon_protocols::Note;
/// #     type Meta = core::convert::Infallible;
/// #     type Entry = core::convert::Infallible;
/// #     fn on_cmd(&mut self, _: Whatever, _: &mut recon_core::ProtoCx<'_, Self>) {}
/// #     fn on_msg(&mut self, _: recon_core::NodeId, _: Whatever,
/// #               _: &mut recon_core::ProtoCx<'_, Self>) {}
/// #     fn on_timer(&mut self, _: recon_core::TimerId,
/// #                 _: &mut recon_core::ProtoCx<'_, Self>) {}
/// # }
/// fn requires_a_log<L: recon_protocols::total_order_log::TotalOrderLog<u32>>() {}
/// requires_a_log::<NotALog>();
/// ```
pub trait TotalOrderLog<V>: Protocol<Note = crate::Note> {
    /// The request that appends `value` to the log.
    ///
    /// A constructor rather than a fixed type, because the request is the implementation's own
    /// vocabulary.
    fn append(value: V) -> Self::Cmd;

    /// The request that reads the ordered sequence from `from` onwards.
    ///
    /// The departure this module's header records. Served locally, so it may lag an append that has
    /// completed elsewhere.
    fn read(from: Position) -> Self::Cmd;

    /// What this indication means to the layer above.
    ///
    /// Total, as [`crate::link::Link::classify`] and [`crate::detector::Detector::classify`] are: a
    /// layer above maps its child's indications with one function, so an implementation that could
    /// report something unclassifiable would leave that layer with a case it could only drop — and
    /// silently absorbing something is this project's cardinal sin.
    fn classify(ind: Self::Ind) -> LogInd<V>;
}

/// What an indication from any totally ordered log amounts to, in the port's own vocabulary.
///
/// A layer above matches on this rather than on the implementation's own indication type, which is
/// how one suite serves every implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogInd<V> {
    /// An entry took its place in the agreed sequence, at `position`, having been appended by
    /// `from`.
    ///
    /// `from` is the process that *appended* it, which the page carries as `⟨ tob, Deliver | s, m ⟩`
    /// and which a checker needs in order to say whose operation completed.
    Ordered { position: Position, from: NodeId, value: V },
    /// The answer to a read: the entries at `from` and later, in order.
    Contents { from: Position, entries: Vec<V> },
    /// A scope that part of this log's guarantee held within has changed.
    ///
    /// Reachable only over a link that can observe one. A log composing a reliable broadcast
    /// inherits that broadcast's inability to bridge an ending — it holds no redundancy outliving
    /// the scope beyond what consensus gives it — so it propagates rather than absorbing.
    Boundary(crate::link::Boundary),
}
