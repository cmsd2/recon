//! Read/write epoch consensus against Module 5.6, driven directly — before anything composes over
//! it, because this is where Paxos's safety argument lives.

use core::time::Duration;
use recon_core::{Effect, Event, MemStore, NodeId, Time, step_with};
use recon_protocols::epoch_consensus::{Cmd, EpochConsensus, EpochMsg, Ind, State, Tagged};
use recon_protocols::perfect_link as pl;
use recon_sim::{Config as SimConfig, Sim};

mod common;
use common::*;

type Ep = EpochConsensus<u32>;
type Fx = Vec<Effect<pl::Wire<Tagged<u32>>, Ind<u32>>>;

/// An instance for epoch `ets` led by `leader`, beginning from `state`.
fn ep(me: NodeId, ets: u64, leader: NodeId, state: State<u32>) -> Ep {
    EpochConsensus::new(me, ALL, ets, leader, state, retransmit())
}

fn fresh(me: NodeId, ets: u64, leader: NodeId) -> Ep {
    ep(me, ets, leader, State::default())
}

fn rng() -> rand_chacha::ChaCha8Rng {
    use rand::SeedableRng;
    rand_chacha::ChaCha8Rng::seed_from_u64(0)
}

fn store() -> MemStore<core::convert::Infallible, core::convert::Infallible> {
    MemStore::default()
}

/// Drive one event and return the effects.
fn drive(p: &mut Ep, ev: Event<Cmd<u32>, pl::Wire<Tagged<u32>>, core::convert::Infallible>) -> Fx {
    step_with(p, ev, Time::ZERO, &mut rng(), &mut store(), &mut 0)
}

/// Wrap an epoch message as it arrives from `from`, stamped for epoch 7 — the epoch every instance
/// in this file is in, unless a test says otherwise.
fn arriving(from: NodeId, seq: u64, msg: EpochMsg<u32>) -> pl::Wire<Tagged<u32>> {
    stamped(from, seq, 7, msg)
}

/// The same, for an arbitrary epoch, so the instance guard can be tested.
fn stamped(from: NodeId, seq: u64, ets: u64, msg: EpochMsg<u32>) -> pl::Wire<Tagged<u32>> {
    pl::Wire { id: pl::MsgId { src: from, seq }, payload: Tagged { ets, msg } }
}

/// Every epoch message these effects send, with its destination.
fn sent(fx: &Fx) -> Vec<(NodeId, EpochMsg<u32>)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Send { to, msg } => Some((*to, msg.payload.msg.clone())),
            _ => None,
        })
        .collect()
}

fn indications(fx: &Fx) -> Vec<Ind<u32>> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Indicate(i) => Some(i.clone()),
            _ => None,
        })
        .collect()
}

// ------------------------------------------------- Only the leader drives: task 4.6

#[test]
fn a_follower_asked_to_propose_initiates_nothing() {
    // `// only leader ℓ`. A follower that read on its own behalf would be a second leader for the
    // epoch, which is the thing the whole structure exists to prevent.
    let mut p = fresh(B, 7, A);
    let fx = drive(&mut p, Event::Cmd(Cmd::Propose(9)));
    assert!(sent(&fx).is_empty(), "a follower initiated something: {:?}", sent(&fx));
}

#[test]
fn the_leader_asked_to_propose_reads() {
    let mut p = fresh(A, 7, A);
    let fx = drive(&mut p, Event::Cmd(Cmd::Propose(9)));
    let out = sent(&fx);
    assert_eq!(out.len(), ALL.len(), "READ goes to everyone, including itself");
    assert!(out.iter().all(|(_, m)| *m == EpochMsg::Read), "{out:?}");
}

#[test]
fn a_read_is_answered_with_this_process_state() {
    let mut p = ep(B, 7, A, State { valts: 3, val: Some(42) });
    let fx = drive(&mut p, Event::Msg { from: A, msg: arriving(A, 1, EpochMsg::Read) });
    assert_eq!(
        sent(&fx),
        vec![(A, EpochMsg::StateIs { valts: 3, val: Some(42) })],
        "answered to the leader alone, with what this process holds"
    );
}

#[test]
fn a_read_from_someone_who_is_not_the_leader_is_ignored() {
    let mut p = ep(B, 7, A, State { valts: 3, val: Some(42) });
    let fx = drive(&mut p, Event::Msg { from: C, msg: arriving(C, 1, EpochMsg::Read) });
    assert!(sent(&fx).is_empty(), "C does not lead epoch 7: {:?}", sent(&fx));
}

// ------------------------------------------------- The two majorities: tasks 4.1 to 4.3

/// Drive the leader through a full round, feeding it `states` from followers and then acceptances.
/// Returns the value it writes and the value it decides.
fn full_round(proposal: u32, states: &[(NodeId, State<u32>)]) -> (Option<u32>, Option<u32>) {
    let mut p = fresh(A, 7, A);
    drive(&mut p, Event::Cmd(Cmd::Propose(proposal)));

    let mut written = None;
    for (i, (from, st)) in states.iter().enumerate() {
        let msg = EpochMsg::StateIs { valts: st.valts, val: st.val };
        let fx = drive(&mut p, Event::Msg { from: *from, msg: arriving(*from, i as u64 + 1, msg) });
        for (_, m) in sent(&fx) {
            if let EpochMsg::Write { val } = m {
                written = Some(val);
            }
        }
    }

    let mut decided = None;
    for (i, from) in ALL.iter().enumerate() {
        let fx = drive(
            &mut p,
            Event::Msg { from: *from, msg: arriving(*from, i as u64 + 100, EpochMsg::Accept) },
        );
        for (_, m) in sent(&fx) {
            if let EpochMsg::Decided { val } = m {
                decided = Some(val);
            }
        }
    }
    (written, decided)
}

#[test]
fn a_leader_reading_nothing_written_writes_its_own_proposal() {
    let empty: Vec<(NodeId, State<u32>)> =
        ALL.iter().map(|n| (*n, State::default())).take(3).collect();
    let (written, decided) = full_round(9, &empty);
    assert_eq!(written, Some(9), "nobody had accepted anything, so the proposal stands");
    assert_eq!(decided, Some(9));
}

#[test]
fn a_value_already_accepted_displaces_the_leaders_proposal() {
    // `if v ≠ ⊥ then tmpval := v` — the line the whole algorithm turns on, and the one an OCR of
    // the page renders as `=`, which would invert it and break safety outright.
    let states =
        vec![(A, State::default()), (B, State { valts: 4, val: Some(77) }), (C, State::default())];
    let (written, decided) = full_round(9, &states);
    assert_eq!(written, Some(77), "the leader must adopt what a higher epoch already accepted");
    assert_eq!(decided, Some(77), "and decide it, not its own proposal");
}

#[test]
fn the_highest_timestamp_wins_among_several_accepted() {
    // `highest` is highest *timestamp*, not highest value — the state written most recently, which
    // is the one that may already have been decided.
    let states = vec![
        (A, State { valts: 2, val: Some(500) }),
        (B, State { valts: 6, val: Some(11) }),
        (C, State { valts: 4, val: Some(300) }),
    ];
    let (written, _) = full_round(9, &states);
    assert_eq!(written, Some(11), "timestamp 6 wins, though 11 is the smallest value");
}

#[test]
fn a_minority_of_states_does_not_produce_a_write() {
    // `upon #(states) > N/2`. Two of five is not a majority.
    let mut p = fresh(A, 7, A);
    drive(&mut p, Event::Cmd(Cmd::Propose(9)));
    let mut wrote = false;
    for (i, from) in [B, C].iter().enumerate() {
        let msg = EpochMsg::StateIs { valts: 0, val: None };
        let fx = drive(&mut p, Event::Msg { from: *from, msg: arriving(*from, i as u64 + 1, msg) });
        wrote |= sent(&fx).iter().any(|(_, m)| matches!(m, EpochMsg::Write { .. }));
    }
    assert!(!wrote, "two of five is not a majority");
}

#[test]
fn a_minority_of_acceptances_does_not_produce_a_decision() {
    // `upon accepted > N/2`. This is the majority whose intersection with a later epoch's read is
    // what makes two epochs agree.
    let mut p = fresh(A, 7, A);
    drive(&mut p, Event::Cmd(Cmd::Propose(9)));
    for (i, from) in ALL.iter().take(3).enumerate() {
        let msg = EpochMsg::StateIs { valts: 0, val: None };
        drive(&mut p, Event::Msg { from: *from, msg: arriving(*from, i as u64 + 1, msg) });
    }
    let mut decided = false;
    for (i, from) in [B, C].iter().enumerate() {
        let fx = drive(
            &mut p,
            Event::Msg { from: *from, msg: arriving(*from, i as u64 + 100, EpochMsg::Accept) },
        );
        decided |= sent(&fx).iter().any(|(_, m)| matches!(m, EpochMsg::Decided { .. }));
    }
    assert!(!decided, "two acceptances of five is not a majority");
}

#[test]
fn a_write_is_accepted_at_this_epochs_timestamp() {
    // `(valts, val) := (ets, v)`. The timestamp recorded is the *epoch's*, which is what lets a
    // later epoch tell how recent this acceptance was.
    let mut p = fresh(B, 7, A);
    let msg = EpochMsg::Write { val: 42 };
    let fx = drive(&mut p, Event::Msg { from: A, msg: arriving(A, 1, msg) });
    assert_eq!(sent(&fx), vec![(A, EpochMsg::Accept)]);
    assert_eq!(*p.state(), State { valts: 7, val: Some(42) }, "at the epoch's timestamp, not 0");
}

#[test]
fn a_decision_is_what_the_leader_announced() {
    let mut p = fresh(B, 7, A);
    let msg = EpochMsg::Decided { val: 42 };
    let fx = drive(&mut p, Event::Msg { from: A, msg: arriving(A, 1, msg) });
    assert_eq!(indications(&fx), vec![Ind::Decide(42)]);
}

// ------------------------------------------------- The abort handshake: tasks 4.4, 4.5

#[test]
fn abandoning_yields_the_state_this_process_accepted() {
    let mut p = fresh(B, 7, A);
    drive(&mut p, Event::Msg { from: A, msg: arriving(A, 1, EpochMsg::Write { val: 42 }) });

    let fx = drive(&mut p, Event::Cmd(Cmd::Abort));
    assert_eq!(
        indications(&fx),
        vec![Ind::Aborted(State { valts: 7, val: Some(42) })],
        "the state is what the next epoch begins from, so losing it loses the safety property"
    );
    assert!(p.is_aborted());
}

#[test]
fn an_abandoned_instance_is_silent() {
    // `halt;  // stop operating when aborted`. An instance that kept answering would be a second
    // leader for an epoch that has moved on: it could still gather a quorum and decide, while the
    // epoch replacing it decided something else. This is a safety bug, not a liveness one.
    let mut p = fresh(A, 7, A);
    drive(&mut p, Event::Cmd(Cmd::Propose(9)));
    drive(&mut p, Event::Cmd(Cmd::Abort));

    for (i, msg) in [
        EpochMsg::Read,
        EpochMsg::StateIs { valts: 9, val: Some(1) },
        EpochMsg::Write { val: 5 },
        EpochMsg::Accept,
        EpochMsg::Decided { val: 5 },
    ]
    .into_iter()
    .enumerate()
    {
        let fx = drive(&mut p, Event::Msg { from: B, msg: arriving(B, i as u64 + 1, msg.clone()) });
        assert!(fx.is_empty(), "an aborted instance answered {msg:?} with {fx:?}");
    }

    let fx = drive(&mut p, Event::Cmd(Cmd::Propose(1)));
    assert!(fx.is_empty(), "and it does not propose either");
}

#[test]
fn an_abandoned_instance_does_not_decide_on_a_quorum_that_was_already_in_flight() {
    // The window that matters: the leader has a majority of acceptances arriving when it is
    // abandoned. Without `halt` it would announce a decision for an epoch nobody is in any more.
    let mut p = fresh(A, 7, A);
    drive(&mut p, Event::Cmd(Cmd::Propose(9)));
    for (i, from) in ALL.iter().take(3).enumerate() {
        let msg = EpochMsg::StateIs { valts: 0, val: None };
        drive(&mut p, Event::Msg { from: *from, msg: arriving(*from, i as u64 + 1, msg) });
    }
    drive(&mut p, Event::Msg { from: B, msg: arriving(B, 100, EpochMsg::Accept) });
    drive(&mut p, Event::Cmd(Cmd::Abort));

    let mut announced = false;
    for (i, from) in [C, D, E].iter().enumerate() {
        let fx = drive(
            &mut p,
            Event::Msg { from: *from, msg: arriving(*from, i as u64 + 200, EpochMsg::Accept) },
        );
        announced |= sent(&fx).iter().any(|(_, m)| matches!(m, EpochMsg::Decided { .. }));
    }
    assert!(!announced, "an abandoned leader announced a decision after being told to stop");
}

// ------------------------------------------------- Lock-in, end to end: task 4.3

#[test]
fn a_value_decided_in_one_epoch_is_what_a_later_epoch_decides() {
    // The intersection argument, run as a whole: epoch 7 decides, and epoch 11 — with a different
    // leader proposing something else — reads a majority and adopts what 7 decided.
    let mut first: Sim<Ep> = Sim::new(SimConfig::default().seed(1), &ALL, |me| fresh(me, 7, A));
    first.command(A, Cmd::Propose(9));
    first.run_for(Duration::from_millis(500));

    let decided: Vec<u32> = ALL
        .iter()
        .filter_map(|n| {
            first.trace().indications_at(*n).find_map(|i| match i {
                Ind::Decide(v) => Some(*v),
                _ => None,
            })
        })
        .collect();
    assert_eq!(decided, vec![9; ALL.len()], "epoch 7 decided 9 everywhere");

    // Carry each process's accepted state into a new epoch with a different leader, which proposes
    // something else. It must adopt 9 regardless.
    let states: std::collections::BTreeMap<NodeId, State<u32>> =
        ALL.iter().map(|n| (*n, first.protocol(*n).expect("exists").state().clone())).collect();
    let mut second: Sim<Ep> =
        Sim::new(SimConfig::default().seed(2), &ALL, move |me| ep(me, 11, B, states[&me].clone()));
    second.command(B, Cmd::Propose(1_000));
    second.run_for(Duration::from_millis(500));

    for n in ALL {
        let got: Vec<u32> = second
            .trace()
            .indications_at(n)
            .filter_map(|i| match i {
                Ind::Decide(v) => Some(*v),
                _ => None,
            })
            .collect();
        assert_eq!(got, vec![9], "{n} must decide 9 again, not B's 1000");
    }
}

// ------------------------------------------------- The instance guard

#[test]
fn traffic_for_another_epoch_is_dropped() {
    // `such that ts = ets`. A `WRITE` from a superseded epoch reaching this instance would be
    // recorded at *this* epoch's timestamp — inventing an acceptance that never happened, which a
    // later epoch's read would then treat as the most recent thing anyone accepted. A safety
    // failure, not a lost message.
    let mut p = fresh(B, 11, A);
    let stale = stamped(A, 1, 7, EpochMsg::Write { val: 42 });
    let fx = drive(&mut p, Event::Msg { from: A, msg: stale });

    assert!(sent(&fx).is_empty(), "a message for epoch 7 was answered by the epoch 11 instance");
    assert_eq!(*p.state(), State::default(), "and nothing was recorded: {:?}", p.state());
}

#[test]
fn traffic_for_this_epoch_is_not_dropped() {
    // Non-vacuity for the guard: the same message, correctly stamped, is acted on. Without this
    // the test above would pass on an instance that ignored everything.
    let mut p = fresh(B, 11, A);
    let current = stamped(A, 1, 11, EpochMsg::Write { val: 42 });
    let fx = drive(&mut p, Event::Msg { from: A, msg: current });

    assert_eq!(sent(&fx), vec![(A, EpochMsg::Accept)]);
    assert_eq!(*p.state(), State { valts: 11, val: Some(42) });
}

#[test]
fn what_this_instance_sends_carries_its_own_epoch() {
    let mut p = fresh(A, 11, A);
    let fx = drive(&mut p, Event::Cmd(Cmd::Propose(9)));
    let stamps: Vec<u64> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Send { msg, .. } => Some(msg.payload.ets),
            _ => None,
        })
        .collect();
    assert!(!stamps.is_empty());
    assert!(stamps.iter().all(|t| *t == 11), "every send is stamped for epoch 11: {stamps:?}");
}

// ------------------------------------------------- bounded by membership, not by time

#[test]
fn the_send_rate_does_not_grow_after_the_epoch_has_decided() {
    // One epoch, decided, then left running. What the perfect links beneath retransmit is fixed
    // once the last message of the protocol has been sent, so the rate must be flat.
    let mut s: Sim<Ep> =
        Sim::new(SimConfig::default().seed(20).synchronous(BOUND), &ALL, |me| fresh(me, 7, E));
    s.command(E, Cmd::Propose(9));
    s.run_for(Duration::from_millis(200));
    assert_send_rate_flat!(s, Duration::from_millis(200), 4);
}
