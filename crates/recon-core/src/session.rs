//! The end of a session, as a protocol hears about it.

use crate::NodeId;

/// A session a protocol was relying on has ended, and a new one has begun.
///
/// This is a domain concept, not a simulator one: any driver — a simulated network, or an adapter
/// over TCP or QUIC — reports the same thing, because it is what a real endpoint learns. It names
/// the peer and the epoch that begins, and deliberately says nothing about what was lost. A real
/// endpoint cannot know how much of its last write arrived, and a type that reported it exactly
/// would permit protocols that cannot exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionEnded {
    pub peer: NodeId,
    /// The epoch now beginning. Increases on every re-establishment, so a new session is
    /// distinguishable from the one it replaces.
    pub epoch: u64,
}
