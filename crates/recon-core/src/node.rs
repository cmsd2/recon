//! Process identity.

use core::fmt;

/// Identifies a process in the system.
///
/// Copy and totally ordered, so collections keyed by it iterate deterministically and
/// membership sets can be enumerated in a stable order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

impl NodeId {
    pub const fn new(n: u64) -> Self {
        NodeId(n)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}
