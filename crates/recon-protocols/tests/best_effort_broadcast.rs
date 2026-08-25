//! Best-effort broadcast against its stated guarantees (Module 3.1): best-effort validity,
//! no duplication, no creation.

use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, NodeId, Time, step};
use recon_protocols::best_effort_broadcast::{BestEffortBroadcast, Cmd, Ind};
use recon_protocols::perfect_link::Wire;
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

fn interval() -> Duration {
    Duration::from_millis(10)
}

fn beb() -> BestEffortBroadcast<u32> {
    BestEffortBroadcast::new(A, ALL, interval())
}

fn sim(config: Config) -> Sim<BestEffortBroadcast<u32>> {
    Sim::new(config, &ALL, |me| BestEffortBroadcast::new(me, ALL, interval()))
}

fn rng() -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(0)
}

/// What `node` delivered, in order.
fn delivered(s: &Sim<BestEffortBroadcast<u32>>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace().indications_at(node).map(|Ind::Deliver { from, msg }| (*from, *msg)).collect()
}

// -------------------------------------------------- No wire fields: task 6.1

#[test]
fn the_layer_contributes_no_wire_fields_of_its_own() {
    let mut p = beb();
    let fx = step(&mut p, Event::Cmd(Cmd::Broadcast(9u32)), Time::ZERO, &mut rng());

    // Every outgoing message is the perfect link's Wire, unchanged — no broadcast header.
    let sends: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Send { to, msg } => Some((*to, msg.clone())),
            _ => None,
        })
        .collect();

    assert_eq!(sends.len(), 4, "one message per process in the system");
    for (to, msg) in &sends {
        let _: &Wire<u32> = msg;
        assert_eq!(msg.payload, 9, "the payload travels unwrapped by this layer");
        assert_eq!(msg.id.src, A);
        assert!(ALL.contains(to));
    }

    // Each fan-out target gets a distinct identifier from the link below.
    let mut seqs: Vec<u64> = sends.iter().map(|(_, m)| m.id.seq).collect();
    seqs.sort();
    assert_eq!(seqs, vec![1, 2, 3, 4]);
}

#[test]
fn a_broadcast_reaches_every_process_including_the_sender() {
    let mut p = beb();
    let fx = step(&mut p, Event::Cmd(Cmd::Broadcast(1u32)), Time::ZERO, &mut rng());
    let targets: Vec<NodeId> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Send { to, .. } => Some(*to),
            _ => None,
        })
        .collect();
    assert!(targets.contains(&A), "Π includes the sender; self-delivery is not a special case");
    for n in ALL {
        assert!(targets.contains(&n));
    }
}

// ------------------------------------------ Best-effort validity: task 6.2

#[test]
fn a_correct_sender_reaches_everyone() {
    let mut s = sim(Config::default().seed(1).loss(0.5));
    s.command(A, Cmd::Broadcast(7));
    s.run_until(Time::from_millis(2000));

    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 7)], "{n} must deliver exactly once");
    }
}

#[test]
fn the_sender_delivers_to_itself() {
    let mut s = sim(Config::default().seed(2));
    s.command(A, Cmd::Broadcast(3));
    s.run_until(Time::from_millis(500));
    assert_eq!(delivered(&s, A), vec![(A, 3)]);
}

#[test]
fn a_correct_sender_reaches_survivors_when_others_have_crashed() {
    let mut s = sim(Config::default().seed(3).loss(0.3));
    s.crash(C);
    s.crash(D);
    s.command(A, Cmd::Broadcast(5));
    s.run_until(Time::from_millis(2000));

    assert_eq!(delivered(&s, A), vec![(A, 5)]);
    assert_eq!(delivered(&s, B), vec![(A, 5)]);
    assert!(delivered(&s, C).is_empty(), "a crashed process delivers nothing");
    assert!(delivered(&s, D).is_empty());
}

#[test]
fn several_broadcasts_from_several_senders_all_arrive() {
    let mut s = sim(Config::default()
        .seed(4)
        .loss(0.4)
        .duplication(0.2)
        .latency(Duration::from_millis(1), Duration::from_millis(15)));
    s.command(A, Cmd::Broadcast(10));
    s.command(B, Cmd::Broadcast(20));
    s.command(C, Cmd::Broadcast(30));
    s.run_until(Time::from_millis(4000));

    for n in ALL {
        let mut got = delivered(&s, n);
        got.sort();
        assert_eq!(got, vec![(A, 10), (B, 20), (C, 30)], "{n} saw the wrong set");
    }
}

// ------------------------------ No duplication and no creation: task 6.3

#[test]
fn each_process_delivers_each_broadcast_exactly_once() {
    let mut s = sim(Config::default().seed(5).loss(0.4).duplication(0.6));
    for i in 0..5u32 {
        s.command(A, Cmd::Broadcast(i));
    }
    s.run_until(Time::from_millis(4000));

    assert!(s.trace().duplicates() > 0, "the network must actually have duplicated");
    for n in ALL {
        let mut got: Vec<u32> = delivered(&s, n).into_iter().map(|(_, m)| m).collect();
        got.sort();
        assert_eq!(got, (0..5).collect::<Vec<_>>(), "{n} must see each broadcast once");
    }
}

#[test]
fn nothing_is_delivered_that_was_not_broadcast() {
    let mut s = sim(Config::default().seed(6).loss(0.3).duplication(0.3));
    s.command(A, Cmd::Broadcast(100));
    s.command(B, Cmd::Broadcast(200));
    s.run_until(Time::from_millis(3000));

    let allowed = [(A, 100u32), (B, 200u32)];
    for n in ALL {
        for d in delivered(&s, n) {
            assert!(allowed.contains(&d), "{n} delivered {d:?}, which was never broadcast");
        }
    }
}

#[test]
fn nothing_is_delivered_when_nothing_is_broadcast() {
    let mut s = sim(Config::default().seed(7).duplication(0.5));
    s.run_until(Time::from_millis(1000));
    assert_eq!(s.trace().indication_count(), 0);
}

// ---------------------------------------- A crashed sender: task 6.4

#[test]
fn a_sender_crashing_partway_violates_nothing() {
    // The guarantee this abstraction deliberately does not make. Some processes may deliver
    // and others may not; what must still hold is no duplication and no creation.
    let mut s = sim(Config::default()
        .seed(8)
        .loss(0.7)
        .latency(Duration::from_millis(5), Duration::from_millis(20)));
    s.command(A, Cmd::Broadcast(42));
    s.run_for(Duration::from_millis(12));
    s.crash(A);
    s.run_until(Time::from_millis(3000));

    let mut any = false;
    for n in ALL {
        let got = delivered(&s, n);
        // No duplication: at most one delivery of the single broadcast.
        assert!(got.len() <= 1, "{n} delivered {} times", got.len());
        // No creation: whatever arrived was what was broadcast.
        for d in &got {
            assert_eq!(*d, (A, 42));
        }
        any |= !got.is_empty();
    }
    assert!(any, "the test is only meaningful if something got through before the crash");
}

#[test]
fn partial_delivery_after_a_crash_is_permitted() {
    // Find a seed where the crash genuinely splits the outcome, and confirm no assertion fires.
    let mut split_seen = false;
    for seed in 0..40u64 {
        let mut s = sim(Config::default()
            .seed(seed)
            .loss(0.75)
            .latency(Duration::from_millis(5), Duration::from_millis(25)));
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(8));
        s.crash(A);
        s.run_until(Time::from_millis(2000));

        let counts: Vec<usize> = ALL.iter().map(|n| delivered(&s, *n).len()).collect();
        if counts.contains(&1) && counts.contains(&0) {
            split_seen = true;
            break;
        }
    }
    assert!(
        split_seen,
        "a crashed sender should be able to produce partial delivery — if this never \
         happens the test is not exercising the case it claims to"
    );
}
