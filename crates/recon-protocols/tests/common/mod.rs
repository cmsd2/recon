//! What the leader-driven family's suites share: five processes, one delivery bound, and the
//! timing every module is configured with.
//!
//! `detect_after` is three heartbeats, and a heartbeat is two bounds, so under `synchronous(BOUND)`
//! the detector is accurate; a noisy configuration withdraws that.

#![allow(dead_code)]

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::Timing;

pub const A: NodeId = NodeId::new(1);
pub const B: NodeId = NodeId::new(2);
pub const C: NodeId = NodeId::new(3);
pub const D: NodeId = NodeId::new(4);
pub const E: NodeId = NodeId::new(5);
pub const ALL: [NodeId; 5] = [A, B, C, D, E];

/// The simulator's delivery bound in synchronous mode.
pub const BOUND: Duration = Duration::from_millis(20);

pub fn retransmit() -> Duration {
    Duration::from_millis(10)
}
pub fn heartbeat() -> Duration {
    BOUND * 2
}
pub fn timeout() -> Duration {
    heartbeat() * 3
}
pub fn timing() -> Timing {
    Timing { retransmit: retransmit(), heartbeat: heartbeat(), detect_after: timeout() }
}

/// Assert that what a run sends per window of time does not grow with how long it has been running.
///
/// Run the work first, then call this: it measures `windows` successive windows of `window` and
/// requires the last to be no more than a tenth above the first. A stack that answers every
/// redelivery with a fresh stubborn transmission fails this within three windows — that is the
/// defect it was written to catch — while a stack whose stubborn children retransmit a *fixed* set
/// passes, because a fixed set is a flat rate.
///
/// A macro rather than a function because `Sim<P>`'s methods carry bounds each suite satisfies
/// differently.
macro_rules! assert_send_rate_flat {
    ($sim:expr, $window:expr, $windows:expr) => {{
        let mut counts: Vec<usize> = Vec::new();
        let mut prev = $sim.trace().send_count();
        for _ in 0..$windows {
            $sim.run_for($window);
            let now = $sim.trace().send_count();
            counts.push(now - prev);
            prev = now;
        }
        let first = counts[0];
        let last = *counts.last().expect("at least one window");
        assert!(first > 0, "nothing was sent, so there is no rate to be flat");
        assert!(
            last <= first + first / 10,
            "the send rate grew with time — work is bounded by how long the run has been going, \
             not by membership: sends per window {counts:?}"
        );
    }};
}
pub(crate) use assert_send_rate_flat;
