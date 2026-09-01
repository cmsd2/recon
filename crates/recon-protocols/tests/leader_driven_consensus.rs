//! Paxos against Algorithm 5.7.
//!
//! The tests that matter are the ones run where the leader detector is **wrong**. Run over an
//! accurate detector this algorithm decides, which is exactly what `flooding_consensus` already
//! does — so a suite that only does that has tested nothing worth having. The disputed-leadership
//! tests carry a non-vacuity half confirming that leadership really was disputed.

use core::time::Duration;
use recon_core::NodeId;
use recon_core::Time;
use recon_protocols::epoch_consensus::EpochMsg;
use recon_protocols::leader_driven_consensus::{Cmd, Ind, LeaderDrivenConsensus, Wire};
use recon_sim::TraceEvent;
use recon_sim::{Config, Sim};
use std::collections::BTreeMap;

mod common;
use common::*;

type Uc = LeaderDrivenConsensus<u32>;

fn uc(me: NodeId) -> Uc {
    LeaderDrivenConsensus::new(me, ALL, timing())
}

fn sync_sim(seed: u64) -> Sim<Uc> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, uc)
}

/// A run where the detector's assumption is withdrawn, so it accuses the living.
fn noisy_sim(seed: u64) -> Sim<Uc> {
    Sim::new(
        Config::default()
            .seed(seed)
            .loss(0.35)
            .latency(Duration::from_millis(1), Duration::from_millis(30)),
        &ALL,
        uc,
    )
}

/// What `node` decided, if anything.
fn decided_by(s: &Sim<Uc>, node: NodeId) -> Vec<u32> {
    s.trace().indications_at(node).map(|Ind::Decide(v)| *v).collect()
}

/// Every decision anywhere in the run.
fn all_decisions(s: &Sim<Uc>) -> Vec<u32> {
    ALL.iter().flat_map(|n| decided_by(s, *n)).collect()
}

/// Whether a second leader began while the first's epoch was still unfinished somewhere.
///
/// Read from the trace rather than from who each process follows when the run ends. An epoch's
/// leader is whoever originates its `READ` — nothing else sends one — and an epoch is finished at a
/// process once `DECIDED` for it has been delivered there. Leadership was disputed if some leader's
/// first `READ` in its epoch precedes another leader's epoch being finished at *every* process:
/// that is a process which may yet be asked to accept the old leader's write while the new one is
/// reading, which is the case the intersection argument exists for.
///
/// This is deliberately not "two leaders originating messages at the same instant". An epoch's
/// leader acts for a few milliseconds and a rival emerges after a detector timeout, so that
/// reading came out at 0 in 40 noisy runs and would have made the safety test vacuous by its own
/// standard. The proxy it replaces — different leaders followed at the end of the run — was
/// measuring divergence at the end, not overlap during.
fn leadership_was_disputed(s: &Sim<Uc>) -> bool {
    // (leader, epoch) -> first READ sent.
    let mut began: BTreeMap<(NodeId, u64), Time> = BTreeMap::new();
    // (epoch, process) -> first DECIDED delivered.
    let mut finished: BTreeMap<(u64, NodeId), Time> = BTreeMap::new();
    for e in s.trace().events() {
        match e {
            TraceEvent::Sent { at, from, msg: Wire::Consensus(w), .. }
                if matches!(w.payload.msg, EpochMsg::Read) =>
            {
                began.entry((*from, w.payload.ets)).or_insert(*at);
            }
            TraceEvent::Delivered { at, to, msg: Wire::Consensus(w), .. }
                if matches!(w.payload.msg, EpochMsg::Decided { .. }) =>
            {
                finished.entry((w.payload.ets, *to)).or_insert(*at);
            }
            _ => {}
        }
    }
    began.iter().any(|((later, _), t_later)| {
        began.iter().any(|((earlier, e_earlier), t_earlier)| {
            earlier != later
                && t_earlier < t_later
                && ALL.iter().any(|p| finished.get(&(*e_earlier, *p)).is_none_or(|t| t > t_later))
        })
    })
}

// ------------------------------------------------- The happy path: task 5.1

#[test]
fn a_run_with_no_faults_decides_everywhere() {
    let mut s = sync_sim(1);
    s.command(D, Cmd::Propose(9));
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);

    for n in ALL {
        assert_eq!(decided_by(&s, n), vec![9], "{n}");
    }
}

#[test]
fn nothing_is_decided_that_was_not_proposed() {
    let mut s = sync_sim(2);
    s.command(E, Cmd::Propose(7));
    s.run_for(timeout() * 4);

    for v in all_decisions(&s) {
        assert_eq!(v, 7, "decided {v}, which nobody proposed");
    }
    assert!(!all_decisions(&s).is_empty(), "and something was decided");
}

#[test]
fn a_process_decides_at_most_once() {
    let mut s = sync_sim(3);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 8);

    for n in ALL {
        assert!(decided_by(&s, n).len() <= 1, "{n} decided {:?}", decided_by(&s, n));
    }
}

#[test]
fn a_decision_is_final_across_a_later_epoch() {
    // `if decided = FALSE then …`. A new epoch beginning after a process has decided must not make
    // it decide again, or differently.
    let mut s = sync_sim(4);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);
    let before: Vec<Vec<u32>> = ALL.iter().map(|n| decided_by(&s, *n)).collect();
    assert!(before.iter().all(|d| d == &vec![9]), "everyone decided first: {before:?}");

    // Force epochs to keep changing under them.
    s.crash(E);
    s.run_for(timeout() * 4);
    s.crash(D);
    s.run_for(timeout() * 4);

    for n in [A, B, C] {
        assert_eq!(decided_by(&s, n), vec![9], "{n} decided again or differently");
    }
}

// ------------------------------------------------- Under crashes: task 5.4

#[test]
fn agreement_holds_when_the_leader_crashes() {
    for seed in 0..8u64 {
        let mut s = sync_sim(seed);
        s.command(E, Cmd::Propose(9));
        s.command(D, Cmd::Propose(9));
        s.run_for(heartbeat());
        s.crash(E);
        s.run_for(timeout() * 6);

        let decisions = all_decisions(&s);
        assert!(
            decisions.windows(2).all(|w| w[0] == w[1]),
            "seed {seed}: two processes decided differently: {decisions:?}"
        );
    }
}

#[test]
fn every_correct_process_decides_once_a_majority_is_correct() {
    // `UC4` — conditional on a correct majority and a settled detector, and stated that way.
    let mut s = sync_sim(9);
    s.command(E, Cmd::Propose(4));
    s.command(D, Cmd::Propose(4));
    s.command(C, Cmd::Propose(4));
    s.run_for(heartbeat());
    s.crash(E);
    s.run_for(timeout() * 8);

    for n in [A, B, C, D] {
        assert_eq!(decided_by(&s, n), vec![4], "{n} did not decide");
    }
}

// ------------------- The headline obligation: tasks 5.5 and 5.6

#[test]
fn agreement_holds_while_the_leader_detector_is_wrong() {
    // **The reason this abstraction exists.**
    //
    // The synchrony assumption is withdrawn, so the detector beneath accuses correct processes and
    // Ω disagrees between them — which `eventual_leader_detector`'s own suite establishes is
    // possible before anything is built on it. Processes then enter different epochs, each with a
    // different leader, and more than one acts as leader at once.
    //
    // `flooding_consensus` splits permanently under exactly this, and its suite says so. This must
    // not.
    let mut disputed = 0;
    for seed in 0..40u64 {
        let mut s = noisy_sim(seed);
        for n in ALL {
            s.command(n, Cmd::Propose(n.0 as u32));
        }
        s.run_for(timeout() * 10);

        let decisions = all_decisions(&s);
        assert!(
            decisions.windows(2).all(|w| w[0] == w[1]),
            "seed {seed}: two processes decided differently under a lying detector: {decisions:?}"
        );
        if leadership_was_disputed(&s) {
            disputed += 1;
        }
    }

    // The non-vacuity half. An agreement assertion over runs where one leader was never challenged
    // proves nothing at all — it is satisfied by any algorithm with a single coordinator. "Disputed"
    // is read from the trace: two processes originating leader-only messages at the same time.
    assert!(
        disputed > 0,
        "no run had processes in epochs with different leaders, so the assertion above never met \
         the case it exists for"
    );
}

#[test]
fn the_disputed_leadership_is_real_and_frequent() {
    // Stronger than the clause inside the test above, and separate so a regression in it is
    // legible: leadership is disputed on a substantial fraction of runs, not once in forty.
    let disputed = (0..40u64)
        .filter(|seed| {
            let mut s = noisy_sim(*seed);
            for n in ALL {
                s.command(n, Cmd::Propose(1));
            }
            s.run_for(timeout() * 10);
            leadership_was_disputed(&s)
        })
        .count();

    assert!(
        disputed >= 4,
        "only {disputed}/40 runs had disputed leadership — the safety test above is mostly \
         exercising the quiet case"
    );
}

#[test]
fn nothing_invented_is_decided_under_a_lying_detector() {
    // Validity is not suspended when the detector is. Every decision must still be somebody's
    // proposal, which rules out a leader that has lost track deciding a value from nowhere.
    for seed in 0..20u64 {
        let mut s = noisy_sim(seed);
        for n in ALL {
            s.command(n, Cmd::Propose(n.0 as u32));
        }
        s.run_for(timeout() * 10);

        let proposals: std::collections::BTreeSet<u32> = ALL.iter().map(|n| n.0 as u32).collect();
        for v in all_decisions(&s) {
            assert!(proposals.contains(&v), "seed {seed}: decided {v}, which nobody proposed");
        }
    }
}

// ------------------- The handshake carries the state: task 5.2

#[test]
fn the_next_epoch_begins_from_the_state_the_previous_one_returned() {
    // `⟨ ep.ts, Aborted | state ⟩ … Initialize a new instance ep.ets … with state state`.
    //
    // Aborting and immediately replacing — which is the obvious implementation, and the one that
    // does not have to wait — drops `state`, and with it every acceptance the old epoch collected.
    // Nothing would fail loudly: the new epoch would simply read an empty state everywhere and be
    // free to decide something else.
    let mut s = sync_sim(11);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);

    let epoch_before = s.at(A).epoch();
    for n in ALL {
        let st = s.at(n).state();
        assert_eq!(st.val, Some(9), "{n} accepted nothing, so this test has nothing to carry");
        assert!(st.valts > 0, "{n} accepted at timestamp 0");
    }

    // Force leadership to walk down the membership, so the epoch consensus is rebuilt twice.
    s.crash(E);
    s.run_for(timeout() * 4);
    s.crash(D);
    s.run_for(timeout() * 4);

    for n in [A, B, C] {
        let p = s.at(n);
        assert!(
            p.epoch() > epoch_before,
            "{n} is still in epoch {}, so nothing was rebuilt and the assertion below is empty",
            p.epoch()
        );
        assert_eq!(
            p.state().val,
            Some(9),
            "{n}'s live epoch lost the accepted value across {} rebuilds",
            p.epoch() - epoch_before
        );
    }
}

// ------------------- The contrast that justifies the abstraction: task 5.7

/// The schedule that splits `flooding_consensus`: a partition nobody crosses, so each side accuses
/// the other of crashing while every process is correct throughout.
fn split_by_false_suspicion(seed: u64) -> Sim<Uc> {
    let mut s = sync_sim(seed);
    s.partition(&[&[A, B], &[C, D, E]]);
    for n in ALL {
        s.command(n, Cmd::Propose(n.0 as u32));
    }
    s.run_for(timeout() * 4);
    s
}

#[test]
fn the_schedule_that_splits_flooding_consensus_does_not_split_this() {
    // `flooding_consensus`'s own suite establishes that this partition makes its two sides decide
    // differently, with nobody crashed — `a_false_suspicion_splits_the_decision`. Here the minority
    // cannot assemble a quorum, so it waits instead of deciding wrongly. That trade — termination
    // surrendered, agreement kept — is the whole reason this abstraction exists.
    for seed in 0..8u64 {
        let s = split_by_false_suspicion(seed);
        let decisions = all_decisions(&s);
        assert!(
            decisions.windows(2).all(|w| w[0] == w[1]),
            "seed {seed}: the partition split the decision: {decisions:?}"
        );
        assert_eq!(s.trace().deaths_in_writes(), 0);
    }
}

#[test]
fn the_majority_side_decides_and_the_minority_side_waits() {
    // The non-vacuity half of the test above. An agreement assertion over a run where nobody
    // decided is satisfied by a protocol that has stopped, which is exactly what a partition
    // tempts this one to do.
    let s = split_by_false_suspicion(0);

    for n in [C, D, E] {
        assert_eq!(decided_by(&s, n).len(), 1, "{n} is in the majority and did not decide");
    }
    for n in [A, B] {
        assert!(
            decided_by(&s, n).is_empty(),
            "{n} decided without a majority: {:?}",
            decided_by(&s, n)
        );
    }
}

// ------------------- Termination is conditional, and stated so: task 5.8

#[test]
fn nothing_is_decided_while_no_majority_exists() {
    // `UC4` is conditional on a correct majority, and this is the half of it that is easy to lose:
    // an algorithm that decided here would be violating agreement in every run where the crashed
    // processes had not really crashed.
    //
    // Crashes rather than a partition, and the reason is the detector. This Ω is derived from a
    // *perfect* failure detector, whose accusations are permanent — see `eventual_leader_detector`.
    // A partition therefore does not heal for the detector even when it heals for the network, so a
    // "restore the majority" run over this stack can never resume, and asserting that it does would
    // be testing the departure rather than the algorithm. Recovery is where that question belongs,
    // and `logged_leader_driven_consensus` is where it is asked.
    let mut s = sync_sim(12);
    // Crashed before anything is proposed: with a quorum available even briefly this algorithm is
    // fast enough to have decided already, and the run would then be testing nothing.
    for n in [A, B, C] {
        s.crash(n);
    }
    for n in [D, E] {
        s.command(n, Cmd::Propose(3));
    }
    s.run_for(timeout() * 8);

    assert!(
        all_decisions(&s).is_empty(),
        "decided with two processes out of five: {:?}",
        all_decisions(&s)
    );

    // The non-vacuity half. Two processes that had stopped would also decide nothing, so confirm
    // the survivors were live, led, and trying: an epoch was entered, and its leader is one of them.
    let live: Vec<&Uc> = [D, E].iter().filter_map(|n| s.protocol(*n)).collect();
    assert_eq!(live.len(), 2, "both survivors are still running");
    assert!(
        live.iter().any(|p| p.epoch() > 0),
        "no survivor ever entered an epoch, so this run never reached the quorum it is missing"
    );
    assert!(
        live.iter().all(|p| [D, E].contains(&p.leader())),
        "the survivors still follow a crashed leader, so the detector never settled on them"
    );
}

// ------------------------------------------------- bounded by membership, not by time

#[test]
fn the_send_rate_does_not_grow_after_the_decision() {
    let mut s = sync_sim(20);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);
    assert_eq!(decided_by(&s, A), vec![9], "decided first, so the rate measured is the idle one");
    assert_send_rate_flat!(s, timeout() * 2, 4);
}
