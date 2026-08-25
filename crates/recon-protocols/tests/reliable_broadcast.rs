//! Reliable broadcast against Module 3.2: validity, no duplication, no creation, and the
//! property that distinguishes this rung — agreement when the sender crashes.

use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, NodeId, Time, step};
use recon_protocols::best_effort_broadcast::{self as beb, BestEffortBroadcast};
use recon_protocols::reliable_broadcast::{BroadcastId, Cmd, Data, Ind, ReliableBroadcast, Wire};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

fn interval() -> Duration {
    Duration::from_millis(10)
}

fn sim(config: Config) -> Sim<ReliableBroadcast<u32>> {
    Sim::new(config, &ALL, |me| ReliableBroadcast::new(me, ALL, interval()))
}

fn rng() -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(0)
}

fn delivered(s: &Sim<ReliableBroadcast<u32>>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace().indications_at(node).map(|Ind::Deliver { from, msg }| (*from, *msg)).collect()
}

// ------------------------------------------------------------ the wire: task 1.1

#[test]
fn the_wire_carries_the_originator() {
    let mut p: ReliableBroadcast<u32> = ReliableBroadcast::new(A, ALL, interval());
    let fx = step(&mut p, Event::Cmd(Cmd::Broadcast(9u32)), Time::ZERO, &mut rng());

    let sends: Vec<Wire<u32>> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Send { msg, .. } => Some(msg.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(sends.len(), 4, "one per process, the sender included");
    for w in &sends {
        assert_eq!(w.payload.id.origin, A);
        assert_eq!(w.payload.id.seq, 1);
        assert_eq!(w.payload.payload, 9);
    }
}

#[test]
fn the_wire_survives_encoding() {
    let d = Data { id: BroadcastId { origin: A, seq: 3 }, payload: 7u32 };
    assert_eq!(recon_sim::codec::round_trip(&d).expect("round trip"), d);
}

// -------------------------------------------- deliver once, relay once: tasks 1.2, 1.3

#[test]
fn a_first_receipt_delivers_and_relays() {
    let mut p: ReliableBroadcast<u32> = ReliableBroadcast::new(B, ALL, interval());
    let mut r = rng();

    // Arrive at B by way of the full stack from A.
    let mut a: ReliableBroadcast<u32> = ReliableBroadcast::new(A, ALL, interval());
    let from_a = step(&mut a, Event::Cmd(Cmd::Broadcast(5u32)), Time::ZERO, &mut r);
    let to_b = from_a
        .iter()
        .find_map(|e| match e {
            Effect::Send { to, msg } if *to == B => Some(msg.clone()),
            _ => None,
        })
        .expect("a message addressed to B");

    let fx = step(&mut p, Event::Msg { from: A, msg: to_b }, Time::from_millis(1), &mut r);

    let indications = fx.iter().filter(|e| matches!(e, Effect::Indicate(_))).count();
    let sends = fx.iter().filter(|e| matches!(e, Effect::Send { .. })).count();
    assert_eq!(indications, 1, "delivered once");
    assert!(sends >= 4, "and relayed to every process, saw {sends}");
    assert_eq!(p.delivered_count(), 1);
}

#[test]
fn a_repeat_receipt_neither_delivers_nor_relays() {
    // Termination: without this the relay would feed itself for ever.
    let mut a: ReliableBroadcast<u32> = ReliableBroadcast::new(A, ALL, interval());
    let mut r = rng();
    let from_a = step(&mut a, Event::Cmd(Cmd::Broadcast(5u32)), Time::ZERO, &mut r);
    let to_b = from_a
        .iter()
        .find_map(|e| match e {
            Effect::Send { to, msg } if *to == B => Some(msg.clone()),
            _ => None,
        })
        .expect("a message addressed to B");

    let mut p: ReliableBroadcast<u32> = ReliableBroadcast::new(B, ALL, interval());
    let _ = step(&mut p, Event::Msg { from: A, msg: to_b.clone() }, Time::ZERO, &mut r);
    let second = step(&mut p, Event::Msg { from: A, msg: to_b }, Time::from_millis(1), &mut r);

    assert!(!second.iter().any(|e| matches!(e, Effect::Indicate(_))), "no second delivery");
    assert!(
        !second.iter().any(|e| matches!(e, Effect::Send { .. })),
        "and no second relay — this is what makes the relay terminate"
    );
}

#[test]
fn a_run_terminates_rather_than_relaying_for_ever() {
    let mut s = sim(Config::default().seed(1).max_steps(200_000));
    s.command(A, Cmd::Broadcast(1));
    s.run_until(Time::from_millis(400));
    for n in ALL {
        assert_eq!(delivered(&s, n).len(), 1, "{n} delivered more than once");
    }
}

// ------------------------------- validity, no duplication, no creation: tasks 2.1, 2.2

#[test]
fn a_correct_sender_delivers_its_own_broadcast() {
    let mut s = sim(Config::default().seed(2).loss(0.4));
    s.command(A, Cmd::Broadcast(3));
    s.run_until(Time::from_millis(3000));
    assert_eq!(delivered(&s, A), vec![(A, 3)]);
}

#[test]
fn every_process_delivers_each_broadcast_exactly_once() {
    let mut s = sim(Config::default().seed(3).loss(0.4).duplication(0.4));
    for i in 0..4u32 {
        s.command(A, Cmd::Broadcast(i));
    }
    s.run_until(Time::from_millis(6000));

    assert!(s.trace().duplicates() > 0, "the network must actually have duplicated");
    for n in ALL {
        let mut got: Vec<u32> = delivered(&s, n).into_iter().map(|(_, m)| m).collect();
        got.sort();
        assert_eq!(got, (0..4).collect::<Vec<_>>(), "{n} saw the wrong multiset");
    }
}

#[test]
fn identical_content_broadcast_twice_is_delivered_twice() {
    let mut s = sim(Config::default().seed(4));
    s.command(A, Cmd::Broadcast(42));
    s.command(A, Cmd::Broadcast(42));
    s.run_until(Time::from_millis(1000));
    for n in ALL {
        assert_eq!(delivered(&s, n).len(), 2, "{n} must deliver both");
    }
}

#[test]
fn a_relayed_message_is_attributed_to_its_originator() {
    // C can only learn A's message via a relay once A is gone, and must still name A.
    let mut s = sim(Config::default().seed(5));
    s.partition(&[&[A, B], &[C, D]]);
    s.command(A, Cmd::Broadcast(77));
    s.run_until(Time::from_millis(200));
    assert!(delivered(&s, C).is_empty(), "C is cut off");

    s.crash(A);
    s.heal();
    s.run_until(Time::from_millis(3000));

    assert_eq!(delivered(&s, C), vec![(A, 77)], "attributed to A, not to B who relayed it");
    assert_eq!(delivered(&s, D), vec![(A, 77)]);
}

#[test]
fn nothing_is_delivered_that_was_not_broadcast() {
    let mut s = sim(Config::default().seed(6).duplication(0.5));
    s.run_until(Time::from_millis(1000));
    assert_eq!(s.trace().indication_count(), 0);
}

// ------------------------------------------------------------ agreement: task 2.3

/// Broadcast, let it reach some processes, crash the sender, and see whether the rest catch up.
fn sender_crash_outcome(seed: u64, settle: Duration) -> Vec<usize> {
    let mut s = sim(Config::default()
        .seed(seed)
        .loss(0.5)
        .latency(Duration::from_millis(2), Duration::from_millis(20)));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(settle);
    s.crash(A);
    s.run_until(Time::from_millis(8000));
    [B, C, D].iter().map(|n| delivered(&s, *n).len()).collect()
}

#[test]
fn agreement_holds_when_the_sender_crashes_partway() {
    for seed in 0..40u64 {
        let counts = sender_crash_outcome(seed, Duration::from_millis(12));
        let any = counts.iter().any(|c| *c > 0);
        let all = counts.iter().all(|c| *c == 1);
        assert!(
            !any || all,
            "seed {seed}: agreement violated — some delivered and some did not: {counts:?}"
        );
    }
}

#[test]
fn agreement_is_not_vacuous_some_seed_actually_delivers() {
    let delivering = (0..40u64)
        .filter(|s| sender_crash_outcome(*s, Duration::from_millis(12)).iter().any(|c| *c > 0))
        .count();
    assert!(
        delivering > 0,
        "if no seed ever delivers, the agreement test above is passing vacuously"
    );
}

// --------------------------- the test must distinguish the two rungs: task 2.4

/// The same scenario against best-effort broadcast, which makes no agreement promise.
fn beb_sender_crash_outcome(seed: u64, settle: Duration) -> Vec<usize> {
    let mut s: Sim<BestEffortBroadcast<u32>> = Sim::new(
        Config::default()
            .seed(seed)
            .loss(0.5)
            .latency(Duration::from_millis(2), Duration::from_millis(20)),
        &ALL,
        |me| BestEffortBroadcast::new(me, ALL, interval()),
    );
    s.command(A, beb::Cmd::Broadcast(1));
    s.run_for(settle);
    s.crash(A);
    s.run_until(Time::from_millis(8000));
    [B, C, D].iter().map(|n| s.trace().indications_at(*n).count()).collect()
}

#[test]
fn best_effort_broadcast_does_violate_agreement_under_the_same_test() {
    // Without this the agreement test could be passing for reasons unrelated to the algorithm.
    let split = (0..60u64).find(|seed| {
        let c = beb_sender_crash_outcome(*seed, Duration::from_millis(12));
        c.iter().any(|x| *x > 0) && c.contains(&0)
    });
    assert!(
        split.is_some(),
        "the test does not distinguish the rungs: best-effort broadcast never disagreed"
    );
}

#[test]
fn reliable_broadcast_survives_the_seed_that_splits_best_effort() {
    let split = (0..60u64)
        .find(|seed| {
            let c = beb_sender_crash_outcome(*seed, Duration::from_millis(12));
            c.iter().any(|x| *x > 0) && c.contains(&0)
        })
        .expect("a splitting seed");

    let counts = sender_crash_outcome(split, Duration::from_millis(12));
    let any = counts.iter().any(|c| *c > 0);
    let all = counts.iter().all(|c| *c == 1);
    assert!(!any || all, "seed {split} splits best-effort but must not split reliable: {counts:?}");
}

#[test]
fn agreement_holds_through_partition_and_healing() {
    for seed in 0..12u64 {
        let mut s = sim(Config::default().seed(seed).loss(0.3));
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(30));
        s.partition(&[&[A, B], &[C, D]]);
        s.run_for(Duration::from_millis(500));
        s.crash(A);
        s.heal();
        s.run_until(Time::from_millis(8000));

        let counts: Vec<usize> = [B, C, D].iter().map(|n| delivered(&s, *n).len()).collect();
        let any = counts.iter().any(|c| *c > 0);
        let all = counts.iter().all(|c| *c == 1);
        assert!(!any || all, "seed {seed}: {counts:?}");
    }
}
