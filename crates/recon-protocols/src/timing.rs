//! The durations the leader-driven family is configured with.
//!
//! Four modules took these as three positional `Duration`s, and nothing but the caller's care kept
//! them in order. Named fields make a swap a compile error.

use core::time::Duration;

/// How often to retransmit, how often to heartbeat, and how long a silence is an accusation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How often a layer sweeps its outstanding work.
    ///
    /// The stubborn links' retransmission interval, and for them it is the rate as well as the
    /// granularity. It is **not** the rate everywhere: `multi_paxos_synod` runs its sweep at this
    /// interval and decides separately, against the delivery bound, whether enough time has passed
    /// for a resend to be worth making. Conflating the two had it resending inside one round trip,
    /// because every suite here sets this below the bound.
    pub retransmit: Duration,
    /// The failure detector's heartbeat interval.
    pub heartbeat: Duration,
    /// How long without a heartbeat before a process is suspected. The detector's synchrony
    /// assumption is that a message arrives within this, so under simulation it should exceed the
    /// configured delivery bound by a margin.
    pub detect_after: Duration,
}
