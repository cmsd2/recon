//! Uniform reliable broadcast against Module 3.3 — and, because the guarantee is only meaningful
//! by contrast, that reliable broadcast genuinely fails the case this protocol is for.

use core::time::Duration;
use recon_core::{Effect, Event, MemStore, NodeId, Time, step_with};
use recon_protocols::perfect_failure_detector::Heartbeat;
use recon_protocols::reliable_broadcast::{self as rb, ReliableBroadcast};
use recon_protocols::uniform_reliable_broadcast::{
    BroadcastId, Cmd, Data, Ind, UniformReliableBroadcast, Wire,
};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

/// The network's promise; everything else is derived from it.
const BOUND: Duration = Duration::from_millis(20);

fn retransmit() -> Duration {
    Duration::from_millis(10)
}
fn heartbeat() -> Duration {
    BOUND * 2
}
fn detect_after() -> Duration {
    heartbeat() * 3
}

fn urb(me: NodeId) -> UniformReliableBroadcast<u32> {
    UniformReliableBroadcast::new(me, ALL, retransmit(), heartbeat(), detect_after())
}

/// A synchronous run — the assumption the detector, and therefore this layer, depends on.
fn sim(seed: u64) -> Sim<UniformReliableBroadcast<u32>> {
    let s: Sim<UniformReliableBroadcast<u32>> =
        Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, urb);
    assert_eq!(s.delivery_bound(), Some(BOUND));
    s
}

fn delivered(s: &Sim<UniformReliableBroadcast<u32>>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace()
        .indications_at(node)
        .filter_map(|ind| match ind {
            Ind::Deliver { from, msg } => Some((*from, *msg)),
            // Over a perfect link there are none. This helper is about deliveries.
            _ => None,
        })
        .collect()
}

fn rng() -> rand_chacha::ChaCha8Rng {
    use rand::SeedableRng;
    rand_chacha::ChaCha8Rng::seed_from_u64(0)
}

// -------------------------------------------------- the wire: task 1.1

#[test]
fn the_wire_multiplexes_broadcasts_and_heartbeats() {
    let mut ids = 0;
    let mut p = urb(A);
    let mut r = rng();

    let started = step_with(&mut p, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    assert!(
        started.iter().any(|e| matches!(e, Effect::Send { msg: Wire::Detector(_), .. })),
        "starting must put heartbeats on the wire"
    );

    let sent = step_with(
        &mut p,
        Event::Cmd(Cmd::Broadcast(9u32)),
        Time::from_millis(1),
        &mut r,
        &mut store(),
        &mut ids,
    );
    assert!(
        sent.iter().any(|e| matches!(e, Effect::Send { msg: Wire::Broadcast(_), .. })),
        "broadcasting must put payloads on the wire"
    );
    assert!(
        !sent.iter().any(|e| matches!(e, Effect::Send { msg: Wire::Detector(_), .. })),
        "and the two are distinguishable"
    );
}

#[test]
fn both_wire_variants_survive_encoding() {
    let d = Wire::Detector::<u32>(Heartbeat);
    assert_eq!(recon_sim::codec::round_trip(&d).expect("round trip"), d);

    let payload = Data { id: BroadcastId { origin: A, seq: 1 }, payload: 5u32 };
    assert_eq!(recon_sim::codec::round_trip(&payload).expect("round trip"), payload);
}

// ------------------------------------ relay and acknowledgement: tasks 1.3, 1.4

#[test]
fn a_repeat_receipt_records_the_acknowledgement_but_does_not_relay() {
    let mut ids = 0;
    let mut p = urb(B);
    let mut r = rng();
    step_with(&mut p, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);

    // Build a message from A as it would arrive.
    let mut a = urb(A);
    let from_a = step_with(
        &mut a,
        Event::Cmd(Cmd::Broadcast(5u32)),
        Time::ZERO,
        &mut r,
        &mut store(),
        &mut ids,
    );
    let to_b = from_a
        .iter()
        .find_map(|e| match e {
            Effect::Send { to, msg } if *to == B => Some(msg.clone()),
            _ => None,
        })
        .expect("a message for B");

    let first = step_with(
        &mut p,
        Event::Msg { from: A, msg: to_b.clone() },
        Time::from_millis(1),
        &mut r,
        &mut store(),
        &mut ids,
    );
    let relays = first.iter().filter(|e| matches!(e, Effect::Send { .. })).count();
    assert!(relays >= 4, "a first receipt relays to everyone, saw {relays}");
    assert_eq!(p.pending_count(), 1);

    // C's relay of the same message is a *different* wire message — it carries C's own perfect
    // link identifier — so it is not suppressed as a duplicate below.
    let mut c = urb(C);
    step_with(&mut c, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let c_relayed = step_with(
        &mut c,
        Event::Msg { from: A, msg: to_b },
        Time::from_millis(1),
        &mut r,
        &mut store(),
        &mut ids,
    );
    let c_to_b = c_relayed
        .iter()
        .find_map(|e| match e {
            Effect::Send { to, msg: m @ Wire::Broadcast(_) } if *to == B => Some(m.clone()),
            _ => None,
        })
        .expect("C relays to B");

    let second = step_with(
        &mut p,
        Event::Msg { from: C, msg: c_to_b },
        Time::from_millis(2),
        &mut r,
        &mut store(),
        &mut ids,
    );
    assert!(
        !second.iter().any(|e| matches!(e, Effect::Send { msg: Wire::Broadcast(_), .. })),
        "a repeat receipt must not relay again"
    );
    let id = BroadcastId { origin: A, seq: 1 };
    let acked: Vec<NodeId> = p.acknowledged_by(id).collect();
    assert!(
        acked.contains(&A) && acked.contains(&C),
        "but it does record the acknowledgement, saw {acked:?}"
    );
}

#[test]
fn delivery_waits_for_every_correct_process() {
    // Nothing is delivered until all four have been seen to acknowledge.
    let mut s = sim(1);
    s.command(A, Cmd::Broadcast(7));
    s.run_for(Duration::from_millis(2));
    for n in ALL {
        assert!(delivered(&s, n).is_empty(), "{n} delivered before anyone could acknowledge");
    }
    s.run_for(Duration::from_millis(600));
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 7)], "{n} should have delivered by now");
    }
}

#[test]
fn a_crash_unblocks_delivery() {
    // D never acknowledges. Delivery is impossible until the detector reports it crashed.
    let mut s = sim(2);
    s.crash(D);
    s.command(A, Cmd::Broadcast(7));
    s.run_for(Duration::from_millis(100));
    for n in [A, B, C] {
        assert!(delivered(&s, n).is_empty(), "{n} cannot deliver while waiting on D");
    }

    s.run_for(detect_after() * 4);
    for n in [A, B, C] {
        assert_eq!(delivered(&s, n), vec![(A, 7)], "{n} should deliver once D is detected");
        assert!(!s.protocol(n).unwrap().correct().any(|p| p == D));
    }
}

// ----------------------------- validity, no duplication, no creation: tasks 2.1, 2.2

#[test]
fn a_correct_sender_delivers_its_own_broadcast() {
    let mut s = sim(3);
    s.command(A, Cmd::Broadcast(3));
    s.run_for(Duration::from_millis(600));
    assert_eq!(delivered(&s, A), vec![(A, 3)]);
}

#[test]
fn every_process_delivers_each_broadcast_exactly_once() {
    let mut s = sim(4);
    for i in 0..4u32 {
        s.command(A, Cmd::Broadcast(i));
    }
    s.command(B, Cmd::Broadcast(100));
    s.run_for(Duration::from_millis(2000));

    for n in ALL {
        let mut got: Vec<u32> = delivered(&s, n).into_iter().map(|(_, m)| m).collect();
        got.sort();
        assert_eq!(got, vec![0, 1, 2, 3, 100], "{n} saw the wrong multiset");
    }
}

#[test]
fn identical_content_broadcast_twice_is_delivered_twice() {
    let mut s = sim(5);
    s.command(A, Cmd::Broadcast(42));
    s.command(A, Cmd::Broadcast(42));
    s.run_for(Duration::from_millis(1000));
    for n in ALL {
        assert_eq!(delivered(&s, n).len(), 2, "{n} must deliver both");
    }
}

#[test]
fn a_message_reaching_a_process_by_relay_is_attributed_to_its_originator() {
    let mut s = sim(6);
    s.partition(&[&[A, B], &[C, D]]);
    s.command(A, Cmd::Broadcast(77));
    s.run_for(Duration::from_millis(200));
    s.heal();
    s.run_for(Duration::from_millis(2000));

    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 77)], "{n} must name A, not a relayer");
    }
}

#[test]
fn nothing_is_delivered_that_was_not_broadcast() {
    let mut s = sim(7);
    s.run_for(Duration::from_millis(1000));
    assert_eq!(s.trace().indication_count(), 0);
}

// ------------------------------------------- uniform agreement: tasks 2.4, 2.5

/// Broadcast, let it settle just enough that someone may deliver, then kill the sender.
fn deliver_then_crash(seed: u64, settle: Duration) -> Vec<usize> {
    let mut s = sim(seed);
    s.command(A, Cmd::Broadcast(1));
    s.run_for(settle);
    s.crash(A);
    s.run_for(detect_after() * 6);
    [B, C, D].iter().map(|n| delivered(&s, *n).len()).collect()
}

#[test]
fn uniform_agreement_holds_when_a_process_delivers_then_crashes() {
    for seed in 0..30u64 {
        for settle in [40u64, 80, 120, 200] {
            let counts = deliver_then_crash(seed, Duration::from_millis(settle));
            let any = counts.iter().any(|c| *c > 0);
            let all = counts.iter().all(|c| *c == 1);
            assert!(
                !any || all,
                "seed {seed} settle {settle}ms: uniform agreement violated: {counts:?}"
            );
        }
    }
}

#[test]
fn uniform_agreement_holds_through_partition_and_healing() {
    for seed in 0..10u64 {
        let mut s = sim(seed);
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(40));
        s.partition(&[&[A, B], &[C, D]]);
        s.run_for(Duration::from_millis(300));
        s.heal();
        s.run_for(detect_after() * 8);

        let counts: Vec<usize> = ALL.iter().map(|n| delivered(&s, *n).len()).collect();
        let any = counts.iter().any(|c| *c > 0);
        let all = counts.iter().all(|c| *c == 1);
        assert!(!any || all, "seed {seed}: {counts:?}");
    }
}

// ------------------------- that the tests distinguish this protocol: tasks 3.1 to 3.3

/// The book's Figure 3.3, which is exactly what separates these two abstractions: the processes that
/// deliver crash before their relays can escape, leaving the survivors never delivering.
///
/// Engineered with a partition so relays cannot cross, then crashing both deliverers before it
/// heals. In synchronous mode with no loss this is the only way to split reliable broadcast —
/// best-effort broadcast sends to everyone in one step, so a crash cannot catch it partway.
fn figure_3_3<Pr, F>(seed: u64, make: F, broadcast: impl FnOnce(&mut Sim<Pr>)) -> Vec<usize>
where
    Pr: recon_core::Protocol,
    Pr::Cmd: Clone,
    Pr::Msg: Clone + PartialEq,
    Pr::Ind: Clone,
    Pr::Meta: Clone,
    Pr::Entry: Clone,
    F: FnMut(NodeId) -> Pr + 'static,
{
    let mut s: Sim<Pr> = Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, make);
    s.partition(&[&[A, B], &[C, D]]);
    broadcast(&mut s);
    s.run_for(Duration::from_millis(150));
    s.crash(A);
    s.crash(B);
    s.heal();
    s.run_for(detect_after() * 8);
    ALL.iter().map(|n| s.trace().indications_at(*n).count()).collect()
}

#[test]
fn reliable_broadcast_does_violate_uniform_agreement_under_the_same_test() {
    // Without this the agreement test could be passing for reasons unrelated to the algorithm.
    let split = (0..20u64).find(|seed| {
        let c = figure_3_3(
            *seed,
            |me| ReliableBroadcast::new(me, ALL, retransmit()),
            |s| {
                s.command(A, rb::Cmd::Broadcast(1));
            },
        );
        c.iter().any(|x| *x > 0) && c.contains(&0)
    });
    assert!(
        split.is_some(),
        "the test does not distinguish the abstractions: reliable broadcast never split"
    );
}

#[test]
fn uniform_reliable_broadcast_does_not_split_under_the_same_test() {
    // The same scenario. Uniform reliable broadcast refuses to deliver at all rather than let
    // some processes deliver what the others will never see.
    for seed in 0..20u64 {
        let c = figure_3_3(seed, urb, |s| {
            s.command(A, Cmd::Broadcast(1));
        });
        let any = c.iter().any(|x| *x > 0);
        let all = c.iter().all(|x| *x == 1);
        assert!(!any || all, "seed {seed}: uniform agreement violated: {c:?}");
    }
}

#[test]
fn the_agreement_tests_are_not_vacuous() {
    // Every absence-of-violation property here is satisfied by delivering nothing.
    let delivering = (0..30u64)
        .filter(|s| deliver_then_crash(*s, Duration::from_millis(200)).iter().any(|c| *c > 0))
        .count();
    assert!(delivering > 0, "no seed ever delivered — the agreement tests pass vacuously");
}

#[test]
fn uniform_agreement_breaks_when_the_timing_assumption_is_withdrawn() {
    // A partition violates the assumption directly: delivery is no longer bounded, so the
    // detector accuses processes that are alive and merely unreachable. `correct` shrinks
    // wrongly, `candeliver` is satisfied too early, and a process delivers something the far
    // side will never see.
    //
    // This is the assumption failing, not the implementation — and it is what makes running the
    // guarantee suites in synchronous mode load-bearing rather than incidental.
    let broke = (0..30u64).any(|seed| {
        let mut s: Sim<UniformReliableBroadcast<u32>> =
            Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, urb);
        s.partition(&[&[A, B], &[C, D]]);
        s.command(A, Cmd::Broadcast(1));
        // Long enough for A and B to accuse C and D of crashing, which they have not.
        s.run_for(detect_after() * 4);
        s.crash(A);
        s.crash(B);
        s.heal();
        s.run_for(detect_after() * 8);

        let counts: Vec<usize> = ALL.iter().map(|n| delivered(&s, *n).len()).collect();
        counts.iter().any(|c| *c > 0) && counts.contains(&0)
    });
    assert!(
        broke,
        "if the guarantee never breaks without synchrony, the synchronous mode is not what \
         makes the agreement tests pass"
    );
}

#[test]
fn the_detector_does_accuse_the_unreachable_side_of_a_partition() {
    // The mechanism behind the test above, isolated: a partition is indistinguishable from a
    // crash to a timeout-based detector, which is precisely why perfect detection needs bounded
    // delivery.
    let mut s: Sim<UniformReliableBroadcast<u32>> =
        Sim::new(Config::default().seed(1).synchronous(BOUND), &ALL, urb);
    s.partition(&[&[A, B], &[C, D]]);
    s.run_for(detect_after() * 4);

    let a_correct: Vec<NodeId> = s.protocol(A).unwrap().correct().collect();
    assert!(
        !a_correct.contains(&C) && !a_correct.contains(&D),
        "A should have accused the far side, saw correct = {a_correct:?}"
    );
    assert!(a_correct.contains(&B), "but not its own side");
}

/// A fresh store per call: these protocols write nothing durably.
fn store() -> MemStore<core::convert::Infallible, core::convert::Infallible> {
    MemStore::default()
}
