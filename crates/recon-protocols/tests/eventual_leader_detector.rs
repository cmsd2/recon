//! Ω against Module 2.9 — and against the fact that everything above it is only worth testing
//! where it is *wrong*.
//!
//! The last test in this file is the load-bearing one. Ω here is derived from a *perfect* failure
//! detector, which never lies, so an Ω that always agrees would make every safety test of the Paxos
//! stack above it vacuous. That the detector can be made to disagree is checked here, before
//! anything is built on it, rather than discovered later when a suite looks thin.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::eventual_leader_detector::{EventualLeaderDetector, Ind};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

const BOUND: Duration = Duration::from_millis(20);

fn heartbeat() -> Duration {
    BOUND * 2
}
fn timeout() -> Duration {
    heartbeat() * 3
}

fn omega(me: NodeId) -> EventualLeaderDetector {
    EventualLeaderDetector::new(me, ALL, heartbeat(), timeout())
}

/// A synchronous run — the assumption the detector beneath rests on, and therefore the one under
/// which this module is accurate.
fn sync_sim(seed: u64) -> Sim<EventualLeaderDetector> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, omega)
}

/// Every leader `node` has trusted, in order.
fn trusted_by(s: &Sim<EventualLeaderDetector>, node: NodeId) -> Vec<NodeId> {
    s.trace().indications_at(node).map(|Ind::Trust { leader }| *leader).collect()
}

/// What each process trusts at the end of the run.
fn final_leaders(s: &Sim<EventualLeaderDetector>) -> Vec<Option<NodeId>> {
    ALL.iter().map(|n| trusted_by(s, *n).last().copied()).collect()
}

// ------------------------------------------------- Determinism: task 2.2

#[test]
fn the_same_suspicions_give_the_same_leader() {
    // `maxrank(Π \ suspected)` is a function of the suspected set and of nothing else, which is
    // what lets two processes agree without exchanging a word about leadership.
    let mut s = sync_sim(1);
    s.run_for(timeout() * 2);

    let leaders = final_leaders(&s);
    assert!(
        leaders.iter().all(|l| *l == leaders[0]),
        "nobody is suspected, so every process must trust the same: {leaders:?}"
    );
    assert_eq!(leaders[0], Some(D), "and `maxrank` of the whole membership is the greatest id");
}

#[test]
fn an_unchanged_suspected_set_raises_nothing_further() {
    // A `Trust` costs the layer above an epoch, and an epoch costs an abort. Repeating an answer
    // that has not changed is pure loss.
    let mut s = sync_sim(2);
    s.run_for(timeout() * 6);

    for n in ALL {
        assert_eq!(
            trusted_by(&s, n).len(),
            1,
            "{n} raised Trust more than once while nothing was suspected: {:?}",
            trusted_by(&s, n)
        );
    }
}

// ------------------------------------------------- Eventual accuracy: task 2.3

#[test]
fn a_single_leader_emerges() {
    let mut s = sync_sim(3);
    s.run_for(timeout() * 2);

    let leaders = final_leaders(&s);
    assert!(leaders.iter().all(|l| *l == Some(D)), "{leaders:?}");
}

#[test]
fn a_crashed_leader_is_replaced_by_a_correct_one() {
    let mut s = sync_sim(4);
    s.run_for(timeout() * 2);
    assert_eq!(final_leaders(&s)[0], Some(D), "D leads first, being the greatest id");

    s.crash(D);
    s.run_for(timeout() * 4);

    for n in [A, B, C] {
        assert_eq!(
            trusted_by(&s, n).last().copied(),
            Some(C),
            "{n} must move to the next greatest that is still correct"
        );
    }
}

#[test]
fn leadership_walks_down_the_membership_as_processes_crash() {
    // Non-vacuity for the test above: one replacement could be luck. This shows the rule.
    let mut s = sync_sim(5);
    s.run_for(timeout() * 2);
    s.crash(D);
    s.run_for(timeout() * 4);
    s.crash(C);
    s.run_for(timeout() * 4);

    assert_eq!(trusted_by(&s, A), vec![D, C, B], "A followed the rank downward as each crashed");
}

// ------------------------------- That it can be wrong at all: task 2.4

#[test]
fn the_detector_can_disagree_when_the_timing_assumption_is_withdrawn() {
    // **The load-bearing test of this file.**
    //
    // Ω here is derived from a perfect failure detector, which never accuses a correct process — so
    // over a synchronous network this module is not merely eventually accurate but accurate from
    // the start, and every safety property of the Paxos stack above it would be untestable.
    //
    // Withdrawing the synchrony assumption is what makes it lie, exactly as
    // `perfect_failure_detector`'s own accuracy test does. If this ever stops finding a
    // disagreement, every "agreement holds under a lying detector" test above it has quietly become
    // a test of a run where nobody disagreed.
    let disagreed = (0..40u64).any(|seed| {
        let mut s: Sim<EventualLeaderDetector> = Sim::new(
            Config::default()
                .seed(seed)
                .loss(0.6)
                .latency(Duration::from_millis(1), Duration::from_millis(30)),
            &ALL,
            omega,
        );
        s.run_for(timeout() * 6);
        let leaders = final_leaders(&s);
        leaders.iter().any(|l| *l != leaders[0])
    });

    assert!(
        disagreed,
        "on a lossy asynchronous network two processes must be able to trust different leaders — \
         if they cannot, this Ω is accurate rather than eventually accurate, and nothing built on \
         it can be tested for the property it exists to have"
    );
}

#[test]
fn a_correct_process_can_be_abandoned_while_the_assumption_is_withdrawn() {
    // The other half of being wrong: not only disagreement between processes, but trusting away
    // from a process that has not in fact crashed.
    let abandoned = (0..40u64).any(|seed| {
        let mut s: Sim<EventualLeaderDetector> = Sim::new(
            Config::default()
                .seed(seed)
                .loss(0.6)
                .latency(Duration::from_millis(1), Duration::from_millis(30)),
            &ALL,
            omega,
        );
        s.run_for(timeout() * 6);
        // D never crashes, so any process that has moved off it has been wrong.
        ALL.iter().any(|n| trusted_by(&s, *n).last().copied().is_some_and(|l| l != D))
    });

    assert!(abandoned, "a correct process must be abandonable, or the detector cannot be wrong");
}
