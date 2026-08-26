//! The vocabulary through which a protocol affects the world.

use crate::NodeId;
use core::time::Duration;

/// Everything a protocol is able to do.
///
/// A protocol expresses every outward action as one of these. It does not transmit, deliver,
/// or schedule by any other means — which is what allows the same protocol to run under a
/// simulator and under a real driver without knowing the difference.
///
/// The four type parameters are the protocol's own message, indication, timer, and durable-state
/// types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect<M, I, T, D> {
    /// Transmit `msg` to `to`. Best-effort: the layer below may lose it.
    Send { to: NodeId, msg: M },
    /// Raise an indication to the layer above — the protocol delivering on its guarantee.
    Indicate(I),
    /// Request that `token` be handed back after `after` has elapsed.
    SetTimer { after: Duration, token: T },
    /// Write this protocol's durable state to stable storage, so that it survives a crash.
    ///
    /// The value is the protocol's durable state *in full*, not a delta: recovery hands back the
    /// last one written, and nothing else. A protocol that keeps nothing durably declares an
    /// uninhabited durable type, and then this variant cannot be constructed for it at all.
    ///
    /// Every store emitted during an event is durable before any send emitted after it leaves the
    /// process. A protocol may therefore write a promise down and make it in the same breath.
    Store(D),
}

impl<M, I, T, D> Effect<M, I, T, D> {
    /// Rewrite this effect's parts, for a parent translating a child's effect into its own terms.
    ///
    /// This is the composition primitive: a parent re-wraps rather than re-encodes, so a
    /// message crossing layers accumulates type structure but is never serialised twice.
    pub fn map<M2, I2, T2, D2>(
        self,
        msg: impl FnOnce(M) -> M2,
        ind: impl FnOnce(I) -> I2,
        timer: impl FnOnce(T) -> T2,
        durable: impl FnOnce(D) -> D2,
    ) -> Effect<M2, I2, T2, D2> {
        match self {
            Effect::Send { to, msg: m } => Effect::Send { to, msg: msg(m) },
            Effect::Indicate(i) => Effect::Indicate(ind(i)),
            Effect::SetTimer { after, token } => Effect::SetTimer { after, token: timer(token) },
            Effect::Store(d) => Effect::Store(durable(d)),
        }
    }
}

/// The total function out of an uninhabited type.
///
/// Passed as the durable mapper wherever a child keeps nothing durably, which is every
/// composition in this repository so far. It is the *only* such function that can be written, and
/// that is the point: a child that does keep durable state has no mapper available, because a
/// parent's durable state contains its own fields as well as its child's and no function of the
/// child's state alone can produce it. Such a composition fails to build rather than silently
/// losing one of the two.
pub fn absurd<T>(never: core::convert::Infallible) -> T {
    match never {}
}
