//! Logged Paxos against Algorithms 5.10 and 5.11.
//!
//! Two suites already establish the halves — `leader_driven_consensus.rs` that agreement survives a
//! leader detector that lies, and `logged_epoch_consensus.rs` that an acceptance survives a crash.
//! What is only testable here is the two at once, and the run that matters is the one containing
//! crashes, recoveries **and** disputed leadership, with a companion asserting it really did.

use core::time::Duration;
use recon_core::NodeId;
use recon_core::Time;
use recon_protocols::logged_epoch_consensus::{self as lep, Announce};
use recon_protocols::logged_leader_driven_consensus::{
    Cmd, Ind, LoggedLeaderDrivenConsensus, Wire,
};
use recon_sim::{Config, Sim, TraceEvent};
use std::collections::BTreeMap;

mod common;
use common::*;

type Luc = LoggedLeaderDrivenConsensus<u32>;

fn luc(me: NodeId) -> Luc {
    LoggedLeaderDrivenConsensus::new(me, ALL, timing())
}

fn sim(seed: u64) -> Sim<Luc> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, luc)
}

/// A run where the detector's assumption is withdrawn, so it accuses the living.
fn noisy(seed: u64) -> Sim<Luc> {
    Sim::new(
        Config::default()
            .seed(seed)
            .loss(0.3)
            .latency(Duration::from_millis(1), Duration::from_millis(30)),
        &ALL,
        luc,
    )
}

/// Every `Decide` raised at `node`, in order. A logged indication may be raised more than once.
fn announced(s: &Sim<Luc>, node: NodeId) -> Vec<u32> {
    s.trace().indications_at(node).map(|Ind::Decide(v)| *v).collect()
}

/// What each running process holds durably.
fn held(s: &Sim<Luc>) -> Vec<(NodeId, Option<u32>)> {
    ALL.iter().filter_map(|n| s.protocol(*n).map(|p| (*n, p.decision().copied()))).collect()
}

/// Every value announced anywhere in the run.
fn all_announced(s: &Sim<Luc>) -> Vec<u32> {
    ALL.iter().flat_map(|n| announced(s, *n)).collect()
}

/// Whether a second leader began while the first's epoch was still unfinished somewhere — see the
/// volatile suite's helper of the same name for the reading. Here the leader-only messages are the
/// `sbeb` announcements.
fn leadership_was_disputed(s: &Sim<Luc>) -> bool {
    let mut began: BTreeMap<(NodeId, u64), Time> = BTreeMap::new();
    let mut finished: BTreeMap<(u64, NodeId), Time> = BTreeMap::new();
    for e in s.trace().events() {
        match e {
            TraceEvent::Sent { at, from, msg: Wire::Consensus(lep::Wire::Announce(t)), .. }
                if matches!(t.msg, Announce::Read) =>
            {
                began.entry((*from, t.ets)).or_insert(*at);
            }
            TraceEvent::Delivered {
                at, to, msg: Wire::Consensus(lep::Wire::Announce(t)), ..
            } if matches!(t.msg, Announce::Decided { .. }) => {
                finished.entry((t.ets, *to)).or_insert(*at);
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

fn crashes(s: &Sim<Luc>) -> usize {
    s.trace().events().iter().filter(|e| matches!(e, TraceEvent::Crashed { .. })).count()
}

fn restarts(s: &Sim<Luc>) -> usize {
    s.trace().events().iter().filter(|e| matches!(e, TraceEvent::Restarted { .. })).count()
}

// ------------------------------------------------- crashes and recoveries: task 8.1

#[test]
fn a_run_with_crashes_and_recoveries_decides_everywhere_once_a_majority_is_back() {
    let mut s = sim(1);
    s.command(E, Cmd::Propose(9));
    s.command(D, Cmd::Propose(9));

    // Two processes down at once: three of five is still a majority, so this is survivable, and
    // the run then puts both back.
    s.run_for(heartbeat());
    s.crash(B);
    s.crash(C);
    s.run_for(timeout() * 2);
    s.restart(B);
    s.run_for(timeout());
    s.restart(C);
    s.run_for(timeout() * 8);

    for (n, d) in held(&s) {
        assert_eq!(d, Some(9), "{n} holds no decision");
    }
    assert!(crashes(&s) == 2 && restarts(&s) == 2, "the run contained the faults it claims to");
}

#[test]
fn nothing_is_decided_that_was_not_proposed() {
    let mut s = sim(2);
    s.command(E, Cmd::Propose(7));
    s.run_for(timeout() * 4);
    s.crash(A);
    s.restart(A);
    s.run_for(timeout() * 4);

    let decided = all_announced(&s);
    assert!(!decided.is_empty(), "nothing was decided, so validity has nothing to say");
    for v in decided {
        assert_eq!(v, 7, "decided {v}, which nobody proposed");
    }
}

// ------------------------------------------------- the decision outlives the process: task 8.2

#[test]
fn a_process_that_decided_still_holds_that_decision_after_a_crash_and_recovery() {
    let mut s = sim(3);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);
    assert_eq!(s.at(C).decision(), Some(&9), "C decided before the crash");
    let before = announced(&s, C).len();

    s.crash(C);
    s.run_for(timeout());
    s.restart(C);
    s.run_for(timeout());

    let p = s.at(C);
    assert_eq!(p.decision(), Some(&9), "C came back without its decision");

    // `⟨ luc, Decide | decision ⟩` is specified as "variable `decision` in stable storage contains
    // the decided value", and Module 5.5 has no integrity property, so the indication comes again.
    // What must not change is the value.
    let after = announced(&s, C);
    assert!(after.len() > before, "C never re-announced what its record still holds");
    assert!(
        after.iter().all(|v| *v == 9),
        "C announced something else after recovering: {after:?}"
    );
    assert_eq!(s.trace().recoveries_with_state(), 1, "C recovered from a record, not from nothing");
}

#[test]
fn a_process_that_had_decided_nothing_comes_back_having_decided_nothing() {
    // The counterpart. Every process writes at `Init`, so the record always exists; what must be
    // true is that it says ⊥, and that recovery does not announce a decision from nowhere.
    let mut s = sim(4);
    s.run_for(heartbeat());
    assert_eq!(s.at(A).decision(), None, "nothing was proposed");

    s.crash(A);
    s.restart(A);
    s.run_for(heartbeat());

    assert_eq!(s.at(A).decision(), None, "A invented a decision on recovery");
    assert!(announced(&s, A).is_empty(), "A announced one: {:?}", announced(&s, A));
}

// ------------------- all three faults at once: tasks 8.3 and 8.4

/// A run under crashes, recoveries, and a detector accusing the living.
fn three_faults(seed: u64) -> Sim<Luc> {
    let mut s = noisy(seed);
    for n in ALL {
        s.command(n, Cmd::Propose(n.0 as u32));
    }
    s.run_for(timeout());
    s.crash(B);
    s.run_for(timeout());
    s.restart(B);
    s.run_for(timeout());
    s.crash(A);
    s.run_for(timeout());
    s.restart(A);
    s.run_for(timeout() * 8);
    s
}

#[test]
fn agreement_holds_under_crashes_recoveries_and_a_lying_detector_at_once() {
    for seed in 0..25u64 {
        let s = three_faults(seed);

        let decided = all_announced(&s);
        assert!(
            decided.windows(2).all(|w| w[0] == w[1]),
            "seed {seed}: two processes decided differently: {decided:?}"
        );

        // And the durable records agree with the announcements, which is the half a run that only
        // watched indications would miss.
        for (n, d) in held(&s) {
            if let Some(v) = d {
                assert!(
                    decided.first().is_none_or(|first| *first == v),
                    "seed {seed}: {n} holds {v} where the run announced {decided:?}"
                );
            }
        }
    }
}

#[test]
fn that_run_really_contained_all_three() {
    // The non-vacuity half, and it is three separate claims: something crashed, something came
    // back, and more than one process acted as leader. An agreement assertion over a quiet run
    // with one unchallenged leader proves nothing at all.
    let mut with_disputed_leadership = 0;
    for seed in 0..25u64 {
        let s = three_faults(seed);
        assert_eq!(crashes(&s), 2, "seed {seed}: two crashes");
        assert_eq!(restarts(&s), 2, "seed {seed}: two recoveries");
        assert!(s.trace().recoveries_with_state() > 0, "seed {seed}: recovered from a record");
        if leadership_was_disputed(&s) {
            with_disputed_leadership += 1;
        }
    }
    assert!(
        with_disputed_leadership >= 3,
        "only {with_disputed_leadership}/25 runs had two processes acting as leader at once, so the \
         agreement test above is mostly exercising the quiet case"
    );
}

#[test]
fn nothing_invented_is_decided_under_all_three() {
    let proposals: std::collections::BTreeSet<u32> = ALL.iter().map(|n| n.0 as u32).collect();
    for seed in 0..15u64 {
        let s = three_faults(seed);
        for v in all_announced(&s) {
            assert!(proposals.contains(&v), "seed {seed}: decided {v}, which nobody proposed");
        }
    }
}

// ------------------------------------------------- termination is conditional: task 8.5

#[test]
fn no_decision_while_no_majority_exists_and_progress_resumes_when_recovery_restores_one() {
    // `LUC1` is conditional on a correct majority. Three of five are down, so no quorum can form
    // anywhere and the honest behaviour is to wait — and unlike the fail-noisy suite, this one can
    // *restore* the majority, because a crashed process here comes back rather than being gone.
    let mut s = sim(5);
    for n in [A, B, C] {
        s.crash(n);
    }
    for n in [D, E] {
        s.command(n, Cmd::Propose(3));
    }
    s.run_for(timeout() * 8);

    assert!(
        all_announced(&s).is_empty(),
        "decided with two processes out of five: {:?}",
        all_announced(&s)
    );
    // Non-vacuity: the survivors were live and led, not stopped.
    let live: Vec<&Luc> = [D, E].iter().filter_map(|n| s.protocol(*n)).collect();
    assert_eq!(live.len(), 2);
    assert!(
        live.iter().any(|p| p.epoch() > 0),
        "no survivor entered an epoch, so this run never reached the quorum it is missing"
    );

    for n in [A, B, C] {
        s.restart(n);
    }
    s.run_for(timeout() * 12);

    for (n, d) in held(&s) {
        assert_eq!(d, Some(3), "{n} did not decide after recovery restored the majority");
    }
}

// ------------------------------------------------- what the slot buys

#[test]
fn the_parent_and_both_children_keep_their_own_part_of_one_record() {
    // The composition this module exists to demonstrate, asserted rather than assumed. All three
    // layers write through one store; if any of them overwrote another's part, a recovery would
    // read back half of what it wrote — and nothing would fail until then.
    let mut s = sim(6);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);

    // Epoch and leader are the parent's, and both are non-trivial: an epoch was entered.
    let p = s.at(C);
    assert!(p.epoch() > 0, "no epoch was entered, so the record under test is the initial one");
    assert_eq!(p.leader(), E);
    assert_eq!(p.decision(), Some(&9), "the parent's own part");
    assert_eq!(p.state().valts, p.epoch(), "the epoch consensus child's part");
    assert_eq!(p.state().val, Some(9));

    s.crash(C);
    s.restart(C);

    // Read before the wire says anything. Running a heartbeat first and then asserting these four
    // would pass with C's storage entirely wiped — E is still leading, the epoch re-establishes and
    // the decision comes back — so the assertions would be about the network rather than about the
    // one record this test exists to check. The mutation audit found exactly that.
    let p = s.at(C);
    assert!(p.epoch() > 0, "the parent lost its epoch");
    assert_eq!(p.leader(), E, "the parent lost its leader");
    assert_eq!(p.decision(), Some(&9), "the parent lost its decision");
    assert_eq!(p.state().val, Some(9), "the epoch consensus child lost what it accepted");

    // And it rejoins on what it read back, rather than the read-back being inert.
    s.run_for(heartbeat());
    let p = s.at(C);
    assert_eq!(p.decision(), Some(&9), "and it still holds it once the run continues");
}

#[test]
fn every_write_is_one_rewritten_record_and_never_an_append() {
    // Three layers, one `Meta`, and `Entry` uninhabited throughout. A protocol that reached for the
    // sequence here would be rewriting something that accumulates, which is the `O(n²)` mistake
    // `docs/bounded-space.md` names.
    let mut s = sim(7);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);

    assert!(s.trace().writes() > 0, "nothing was written");
    assert_eq!(
        s.trace().metadata_writes(),
        s.trace().writes(),
        "something appended, and nothing in this stack has an inhabited Entry"
    );
    assert_eq!(s.trace().appends(), 0);
}

// ------------------------------------------------- bounded by membership, not by time

#[test]
fn the_send_rate_does_not_grow_after_the_decision() {
    // Measured before the children's guards: 27.8k → 91.8k per 400 ms across five windows. The
    // final epoch never aborts, so nothing but the guards bounds this.
    let mut s = sim(20);
    s.command(E, Cmd::Propose(9));
    s.run_for(timeout() * 4);
    assert_eq!(s.at(A).decision(), Some(&9), "decided first, so the rate measured is the idle one");
    assert_send_rate_flat!(s, timeout() * 2, 4);
}

// ------------------------------------------------- dying inside the write

#[test]
fn dying_inside_the_decision_write_never_leaves_a_decision_announced_without_a_record() {
    // Armed once C has accepted and not yet decided, so the write it dies in is one of the two
    // that make the decision durable — the child's `epochdecision` or this layer's `decision`.
    // Either may or may not have landed. What is not allowed is C announcing `Decide` from the
    // handler that died; and what must follow is that C decides 9 regardless, because the
    // announcement is still being retransmitted and the record, if it landed, is read back.
    let mut landed = 0;
    let mut lost = 0;
    let mut armed_runs = 0;
    for seed in 0..40u64 {
        let mut s = sim(seed);
        s.command(E, Cmd::Propose(9));
        // Step until C has accepted but not decided.
        let mut armed = false;
        for _ in 0..200 {
            s.run_for(Duration::from_millis(1));
            let p = s.at(C);
            if p.state().val == Some(9) && p.decision().is_none() {
                s.crash_on_next_write(C);
                armed = true;
                break;
            }
            if p.decision().is_some() {
                break;
            }
        }
        if !armed {
            continue; // the seed decided C's write and decision in one step; nothing to arm
        }
        armed_runs += 1;
        let before = announced(&s, C).len();
        s.run_for(timeout() * 2);
        assert_eq!(s.trace().deaths_in_writes(), 1, "seed {seed}: C died in a write");
        assert_eq!(
            announced(&s, C).len(),
            before,
            "seed {seed}: C announced from the doomed handler"
        );

        s.restart(C);
        // Recovery reads both records. If the child's `epochdecision` landed, the recovery branch
        // of Algorithm 5.10 announces it now — that is the "landed" outcome made visible.
        if s.at(C).decision() == Some(&9) {
            landed += 1;
        } else {
            lost += 1;
        }
        s.run_for(timeout() * 6);
        assert_eq!(s.at(C).decision(), Some(&9), "seed {seed}: C never decided after recovering");
        assert!(announced(&s, C).iter().all(|v| *v == 9), "seed {seed}: {:?}", announced(&s, C));
    }
    assert!(armed_runs >= 10, "only {armed_runs} runs could be armed, so this proves little");
    assert!(landed > 0 && lost > 0, "both outcomes must occur: {landed} landed, {lost} lost");
}
