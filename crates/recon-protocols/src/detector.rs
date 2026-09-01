//! The detector port: what a layer above a failure detector may depend on, and the whole of it.
//!
//! Two detectors exist now — [`crate::perfect_failure_detector`] and
//! [`crate::eventually_perfect_failure_detector`] — and Ω is written against whichever it is given.
//! That is the second consumer `CLAUDE.md` constraint 4 asks for before a port is extracted, and
//! the reason this module did not exist while there was only one.
//!
//! # Why the vocabulary is not pinned to one pair of types
//!
//! The same argument [`crate::link`] makes. Pinning `Ind = pfd::Ind` admits the perfect detector
//! and rejects `◇P`, whose `Ind` has a second variant — and admitting both is the entire point,
//! because a leader detector written against one cannot compose over the other. So a detector keeps
//! its own `Ind`, and the port supplies the one translation a layer above needs: is this a
//! suspicion, or the withdrawal of one?
//!
//! # A detector that never retracts says so by never producing a withdrawal
//!
//! `P` classifies its `Crash` as [`DetectorInd::Suspect`] and never yields
//! [`DetectorInd::Restore`] — not because a flag says so, but because it has no indication that
//! could become one. `docs/scope-annotated-modules.md` forbids a module declaring what it cannot
//! observe, and this is that rule applied to a detector: a layer above handles both arms, and over
//! `P` the second is unreachable rather than merely unused.
//!
//! There is deliberately no `RetractingDetector` marker trait. `link.rs` records at length what
//! happened to `ScopedLink` — introduced for a bound nothing could take, deleted for want of a
//! consumer — and the same would be true here. Ω needs no such bound: it handles a withdrawal if one
//! comes and is correct if none ever does.

use recon_core::{NodeId, Protocol};

/// What a layer above a failure detector may depend on.
///
/// Satisfying the port is a decision, not an accident of shape: a detector says so by implementing
/// this, exactly as a link does. A protocol that has not is rejected when the project is built.
///
/// ```compile_fail
/// # use recon_protocols::detector::Detector;
/// # use recon_core::NodeId;
/// fn needs_a_detector<D: Detector>(_: D) {}
/// // `PerfectLink` is a protocol, and is not a detector.
/// needs_a_detector(recon_protocols::perfect_link::PerfectLink::<u32>::new(
///     NodeId::new(1),
///     core::time::Duration::from_millis(1),
/// ));
/// ```
pub trait Detector: Protocol<Note = crate::Note> {
    /// Read one of this detector's indications in the port's terms.
    ///
    /// Total, as [`crate::link::Link::classify`] is: every indication a detector raises is either a
    /// suspicion or the withdrawal of one, and a detector with something else to say would be
    /// saying it to a layer that cannot hear it.
    fn classify(ind: Self::Ind) -> DetectorInd;
}

/// A detector's indication, in the port's terms.
///
/// The layer above matches on this rather than on the detector's own indication type, which is how
/// one leader detector serves both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorInd {
    /// `node` is suspected of having crashed. A perfect detector means it; an eventually perfect
    /// one may be wrong and may take it back.
    Suspect { node: NodeId },
    /// `node` is no longer suspected. Raised only by a detector that can observe its own mistake.
    Restore { node: NodeId },
}

/// A detector that keeps nothing durable *the layer above has to know about*.
///
/// The conjunction written once rather than at every composing layer, as
/// [`crate::link::VolatileLink`] is. Every detector in this crate satisfies it.
pub trait VolatileDetector:
    Detector<Meta = core::convert::Infallible, Entry = core::convert::Infallible>
{
}

impl<D> VolatileDetector for D where
    D: Detector<Meta = core::convert::Infallible, Entry = core::convert::Infallible>
{
}
