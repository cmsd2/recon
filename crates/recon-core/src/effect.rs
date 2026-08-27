//! The vocabulary through which a protocol affects the world.

use crate::NodeId;
use core::time::Duration;

/// Everything a protocol is able to do.
///
/// A protocol expresses every outward action as one of these. It does not transmit, deliver,
/// or schedule by any other means — which is what allows the same protocol to run under a
/// simulator and under a real driver without knowing the difference.
///
/// The two type parameters are the protocol's own message and indication types. A timer needs
/// none: it is named by an opaque [`TimerId`], and the driver hands the expiry back to whoever
/// registered it rather than deducing the owner from the token's type.
///
/// Storage is not here: an effect is deferred, and a write must be durable before it returns.
/// See [`crate::store`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect<M, I> {
    /// Transmit `msg` to `to`. Best-effort: the layer below may lose it.
    Send { to: NodeId, msg: M },
    /// Raise an indication to the layer above — the protocol delivering on its guarantee.
    Indicate(I),
    /// Request that `id` be handed back after `after` has elapsed.
    SetTimer { after: Duration, id: TimerId },
}

/// Names one registered timer.
///
/// Opaque, and the same type for every protocol, so a timer's identity says nothing about which
/// layer registered it or where that layer sits in a composition. The alternative — a timer type
/// per protocol, re-wrapped by each parent — makes the *type* encode the composition path, so
/// inserting a layer rewraps every timer beneath it and a layer's timer vocabulary becomes visible
/// to everything above it.
///
/// Holding one also lets a layer recognise an expiry it has superseded, and would let one be
/// cancelled. A `bool` can express neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerId(pub u64);

/// Which kind of write happened, for the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    /// The metadata value was replaced.
    Set,
    /// An entry was appended.
    Append,
}

impl<M, I> Effect<M, I> {
    /// Rewrite this effect's parts, for a parent translating a child's effect into its own terms.
    ///
    /// This is the composition primitive: a parent re-wraps rather than re-encodes, so a
    /// message crossing layers accumulates type structure but is never serialised twice. A timer
    /// passes through untouched, having nothing in it that belongs to one layer.
    pub fn map<M2, I2>(
        self,
        msg: impl FnOnce(M) -> M2,
        ind: impl FnOnce(I) -> I2,
    ) -> Effect<M2, I2> {
        match self {
            Effect::Send { to, msg: m } => Effect::Send { to, msg: msg(m) },
            Effect::Indicate(i) => Effect::Indicate(ind(i)),
            Effect::SetTimer { after, id } => Effect::SetTimer { after, id },
        }
    }
}
