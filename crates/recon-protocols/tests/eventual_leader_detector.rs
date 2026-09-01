//! Ω against Module 2.9 — and against the fact that everything above it is only worth testing
//! where it is *wrong*.
//!
//! Two tests here are load-bearing. That the detector can be made to **disagree** is checked before
//! anything is built on it, because an Ω that always agrees would make every safety test of the
//! Paxos stack vacuous. And that trust **returns** to a process that was suspected is checked
//! because it is what `◇P` buys over `P` and what the fail-recovery stack above needs: under a
//! detector that never retracts, leadership only ever walks downward and a recovered process can
//! never lead again.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::eventual_leader_detector::{EventualLeaderDetector, Ind};
use recon_sim::{Config, Sim};

mod common;
use common::*;

const ALL: [NodeId; 4] = [A, B, C, D];

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

// ------------------------------------------------- bounded by membership, not by time

#[test]
fn the_send_rate_does_not_grow_with_time() {
    // The module claims a membership bound. Heartbeats are the only traffic, and a heartbeat
    // schedule is a function of the membership and the interval — so the rate must be flat.
    let mut s = sync_sim(20);
    s.run_for(timeout() * 2);
    assert_send_rate_flat!(s, timeout() * 2, 4);
}

// ------------------------------------------------- trust returns: tasks 3.2 and 3.3

#[test]
fn a_recovered_process_can_lead_again() {
    // The reason for `◇P`. Under a detector that never retracts, D is suspected the moment it
    // crashes and stays suspected for the rest of the run, so leadership walks down to C and can
    // never come back — however long D runs afterwards.
    let mut s = sync_sim(20);
    s.run_for(timeout() * 2);
    assert_eq!(trusted_by(&s, A).last().copied(), Some(D), "D leads, being maxrank");

    s.crash(D);
    s.run_for(timeout() * 4);
    assert_eq!(trusted_by(&s, A).last().copied(), Some(C), "leadership walked down to C");

    s.restart(D);
    s.run_for(timeout() * 6);

    for n in ALL {
        assert_eq!(
            trusted_by(&s, n).last().copied(),
            Some(D),
            "{n} did not return to the recovered process — leadership only walks downward, which is \
             what `P` makes unavoidable and `◇P` exists to fix"
        );
    }
    // And it went down and back up, rather than never having moved.
    assert_eq!(trusted_by(&s, A), vec![D, C, D], "A followed leadership down and back");
}

#[test]
fn a_restoration_that_does_not_change_the_leader_raises_nothing() {
    // Ω's other property, now that `suspected` can shrink: trust is a function of the set, so a
    // withdrawal below the incumbent's rank changes nothing and must say nothing. Otherwise every
    // recovery of a low-ranked process would cost the layer above an epoch.
    let mut s = sync_sim(21);
    s.run_for(timeout() * 2);
    s.crash(A);
    s.run_for(timeout() * 4);
    let before = trusted_by(&s, D);
    assert_eq!(before.last().copied(), Some(D), "D leads throughout: A is the lowest rank");

    s.restart(A);
    s.run_for(timeout() * 6);

    assert_eq!(
        trusted_by(&s, D),
        before,
        "restoring a process below the incumbent changed the trusted process"
    );
}

#[test]
fn leadership_returning_is_a_property_of_the_detector_not_of_omega() {
    // Composed over `P` instead, the same run cannot recover its leader — because `P` has no
    // withdrawal to raise. This is the contrast that makes the test above mean something, and it
    // is why the detector is a parameter rather than a fixed child.
    use recon_protocols::stacks::EventualLeaderDetectorOverPerfectDetection;

    let mut s: Sim<EventualLeaderDetectorOverPerfectDetection> =
        Sim::new(Config::default().seed(20).synchronous(BOUND), &ALL, |me| {
            EventualLeaderDetectorOverPerfectDetection::over_perfect_detection(
                me,
                ALL,
                heartbeat(),
                timeout(),
            )
        });
    s.run_for(timeout() * 2);
    s.crash(D);
    s.run_for(timeout() * 4);
    s.restart(D);
    s.run_for(timeout() * 6);

    let trusted: Vec<NodeId> =
        s.trace().indications_at(A).map(|Ind::Trust { leader }| *leader).collect();
    assert_eq!(
        trusted.last().copied(),
        Some(C),
        "over a detector that never retracts, leadership cannot return: {trusted:?}"
    );
}

// ------------------------------------------------- a bridge: ELD1's condition failing

#[test]
fn under_a_bridge_the_processes_never_agree_on_a_leader() {
    // `ELD1` is `[eventual]` and inherits the detector's condition. Under a bridge that condition
    // fails permanently — see `eventually_perfect_failure_detector`'s test of the same name — and
    // so does this one: `A` suspects `D` and trusts the highest it can still see, while `B` and `C`
    // suspect nobody and trust `D`.
    //
    // Recorded rather than repaired. Three correct processes with three views, none of them wrong,
    // is exactly what `[eventual]` warns of, and the layer above is obliged to stay safe through it
    // — which `leader_driven_consensus` is where we check.
    let mut s = sync_sim(30);
    s.run_for(timeout() * 2);
    assert_eq!(final_leaders(&s), vec![Some(D); 4], "everyone trusts maxrank to begin with");

    s.sever(A, D);
    s.run_for(timeout() * 20);

    assert!(s.reachable(A, B) && s.reachable(B, D), "a bridge, not two islands");
    assert!(!s.reachable(A, D));

    let leaders = final_leaders(&s);
    assert_eq!(leaders[0], Some(C), "A cannot see D, so it trusts the highest it can");
    for (n, seen) in leaders.iter().enumerate().skip(1) {
        assert_eq!(*seen, Some(D), "{n} still sees D and trusts it");
    }
    assert!(
        leaders.iter().collect::<std::collections::BTreeSet<_>>().len() > 1,
        "so the processes disagree, permanently, and that is ELD1's condition failing"
    );
}
