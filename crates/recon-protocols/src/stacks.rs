//! Ready-made compositions: a layer, spelled with the link it runs over.
//!
//! Every protocol here takes its link as a type parameter, which is what removed the four forked
//! `session_*` broadcast modules. The cost is that naming a stack means naming both halves, and the
//! payload the link carries is the layer's business rather than the caller's:
//!
//! ```text
//! ReliableBroadcast<u32, SessionLink<reliable_broadcast::Data<u32>>>
//! ```
//!
//! `Data` is an implementation detail of Algorithm 3.3 — the originator and sequence number it
//! deduplicates on — and a caller who wants "reliable broadcast over a session link, carrying
//! `u32`" should not have to know it exists. Each layer names it as `Carried<P>`, and the aliases
//! below use that to spell the whole stack in one type parameter.
//!
//! # Why these live here and not beside either half
//!
//! Putting `OverSessions` in `reliable_broadcast` would make that module name a particular link,
//! which is exactly the dependency the port was built to remove — a layer states its requirement as
//! `Link` and names no implementation. Putting it in `session_link` would invert the same problem.
//! A composition belongs to neither of the things it composes, so it lives in its own module.
//!
//! Only the session stacks are named, because they are the ones that existed as forked modules and
//! so have call sites wanting them. A stack over an application's own link is spelled at its own
//! call site with its own alias; there is nothing for this crate to name.

use crate::best_effort_broadcast::BestEffortBroadcast;
use crate::eventual_leader_detector::EventualLeaderDetector;
use crate::flooding_consensus::{self as fc, FloodingConsensus};
use crate::lazy_probabilistic_broadcast::{self as lpb, LazyProbabilisticBroadcast};
use crate::majority_ack_uniform_reliable_broadcast::{
    self as maurb, MajorityAckUniformReliableBroadcast,
};
use crate::perfect_failure_detector::PerfectFailureDetector;
use crate::probabilistic_broadcast::{self as pb, ProbabilisticBroadcast};
use crate::reliable_broadcast::{self as rb, ReliableBroadcast};
use crate::session_link::SessionLink;
use crate::uniform_reliable_broadcast::{self as urb, UniformReliableBroadcast};

/// Best-effort broadcast over a session link — what `session_best_effort_broadcast` was.
///
/// Validity holds while the sessions carrying a broadcast hold. This layer keeps nothing it could
/// resend from, so it reports a boundary upward rather than repairing one.
pub type BestEffortBroadcastOverSessions<P> = BestEffortBroadcast<P, SessionLink<P>>;

/// Eager reliable broadcast over a session link — what `session_reliable_broadcast` was.
///
/// `RB4` is scoped to the sessions that carried the relay: this layer relays once and keeps
/// identifiers rather than payloads, so a relay lost to an ending is lost for good.
pub type ReliableBroadcastOverSessions<P> = ReliableBroadcast<P, SessionLink<rb::Carried<P>>>;

/// Uniform reliable broadcast over a session link — what `session_uniform_reliable_broadcast` was.
///
/// Bridges an ending rather than propagating it: `pending` outlives the scope, so a
/// re-establishment prompts a directed resend.
pub type UniformReliableBroadcastOverSessions<P> =
    UniformReliableBroadcast<P, SessionLink<urb::Carried<P>>>;

/// Majority-ack uniform reliable broadcast over a session link — what
/// `session_majority_ack_uniform_reliable_broadcast` was.
///
/// Bridges an ending the same way, and without a failure detector, so no timing assumption comes
/// with it.
pub type MajorityAckUniformReliableBroadcastOverSessions<P> =
    MajorityAckUniformReliableBroadcast<P, SessionLink<maurb::Carried<P>>>;

/// Flooding consensus over a session link.
///
/// Named for completeness rather than because a fork of it existed. Its failure detector's
/// synchrony assumption is unaffected by the link beneath.
pub type FloodingConsensusOverSessions<P> = FloodingConsensus<P, SessionLink<fc::Carried<P>>>;

/// Eager gossip over a session link — the real-world set's form of Algorithm 3.9.
///
/// Within a session nothing is lost, so the only loss is a session ending, which this layer
/// propagates: it keeps identifiers rather than payloads and has nothing to resend. `PB1` stays
/// probabilistic because `picktargets` is still random.
pub type ProbabilisticBroadcastOverSessions<P> =
    ProbabilisticBroadcast<P, SessionLink<pb::Carried<P>>>;

/// Lazy gossip over session links — both halves, the gossip and the recovery, over one session per
/// peer pair.
///
/// Bridges a session ending the way the algorithm bridges any loss: the next message from that
/// sender exposes the gap, and a request pulls what the ending dropped from whoever stored it.
pub type LazyProbabilisticBroadcastOverSessions<P> = LazyProbabilisticBroadcast<
    P,
    SessionLink<lpb::Recovery<P>>,
    SessionLink<pb::Carried<lpb::Data<P>>>,
>;

/// Ω over the **perfect** failure detector — what this module was before `◇P` existed.
///
/// Strictly stronger than Algorithm 2.8 asks for, and therefore correct: a detector that is right
/// from the start satisfies "eventually right". What it costs is that a suspicion is permanent, so
/// leadership only ever walks *downward* through the membership and a process that crashed and
/// recovered can never lead again. In the crash-stop model nothing recovers and that costs nothing;
/// in the fail-recovery model it is the difference between a stack that can regain a leader and one
/// that cannot.
///
/// Use it where the delivery bound really is known and permanence is wanted. Otherwise use
/// [`EventualLeaderDetector`], which defaults to the detector its algorithm names.
pub type EventualLeaderDetectorOverPerfectDetection =
    EventualLeaderDetector<PerfectFailureDetector>;

impl EventualLeaderDetectorOverPerfectDetection {
    /// Ω over a perfect failure detector beating every `heartbeat` and accusing after
    /// `detect_after` of silence.
    pub fn over_perfect_detection(
        me: recon_core::NodeId,
        peers: impl IntoIterator<Item = recon_core::NodeId>,
        heartbeat: core::time::Duration,
        detect_after: core::time::Duration,
    ) -> Self {
        EventualLeaderDetector::with_detector(me, peers, |me, all| {
            PerfectFailureDetector::new(me, all, heartbeat, detect_after)
        })
    }
}
