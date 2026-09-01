//! Epoch-change against Module 5.5 — monotonicity always, consistency eventually.

use core::time::Duration;
use recon_core::{Effect, Event, MemStore, NodeId, ProtoEffect, Protocol, Time, step_with};
use recon_protocols::epoch_change::{EpochChange, EpochMsg, Ind, Wire};
use recon_protocols::perfect_link as pl;
use recon_sim::{Config, Sim};

mod common;
use common::*;

const ALL: [NodeId; 4] = [A, B, C, D];

fn ec(me: NodeId) -> EpochChange {
    EpochChange::new(me, ALL, timing())
}

fn sync_sim(seed: u64) -> Sim<EpochChange> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, ec)
}

/// Every epoch `node` started, in order.
/// `rank(p)` as the module computes it: position in the ordered membership, counting from one.
fn rank_of(p: NodeId) -> u64 {
    ALL.iter().position(|q| *q == p).expect("p is a member") as u64 + 1
}

/// One report or refusal, as it arrives over the wire.
fn nack(from: NodeId, seq: u64, nts: u64) -> <EpochChange as Protocol>::Msg {
    Wire::Epoch(pl::Wire { id: pl::MsgId { src: from, seq }, payload: EpochMsg::Nack { nts } })
}

/// Drive one event against a directly-held instance.
fn drive(
    p: &mut EpochChange,
    ev: Event<
        <EpochChange as Protocol>::Cmd,
        <EpochChange as Protocol>::Msg,
        core::convert::Infallible,
    >,
) -> Vec<ProtoEffect<EpochChange>> {
    use rand::SeedableRng;
    step_with(
        p,
        ev,
        Time::ZERO,
        &mut rand_chacha::ChaCha8Rng::seed_from_u64(0),
        &mut MemStore::default(),
        &mut 0,
    )
}

/// The timestamps announced by these effects.
fn announcements(fx: &[ProtoEffect<EpochChange>]) -> Vec<u64> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Send { msg: Wire::Epoch(w), .. } => match w.payload {
                EpochMsg::NewEpoch { ts } => Some(ts),
                EpochMsg::Nack { .. } => None,
            },
            _ => None,
        })
        .collect::<std::collections::BTreeSet<u64>>()
        .into_iter()
        .collect()
}

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

// ------------------------------------------------- bounded by membership, not by time

#[test]
fn the_send_rate_does_not_grow_once_leadership_has_settled() {
    // The module claims a membership bound. Once leadership settles the perfect links beneath
    // retransmit a *fixed* set for ever, and a fixed set is a flat rate. The unguarded NACK of
    // Algorithm 5.5 fails this: every refusal re-announces, and the set is never fixed.
    let mut s = sync_sim(20);
    s.run_for(timeout() * 4);
    assert_send_rate_flat!(s, timeout() * 2, 4);
}

// ------------------------------------------------- telling a leader where the others reached

#[test]
fn a_leader_that_never_observed_a_change_still_starts_an_epoch() {
    // The gap a detector that retracts exposes, and the reason for the report. D is maxrank of the
    // whole membership *and* of its own partition, so its trusted process never changes and Ω never
    // tells it anything. Meanwhile [A,B] and [C] run their epochs ahead under leaders of their own.
    // When the partition heals everyone trusts D — which, told nothing, would announce nothing for
    // ever, and nothing it retransmits draws a refusal because its recipients deduplicate it.
    let mut s = sync_sim(13);
    s.run_for(timeout() * 2);
    s.partition(&[&[A, B], &[C], &[D]]);
    s.run_for(timeout() * 6);

    let ahead = s.at(C).last_timestamp().max(s.at(A).last_timestamp());
    assert!(ahead > s.at(D).last_timestamp(), "the isolated groups ran ahead of D: {ahead}");
    assert_eq!(s.at(D).trusted(), D, "D trusted itself throughout, so was never told anything");

    s.heal();
    s.run_for(timeout() * 20);

    for n in ALL {
        assert_eq!(s.at(n).trusted(), D, "{n} trusts maxrank once suspicions are withdrawn");
    }
    let settled = s.at(D).last_timestamp();
    assert!(settled > ahead, "D's epoch {settled} did not climb past the group that ran ahead");
    for n in ALL {
        assert_eq!(s.at(n).last_timestamp(), settled, "{n} did not join D's epoch");
    }
}

#[test]
fn a_refused_leader_climbs_past_in_one_step_not_one_per_refusal() {
    // The report carries how far the sender has reached, and the leader jumps above it rather than
    // adding `N` once per refusal — which costs a round trip per step, so crossing a gap of `g`
    // costs `g / N` of them. Driven directly, because what is under test is the arithmetic and a
    // run's incidental gap is not evidence about it.
    let mut d = ec(D);
    assert_eq!(d.trusted(), D, "D is maxrank, so it trusts itself from the start");

    let fx = drive(&mut d, Event::Msg { from: A, msg: nack(A, 1, 100) });
    let announced = announcements(&fx);
    assert_eq!(announced.len(), 1, "one announcement, not one per step of the gap");
    let ts = announced[0];
    assert!(ts > 100, "the candidate {ts} is not above the timestamp it was told");
    assert_eq!(
        ts % ALL.len() as u64,
        rank_of(D) % ALL.len() as u64,
        "the jump left this process's residue class, so two processes could mint {ts}"
    );

    // And a repeat of the same report names a timestamp already passed, so nothing moves.
    let fx = drive(&mut d, Event::Msg { from: A, msg: nack(A, 2, 100) });
    assert!(announcements(&fx).is_empty(), "a repeated report moved the candidate again");
}

#[test]
fn a_report_no_higher_than_the_candidate_already_chosen_moves_nothing() {
    // Boundedness. The refusal guard was relaxed from `nts = ts` to `nts ≥ ts`, and what keeps it
    // bounded is that the jump leaves the candidate strictly above what was reported — so a repeat
    // names a timestamp already passed. Repeats are guaranteed: the link beneath retransmits.
    let mut s = sync_sim(15);
    s.run_for(timeout() * 4);
    let settled = s.at(D).last_timestamp();
    let before = epochs_at(&s, A).len();

    s.run_for(timeout() * 20);
    assert_eq!(s.at(D).last_timestamp(), settled, "D's epoch moved with nothing changed");
    assert_eq!(epochs_at(&s, A).len(), before, "an epoch began with nothing changed");
}

#[test]
fn a_settled_stack_reports_nothing() {
    // The report rides the trust-change edge, so a run in which trust never changes contains none
    // at all — the quiescence the steady-leader test asserts, now that there is a second thing that
    // could break it.
    let mut s = sync_sim(16);
    s.run_for(timeout() * 8);
    let reports = s
        .trace()
        .sends()
        .filter(
            |(_, _, m)| matches!(m, Wire::Epoch(w) if matches!(w.payload, EpochMsg::Nack { .. })),
        )
        .count();
    assert_eq!(reports, 0, "a settled stack sent {reports} reports or refusals");
}

// ------------------------------------------------- a bridge: epochs settle anyway

#[test]
fn under_a_bridge_epochs_settle_even_though_leadership_does_not() {
    // The interesting half of the bridge, and not what I expected before running it. Ω never
    // converges — `A` trusts `C` while everyone else trusts `D`, and no partition heals because
    // nothing is broken. Yet the epochs *do* settle: all four sit in one epoch led by `D`, and stay
    // there.
    //
    // Why: `A` trusts `C`, but `C` trusts `D` and so never announces, and `A` starts no epoch of its
    // own because it is not its own leader. So the disagreement is stable rather than churning —
    // `EC2`'s condition has lapsed, and what that costs is that `A` follows an epoch led by a
    // process it no longer trusts, not that epochs run away.
    let mut s = sync_sim(30);
    s.run_for(timeout() * 2);
    s.sever(A, D);
    s.run_for(timeout() * 20);

    assert!(s.reachable(A, B) && s.reachable(B, D) && !s.reachable(A, D), "a bridge");
    assert_eq!(s.at(A).trusted(), C, "A trusts the highest it can see");
    assert_eq!(s.at(D).trusted(), D, "and D still trusts itself");

    let settled: Vec<u64> = ALL.iter().map(|n| s.at(*n).last_timestamp()).collect();
    assert!(settled.iter().all(|t| *t == settled[0]), "all in one epoch: {settled:?}");

    // Settled, not merely momentarily equal: twenty more timeouts move nothing.
    s.run_for(timeout() * 20);
    let later: Vec<u64> = ALL.iter().map(|n| s.at(*n).last_timestamp()).collect();
    assert_eq!(settled, later, "the epochs are stable under a standing disagreement");
}
