//! The vocabulary through which a protocol affects the world.

use crate::NodeId;
use core::time::Duration;

/// Everything a protocol is able to do.
///
/// A protocol expresses every outward action as one of these. It does not transmit, deliver,
/// or schedule by any other means — which is what allows the same protocol to run under a
/// simulator and under a real driver without knowing the difference.
///
/// The three type parameters are the protocol's own message, indication, and timer types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect<M, I, T> {
    /// Transmit `msg` to `to`. Best-effort: the layer below may lose it.
    Send { to: NodeId, msg: M },
    /// Raise an indication to the layer above — the protocol delivering on its guarantee.
    Indicate(I),
    /// Request that `token` be handed back after `after` has elapsed.
    SetTimer { after: Duration, token: T },
}

impl<M, I, T> Effect<M, I, T> {
    /// Rewrite this effect's parts, for a parent translating a child's effect into its own terms.
    ///
    /// This is the composition primitive: a parent re-wraps rather than re-encodes, so a
    /// message crossing layers accumulates type structure but is never serialised twice.
    pub fn map<M2, I2, T2>(
        self,
        msg: impl FnOnce(M) -> M2,
        ind: impl FnOnce(I) -> I2,
        timer: impl FnOnce(T) -> T2,
    ) -> Effect<M2, I2, T2> {
        match self {
            Effect::Send { to, msg: m } => Effect::Send { to, msg: msg(m) },
            Effect::Indicate(i) => Effect::Indicate(ind(i)),
            Effect::SetTimer { after, token } => Effect::SetTimer { after, token: timer(token) },
        }
    }
}
