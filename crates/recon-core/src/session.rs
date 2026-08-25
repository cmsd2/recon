//! What a protocol learns about the sessions carrying its messages.

use crate::NodeId;

/// Something happened to the session with a peer.
///
/// These are two distinct events and neither substitutes for the other, because a real endpoint
/// learns them separately and can act on only one.
///
/// An **ending** is synchronous and knowable: the operating system closes the handle and the next
/// read or write fails, so a protocol learns at the moment of failure that its last writes may be
/// gone. It cannot act on that, the peer being unreachable.
///
/// An **establishment** is what can be acted on. It happens when the link manages to reconnect —
/// a deployed link keeps trying on its own — so it arrives at a moment the layers above neither
/// choose nor control.
///
/// This is a domain concept, not a simulator one: a simulated network and an adapter over TCP or
/// QUIC report the same thing, because it is what a real endpoint learns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionEvent {
    /// The session with `peer` has ended. An unknown suffix of what was in flight may be lost.
    ///
    /// `epoch` is the one that **ended**, not a prediction of the next: at the moment of failure
    /// the next epoch is not a fact, and may never become one.
    Ended { peer: NodeId, epoch: u64 },
    /// A session with `peer` is in force at `epoch`. The peer can be reached again.
    ///
    /// This is the event on which anything must be resent, and the only one on which a resend
    /// can succeed.
    Established { peer: NodeId, epoch: u64 },
}

impl SessionEvent {
    pub fn peer(&self) -> NodeId {
        match self {
            SessionEvent::Ended { peer, .. } | SessionEvent::Established { peer, .. } => *peer,
        }
    }

    pub fn epoch(&self) -> u64 {
        match self {
            SessionEvent::Ended { epoch, .. } | SessionEvent::Established { epoch, .. } => *epoch,
        }
    }
}
