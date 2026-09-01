//! Logged read/write epoch consensus against Algorithm 5.9.
//!
//! Everything [`epoch_consensus`](../src/epoch_consensus.rs)'s suite establishes about quorums and
//! the abort handshake is assumed here. What this suite is for is the sentence that makes the
//! logged version worth having: **what a process accepted is a promise to a quorum, and it has to
//! be a promise the process can still keep after it has died and come back.**

use core::time::Duration;
use recon_core::{Event, MemStore, NodeId, Time, step_with};
use recon_protocols::logged_epoch_consensus::{
    Announce, Cmd, Ind, LoggedEpochConsensus, Reply, State, Tagged, Wire,
};
use recon_sim::{Config, Sim, TraceEvent};

mod common;
use common::*;

const EPOCH: u64 = 7;

type Lep = LoggedEpochConsensus<u32>;

/// Every process in an instance for [`EPOCH`], led by `E`.
fn sim(seed: u64) -> Sim<Lep> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, |me| {
        LoggedEpochConsensus::new(me, ALL, EPOCH, E, State::default(), retransmit())
    })
}

fn lossy(seed: u64) -> Sim<Lep> {
    Sim::new(Config::default().seed(seed).loss(0.3), &ALL, |me| {
        LoggedEpochConsensus::new(me, ALL, EPOCH, E, State::default(), retransmit())
    })
}

fn settle(s: &mut Sim<Lep>) {
    s.run_for(Duration::from_millis(400));
}

fn decided_by(s: &Sim<Lep>, node: NodeId) -> Vec<u32> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            Ind::Decide(v) => Some(*v),
            Ind::Aborted(_) => None,
        })
        .collect()
}

fn all_decisions(s: &Sim<Lep>) -> Vec<u32> {
    ALL.iter().flat_map(|n| decided_by(s, *n)).collect()
}

/// The index in the trace of the first event matching `f`.
fn first_index(
    s: &Sim<Lep>,
    f: impl Fn(&TraceEvent<Wire<u32>, Ind<u32>, recon_protocols::Note>) -> bool,
) -> Option<usize> {
    s.trace().events().iter().position(f)
}

fn is_accept_from(
    e: &TraceEvent<Wire<u32>, Ind<u32>, recon_protocols::Note>,
    node: NodeId,
) -> bool {
    matches!(
        e,
        TraceEvent::Sent { from, msg: Wire::Reply(t), .. }
            if *from == node && matches!(t.msg, Reply::Accept)
    )
}

// ------------------------------------------------- durable before visible: task 7.1

#[test]
fn the_acceptance_is_written_before_it_is_sent() {
    // `(valts, val) := (ets, v); store(valts, val); trigger ⟨ sl, Send | ℓ, [ACCEPT] ⟩` — in that
    // order, in the handler's own text. The ACCEPT is a promise to a quorum: a process that made it
    // and then came back with no record would answer a later epoch's read with an empty state, the
    // later leader would find nothing in the intersection, and `EPC4` would fail silently.
    let mut s = sim(1);
    s.command(E, Cmd::Propose(9));
    settle(&mut s);

    for n in ALL {
        let accept = first_index(&s, |e| is_accept_from(e, n))
            .unwrap_or_else(|| panic!("{n} never accepted, so there is nothing to order"));
        let wrote = first_index(&s, |e| matches!(e, TraceEvent::Wrote { node, .. } if *node == n))
            .expect("every process writes at init");

        // The init write is not the one being claimed about. Find the write that recorded the
        // acceptance: the last one at or before the ACCEPT.
        let recorded = s.trace().events()[..accept]
            .iter()
            .filter(|e| matches!(e, TraceEvent::Wrote { node, .. } if *node == n))
            .count();
        assert!(
            recorded >= 2,
            "{n} sent ACCEPT after only {recorded} write(s) — the init write at {wrote} and \
             nothing recording what it accepted"
        );
        assert_eq!(s.at(n).state().valts, EPOCH, "{n}");
    }
}

#[test]
fn the_decision_is_written_before_it_is_reported() {
    let mut s = sim(2);
    s.command(E, Cmd::Propose(9));
    settle(&mut s);

    for n in ALL {
        let decided = first_index(
            &s,
            |e| matches!(e, TraceEvent::Indicated { node, ind: Ind::Decide(_), .. } if *node == n),
        )
        .unwrap_or_else(|| panic!("{n} never decided"));
        let writes_before = s.trace().events()[..decided]
            .iter()
            .filter(|e| matches!(e, TraceEvent::Wrote { node, .. } if *node == n))
            .count();
        // Init, the acceptance, and the decision.
        assert_eq!(
            writes_before, 3,
            "{n} reported a decision with {writes_before} writes behind it"
        );
        assert_eq!(s.at(n).epoch_decision(), Some(&9), "{n}");
    }
}

#[test]
fn a_repeated_write_is_recorded_once() {
    // The stubborn broadcast redelivers `[WRITE, v]` for ever. Storing the same value again is
    // harmless but it is still a write, and a claim about write cost that a repeat quietly
    // multiplies is not a claim. One write per acceptance, whatever the link does.
    let mut s = sim(3);
    s.command(E, Cmd::Propose(9));
    settle(&mut s);

    // Init, acceptance, decision — three per process, and the run is long enough that every
    // message has been redelivered many times.
    assert_eq!(s.trace().writes(), 3 * ALL.len(), "writes: {}", s.trace().writes());
    assert!(
        s.trace().delivery_count() > 3 * ALL.len(),
        "the run did not repeat anything, so this test proves nothing"
    );
}

// ------------------------------------------------- recovery: tasks 7.2 and 7.3

#[test]
fn a_recovered_process_answers_a_read_with_what_it_accepted() {
    // The whole purchase. C accepts, dies, comes back, and a later read must find the acceptance —
    // because that is the intersection a later epoch's leader depends on.
    let mut s = sim(4);
    s.command(E, Cmd::Propose(9));
    settle(&mut s);
    assert_eq!(s.at(C).state().val, Some(9), "C accepted before the crash");

    s.crash(C);
    s.restart(C);
    s.run_for(Duration::from_millis(50));

    let p = s.at(C);
    assert_eq!(p.state().valts, EPOCH, "C lost the timestamp it accepted at");
    assert_eq!(p.state().val, Some(9), "C lost the value it accepted");
    assert_eq!(s.trace().recoveries_with_state(), 1, "C recovered from a record");

    // And it says so on the wire, not only in its own field: the leader's READ is still being
    // retransmitted, so C answers it again after recovering.
    let after_restart =
        first_index(&s, |e| matches!(e, TraceEvent::Restarted { node, .. } if *node == C))
            .expect("C restarted");
    let answered = s.trace().events()[after_restart..].iter().any(|e| {
        matches!(
            e,
            TraceEvent::Sent { from, msg: Wire::Reply(t), .. }
                if *from == C && matches!(t.msg, Reply::StateIs { valts: EPOCH, val: Some(9) })
        )
    });
    assert!(answered, "C answered no read after recovering, so nothing observed its state");
}

#[test]
fn a_process_that_accepted_nothing_recovers_the_empty_state() {
    // The counterpart, and it is not "recovers nothing": Algorithm 5.9 stores `(valts, val)` in
    // `Init`, so every process has a record from its first event. What must be true is that the
    // record says ⊥ — a process that accepted nothing must not be read as having accepted, or a
    // later leader would adopt a value out of thin air.
    let mut s = sim(5);
    s.run_for(Duration::from_millis(5));
    assert_eq!(s.at(D).state().val, None, "nothing was proposed, so D accepted nothing");

    s.crash(D);
    s.restart(D);
    s.run_for(Duration::from_millis(5));

    let p = s.at(D);
    assert_eq!(p.state().valts, 0, "D came back claiming a timestamp");
    assert_eq!(p.state().val, None, "D came back claiming a value");
    assert_eq!(p.epoch_decision(), None, "D came back claiming a decision");
}

// ------------------------------------------------- dying inside the write: task 7.4

#[test]
fn dying_inside_the_write_never_leaves_an_acceptance_announced_without_a_record() {
    // `crash_on_next_write` kills the process *inside* the write, and the seed decides whether it
    // landed. Either outcome is allowed. What is not allowed is D having sent ACCEPT — a promise to
    // the leader — with no record of what it accepted.
    let mut landed = 0;
    let mut lost = 0;
    for seed in 0..40u64 {
        let mut s = sim(seed);
        // Past the `Init` write, so the armed one is the acceptance.
        s.step_now();
        s.crash_on_next_write(D);
        s.command(E, Cmd::Propose(9));
        s.run_for(Duration::from_millis(60));
        assert_eq!(s.trace().deaths_in_writes(), 1, "seed {seed}: D died in a write");

        // The effects of a handler that died are discarded, so the ACCEPT never escaped.
        assert!(
            first_index(&s, |e| is_accept_from(e, D)).is_none(),
            "seed {seed}: D promised an acceptance from inside the handler that died"
        );

        s.restart(D);
        let p = s.at(D);
        if p.state().val == Some(9) {
            landed += 1;
        } else {
            lost += 1;
            assert_eq!(p.state().valts, 0, "seed {seed}: lost cleanly, not half-written");
        }

        // Either way the leader is still retransmitting, so D ends up accepting, and this time with
        // a record behind it. The guarantee holds across the fault rather than in spite of it.
        settle(&mut s);
        assert_eq!(s.at(D).state().val, Some(9), "seed {seed}");
        let accept = first_index(&s, |e| is_accept_from(e, D));
        assert!(accept.is_some(), "seed {seed}: D never accepted at all");
    }
    assert!(landed > 0 && lost > 0, "both outcomes must occur: {landed} landed, {lost} lost");
}

// ------------------------------------------------- safety across faults: task 7.5

#[test]
fn agreement_holds_across_crashes_and_recoveries() {
    for seed in 0..20u64 {
        let mut s = lossy(seed);
        s.command(E, Cmd::Propose(9));
        s.run_for(Duration::from_millis(40));
        s.crash(B);
        s.run_for(Duration::from_millis(40));
        s.restart(B);
        s.crash(C);
        s.run_for(Duration::from_millis(40));
        s.restart(C);
        settle(&mut s);

        let decisions = all_decisions(&s);
        assert!(
            decisions.windows(2).all(|w| w[0] == w[1]),
            "seed {seed}: two processes decided differently: {decisions:?}"
        );
        for n in ALL {
            assert!(decided_by(&s, n).len() <= 1, "{n} decided twice on seed {seed}");
        }
    }
}

#[test]
fn a_leader_crashing_partway_through_leaves_no_two_processes_holding_different_acceptances() {
    // The leader goes down partway through the write, so some processes have accepted and some
    // have not. The instance is then stuck — an aborted epoch is the layer above's business — but
    // what it must never do is leave two processes holding *different* values at this epoch's
    // timestamp, because a later epoch's `highest(states)` would then be picking between them.
    let mut partial = 0;
    for seed in 0..30u64 {
        let mut s = lossy(seed);
        s.command(E, Cmd::Propose(9));
        s.run_for(Duration::from_millis(45));
        s.crash(E);
        settle(&mut s);

        let accepted: Vec<State<u32>> = [A, B, C, D]
            .iter()
            .filter_map(|n| s.protocol(*n))
            .map(|p| p.state().clone())
            .filter(|st| st.valts == EPOCH)
            .collect();
        assert!(
            accepted.windows(2).all(|w| w[0].val == w[1].val),
            "seed {seed}: two processes accepted different values at epoch {EPOCH}: {accepted:?}"
        );

        let decisions = all_decisions(&s);
        assert!(
            decisions.windows(2).all(|w| w[0] == w[1]),
            "seed {seed}: two processes decided differently: {decisions:?}"
        );

        if !accepted.is_empty() && accepted.len() < 4 {
            partial += 1;
        }
    }

    // The non-vacuity half. If every run either accepted nowhere or accepted everywhere, the
    // assertion above never met the case it exists for.
    assert!(partial > 0, "no run left the write partly applied, so nothing was tested");
}

#[test]
fn what_this_instance_sends_carries_its_own_epoch() {
    let mut s = sim(7);
    s.command(E, Cmd::Propose(9));
    settle(&mut s);

    let mut seen = 0;
    for (_, _, msg) in s.trace().sends() {
        let ets = match msg {
            Wire::Announce(t) => t.ets,
            Wire::Reply(t) => t.ets,
        };
        assert_eq!(ets, EPOCH, "a message left this instance stamped for another epoch");
        seen += 1;
    }
    assert!(seen > 0, "nothing was sent");
}

#[test]
fn traffic_for_another_epoch_is_dropped() {
    // Safety, not tidiness: a `WRITE` from epoch 7 acted on by an instance at epoch 11 would record
    // an acceptance at timestamp 11 that never happened, and a later read would find it. Driven
    // directly, because the simulator has no way to hand a process a message nobody sent.
    use rand::SeedableRng;
    let mut p = LoggedEpochConsensus::<u32>::new(A, ALL, EPOCH, E, State::default(), retransmit());
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(0);
    let mut store = MemStore::default();
    let mut timers = 0;

    let foreign = Wire::Announce(Tagged { ets: EPOCH + 4, msg: Announce::Write { val: 3 } });
    let fx = step_with(
        &mut p,
        Event::Msg { from: E, msg: foreign },
        Time::ZERO,
        &mut rng,
        &mut store,
        &mut timers,
    );
    assert!(fx.is_empty(), "another epoch's write produced {fx:?}");
    assert_eq!(*p.state(), State::default(), "A acted on another epoch's write");

    // Non-vacuity: the same message stamped for this epoch is acted on.
    let mine = Wire::Announce(Tagged { ets: EPOCH, msg: Announce::Write { val: 3 } });
    let fx = step_with(
        &mut p,
        Event::Msg { from: E, msg: mine },
        Time::ZERO,
        &mut rng,
        &mut store,
        &mut timers,
    );
    assert!(
        !fx.is_empty(),
        "this epoch's write produced nothing, so the guard above proves nothing"
    );
    assert_eq!(p.state().valts, EPOCH);
    assert_eq!(p.state().val, Some(3));
}

// ------------------------------------------------- bounded by membership, not by time

#[test]
fn the_send_rate_does_not_grow_after_the_epoch_has_decided() {
    // Measured before the guards: 12.6k, 28.6k, 44.6k, 60.6k, 76.6k sends in successive 400 ms
    // windows — every redelivered `READ` and `WRITE` answered on a fresh stubborn transmission,
    // for ever. Answering once makes the set the stubborn children retransmit a fixed one.
    let mut s = sim(20);
    s.command(E, Cmd::Propose(9));
    settle(&mut s);
    assert_eq!(decided_by(&s, A), vec![9], "decided first, so the rate measured is the idle one");
    assert_send_rate_flat!(s, Duration::from_millis(400), 4);
}

#[test]
fn a_redelivered_read_or_write_is_not_answered_again() {
    // The mechanism behind the rate, asserted directly: one `STATE` and one `ACCEPT` from each
    // follower to the leader, however many times the announcements come round.
    let mut s = sim(21);
    s.command(E, Cmd::Propose(9));
    settle(&mut s);

    let redelivered = s
        .trace()
        .deliveries()
        .filter(|(_, to, m)| {
            *to == A && matches!(m, Wire::Announce(t) if matches!(t.msg, Announce::Read))
        })
        .count();
    assert!(
        redelivered > 3,
        "the READ reached A only {redelivered} times, so nothing was repeated"
    );

    // Distinct replies, not transmissions: the stubborn link retransmits the one reply many times,
    // which is what it is for. What must not happen is a *second* reply.
    let distinct = |kind: fn(&Reply<u32>) -> bool| {
        s.trace()
            .sends()
            .filter_map(|(from, to, m)| match m {
                Wire::Reply(t) if from == A && to == E && kind(&t.msg) => Some(format!("{t:?}")),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<String>>()
            .len()
    };
    assert_eq!(distinct(|r| matches!(r, Reply::StateIs { .. })), 1, "A answered READ twice");
    assert_eq!(distinct(|r| matches!(r, Reply::Accept)), 1, "A answered WRITE twice");
}
