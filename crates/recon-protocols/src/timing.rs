//! The durations the leader-driven family is configured with.
//!
//! Four modules took these as three positional `Duration`s, and nothing but the caller's care kept
//! them in order. Named fields make a swap a compile error.

use core::time::Duration;

/// How often to retransmit, how often to heartbeat, and how long a silence is an accusation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// The stubborn links' retransmission interval.
    pub retransmit: Duration,
    /// The failure detector's heartbeat interval.
    pub heartbeat: Duration,
    /// How long without a heartbeat before a process is suspected. The detector's synchrony
    /// assumption is that a message arrives within this, so under simulation it should exceed the
    /// configured delivery bound by a margin.
    pub detect_after: Duration,
}
