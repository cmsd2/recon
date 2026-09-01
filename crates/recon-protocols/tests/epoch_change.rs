//! Epoch-change against Module 5.5 — monotonicity always, consistency eventually.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::epoch_change::{EpochChange, Ind};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

const BOUND: Duration = Duration::from_millis(20);

fn retransmit() -> Duration {
    Duration::from_millis(10)
}
fn heartbeat() -> Duration {
    BOUND * 2
}
fn timeout() -> Duration {
    heartbeat() * 3
}

fn ec(me: NodeId) -> EpochChange {
    EpochChange::new(me, ALL, retransmit(), heartbeat(), timeout())
}

fn sync_sim(seed: u64) -> Sim<EpochChange> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, ec)
}

/// Every epoch `node` started, in order.
fn epochs_at(s: &Sim<EpochChange>, node: NodeId) -> Vec<(u64, NodeId)> {
    s.trace().indications_at(node).map(|Ind::StartEpoch { ts, leader }| (*ts, *leader)).collect()
}

// ------------------------------------------------- Monotonicity: tasks 3.1 to 3.3

#[test]
fn a_steady_leader_starts_one_epoch_and_no_more() {
    // An epoch costs the layer above an abort and a restart, so one that begins for any reason
    // other than a leadership change is pure loss.
    let mut s = sync_sim(1);
    s.run_for(timeout() * 8);

    for n in ALL {
        let started = epochs_at(&s, n);
        assert_eq!(started.len(), 1, "{n} started {started:?} while leadership never changed");
        assert_eq!(started[0].1, D, "and the leader is maxrank(Π)");
    }
}

#[test]
fn timestamps_strictly_increase_at_each_process() {
    let mut s = sync_sim(2);
    s.run_for(timeout() * 2);
    s.crash(D);
    s.run_for(timeout() * 4);
    s.crash(C);
    s.run_for(timeout() * 4);

    for n in [A, B] {
        let ts: Vec<u64> = epochs_at(&s, n).into_iter().map(|(t, _)| t).collect();
        assert!(ts.len() >= 2, "{n} needs several epochs for this to say anything: {ts:?}");
        assert!(ts.windows(2).all(|w| w[1] > w[0]), "{n} did not increase: {ts:?}");
    }
}

#[test]
fn one_timestamp_names_one_leader() {
    // `ts := rank(self)` advanced by `ts := ts + N` puts each process in its own residue class, so
    // two processes cannot mint the same timestamp. This checks the consequence: nobody observes
    // one timestamp under two different leaders.
    let mut s = sync_sim(3);
    s.run_for(timeout() * 2);
    s.crash(D);
    s.run_for(timeout() * 4);
    s.crash(C);
    s.run_for(timeout() * 4);

    let mut seen: std::collections::BTreeMap<u64, NodeId> = Default::default();
    for n in ALL {
        for (ts, leader) in epochs_at(&s, n) {
            if let Some(previous) = seen.insert(ts, leader) {
                assert_eq!(previous, leader, "timestamp {ts} named two leaders");
            }
        }
    }
    assert!(seen.len() >= 2, "several timestamps must have been observed: {seen:?}");
}

#[test]
fn each_process_draws_from_its_own_residue_class() {
    // The mechanism behind the test above, checked directly. A leader's timestamps are all
    // congruent to its rank modulo N, so no two processes can ever collide.
    let mut s = sync_sim(4);
    s.run_for(timeout() * 2);
    s.crash(D);
    s.run_for(timeout() * 4);
    s.crash(C);
    s.run_for(timeout() * 4);

    let n = ALL.len() as u64;
    let ranks: std::collections::BTreeMap<NodeId, u64> =
        ALL.iter().enumerate().map(|(i, p)| (*p, i as u64 + 1)).collect();

    for process in ALL {
        for (ts, leader) in epochs_at(&s, process) {
            assert_eq!(
                ts % n,
                ranks[&leader] % n,
                "epoch {ts} was led by {leader}, whose rank is {}",
                ranks[&leader]
            );
        }
    }
}

#[test]
fn a_leadership_change_starts_a_new_epoch() {
    // **More than one, and that is the algorithm rather than a defect.** A test asserting exactly
    // two failed here, observing `[(8, D), (11, C), (15, C), (19, C)]`.
    //
    // `Trust` does not reach every process at the same instant. A process still trusting D that
    // receives C's `NEWEPOCH` takes the `else` branch — the sender is not who it trusts — and
    // NACKs, so C bumps its timestamp and announces again. The churn stops once everyone trusts C,
    // which is what `epochs_settle_once_leadership_does` pins. The guarantee is monotonicity and
    // eventual consistency, not one epoch per leadership change.
    let mut s = sync_sim(5);
    s.run_for(timeout() * 2);
    let before = epochs_at(&s, A);

    s.crash(D);
    s.run_for(timeout() * 8);
    let after = epochs_at(&s, A);

    assert_eq!(before.len(), 1, "one epoch under D");
    assert!(after.len() > before.len(), "D going must start at least one more: {after:?}");
    assert_eq!(after.last().unwrap().1, C, "and the last is led by the next greatest correct");
    assert!(
        after[1..].iter().all(|(_, l)| *l == C),
        "every epoch after D's is C's — the churn is C climbing past NACKs, not leadership \
         changing again: {after:?}"
    );
}

#[test]
fn the_churn_after_a_leadership_change_is_finite() {
    // Non-vacuity for the settling test: the NACK loop terminates rather than running as long as
    // the simulation does. Without this, "eventually settles" could be satisfied by a run that had
    // not yet been given long enough.
    let mut s = sync_sim(11);
    s.run_for(timeout() * 2);
    s.crash(D);
    s.run_for(timeout() * 8);
    let settled = epochs_at(&s, A).len();

    s.run_for(timeout() * 24);
    assert_eq!(
        epochs_at(&s, A).len(),
        settled,
        "three times the time started no further epoch, so the NACK loop converged"
    );
}

// ------------------------------------------------- Consistency: tasks 3.4 and 3.5

#[test]
fn epochs_settle_once_leadership_does() {
    let mut s = sync_sim(6);
    s.run_for(timeout() * 2);
    s.crash(D);
    s.run_for(timeout() * 8);

    let finals: Vec<Option<(u64, NodeId)>> =
        [A, B, C].iter().map(|n| epochs_at(&s, *n).last().copied()).collect();
    assert!(
        finals.iter().all(|f| *f == finals[0]),
        "every correct process must reach the same last epoch: {finals:?}"
    );
    assert_eq!(finals[0].map(|(_, l)| l), Some(C), "led by C");
}

#[test]
fn processes_may_be_in_different_epochs_meanwhile() {
    // The settling assertion above must not be read as a claim that they never differ. On a lossy
    // asynchronous network the leader detector disagrees, and epochs diverge with it — which is
    // permitted, and is the case everything above this layer has to survive.
    let diverged = (0..40u64).any(|seed| {
        let mut s: Sim<EpochChange> = Sim::new(
            Config::default()
                .seed(seed)
                .loss(0.6)
                .latency(Duration::from_millis(1), Duration::from_millis(30)),
            &ALL,
            ec,
        );
        s.run_for(timeout() * 6);
        let finals: Vec<Option<(u64, NodeId)>> =
            ALL.iter().map(|n| epochs_at(&s, *n).last().copied()).collect();
        finals.iter().any(|f| *f != finals[0])
    });

    assert!(
        diverged,
        "correct processes must be able to sit in different epochs — if they cannot, the layers \
         above are never tested against the case they exist to survive"
    );
}
