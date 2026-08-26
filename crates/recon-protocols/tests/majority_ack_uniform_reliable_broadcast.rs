//! Majority-ack uniform reliable broadcast against Module 3.3 — and, because the point of this
//! abstraction is what it stopped assuming, that the schedule which breaks the all-ack version leaves
//! this one intact.
//!
//! Five processes throughout, not four. With four, the schedule that breaks all-ack's uniform
//! agreement splits them two and two, and against a majority quorum *neither side is a majority*:
//! nothing is delivered anywhere and "no two processes disagree" passes vacuously. Five gives a
//! three-two split with a real majority side, so the contrast can be asserted alongside a positive
//! delivery count. Five also tolerates two crashes rather than one, since N > 2f.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::majority_ack_uniform_reliable_broadcast::{
    Cmd, Ind, MajorityAckUniformReliableBroadcast,
};
use recon_protocols::uniform_reliable_broadcast::{self as allack, UniformReliableBroadcast};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const E: NodeId = NodeId::new(5);
const ALL: [NodeId; 5] = [A, B, C, D, E];

/// The network's promise, where a run makes one.
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

type Urb = MajorityAckUniformReliableBroadcast<u32>;

fn urb(me: NodeId) -> Urb {
    MajorityAckUniformReliableBroadcast::new(me, ALL, retransmit())
}

/// A synchronous run. Nothing here needs it — that is rather the point — but it keeps the
/// schedules comparable with the all-ack suite's.
fn sim(seed: u64) -> Sim<Urb> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, urb)
}

/// An asynchronous run with every fault knob on: no timing assumption of any kind.
fn rough(seed: u64) -> Sim<Urb> {
    Sim::new(
        Config::default()
            .seed(seed)
            .loss(0.3)
            .duplication(0.2)
            .reorder(0.2)
            .latency(Duration::from_millis(1), Duration::from_millis(40)),
        &ALL,
        urb,
    )
}

fn delivered(s: &Sim<Urb>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace().indications_at(node).map(|Ind::Deliver { from, msg }| (*from, *msg)).collect()
}

fn settle(s: &mut Sim<Urb>) {
    s.run_for(Duration::from_millis(1500));
}

// -------------------------------------------------------------- the module

#[test]
fn a_five_process_run_with_no_faults_delivers_everywhere() {
    let mut s = sim(1);
    s.command(A, Cmd::Broadcast(7));
    settle(&mut s);
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 7)], "{n}");
    }
}

#[test]
fn no_failure_detection_traffic_is_sent() {
    // Asserted as an observable difference rather than by inspecting the struct. The all-ack
    // version chatters when nothing at all is broadcast — that is its detector keeping its
    // beliefs current. This one is silent, because it has no beliefs to keep.
    let mut quiet = sim(2);
    quiet.run_for(detect_after() * 4);
    assert_eq!(quiet.trace().send_count(), 0, "nothing broadcast, so nothing sent at all");

    let mut chatty: Sim<UniformReliableBroadcast<u32>> =
        Sim::new(Config::default().seed(2).synchronous(BOUND), &ALL, |me| {
            UniformReliableBroadcast::new(me, ALL, retransmit(), heartbeat(), detect_after())
        });
    chatty.run_for(detect_after() * 4);
    assert!(
        chatty.trace().send_count() > 0,
        "the all-ack version keeps talking with nothing to say — that traffic is the detector"
    );
}

#[test]
fn broadcasting_is_the_only_request_this_layer_needs() {
    // No Start command exists to forget: `Cmd` has one variant, and a fresh run delivers.
    let mut s = sim(3);
    s.command(A, Cmd::Broadcast(9));
    settle(&mut s);
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 9)], "{n}");
    }
}

#[test]
fn the_majority_boundary_is_exact() {
    // Five processes: a majority is three. Crash enough that only two can ever relay and nothing
    // is delivered; crash one fewer and it is. The originator's own relay counts like any other.
    assert_eq!(urb(A).majority(), 3);

    // Two relayers: A and B. C, D and E are gone before anything is sent.
    let mut short = sim(4);
    for n in [C, D, E] {
        short.crash(n);
    }
    short.command(A, Cmd::Broadcast(1));
    settle(&mut short);
    for n in [A, B] {
        assert!(
            delivered(&short, n).is_empty(),
            "{n}: two of five is not a majority, so nothing may be delivered"
        );
    }

    // Three relayers: A, B and C. One more, and the same broadcast goes through.
    let mut enough = sim(4);
    for n in [D, E] {
        enough.crash(n);
    }
    enough.command(A, Cmd::Broadcast(1));
    settle(&mut enough);
    for n in [A, B, C] {
        assert_eq!(delivered(&enough, n), vec![(A, 1)], "{n}: three of five is a majority");
    }
}

#[test]
fn the_majority_boundary_needs_an_even_membership_to_be_pinned() {
    // With five processes, `2k > 5` and `2k >= 5` are the same predicate: both need three. The
    // off-by-one that would let exactly half suffice is invisible at every odd membership, so the
    // boundary is asserted at an even one, where half is a real quantity.
    const FOUR: [NodeId; 4] = [A, B, C, D];
    type Four = MajorityAckUniformReliableBroadcast<u32>;
    let four = |seed: u64| -> Sim<Four> {
        Sim::new(Config::default().seed(seed).synchronous(BOUND), &FOUR, |me| {
            MajorityAckUniformReliableBroadcast::new(me, FOUR, retransmit())
        })
    };
    let got = |s: &Sim<Four>, n: NodeId| -> usize {
        s.trace().indications_at(n).filter(|i| matches!(i, Ind::Deliver { .. })).count()
    };

    assert_eq!(
        MajorityAckUniformReliableBroadcast::<u32>::new(A, FOUR, retransmit()).majority(),
        3,
        "four processes need three, not two"
    );

    // Exactly half can relay. Half is not a majority, so nothing may be delivered.
    let mut half = four(1);
    half.crash(C);
    half.crash(D);
    half.command(A, Cmd::Broadcast(1));
    half.run_for(Duration::from_millis(1500));
    for n in [A, B] {
        assert_eq!(got(&half, n), 0, "{n}: two of four is exactly half, and half is not more");
    }

    // One more, and it goes through.
    let mut more = four(1);
    more.crash(D);
    more.command(A, Cmd::Broadcast(1));
    more.run_for(Duration::from_millis(1500));
    for n in [A, B, C] {
        assert_eq!(got(&more, n), 1, "{n}: three of four is a majority");
    }
}

// ------------------------------------------- the four guarantees, without a detector

#[test]
fn a_correct_sender_delivers_its_own_broadcast() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.command(B, Cmd::Broadcast(4));
        settle(&mut s);
        assert_eq!(delivered(&s, B), vec![(B, 4)], "seed {seed}");
    }
}

#[test]
fn a_minority_crashing_does_not_prevent_delivery() {
    // Two of five crash, leaving three correct — a majority, since N > 2f with N = 5, f = 2.
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.command(A, Cmd::Broadcast(1));
        s.run_for(BOUND / 2);
        s.crash(D);
        s.crash(E);
        settle(&mut s);
        for n in [A, B, C] {
            assert_eq!(delivered(&s, n), vec![(A, 1)], "seed {seed}: {n} of three correct");
        }
    }
}

#[test]
fn uniform_agreement_holds_when_a_process_delivers_then_crashes() {
    let found = (0..40u64).find_map(|seed| {
        let mut s = sim(seed);
        s.command(A, Cmd::Broadcast(1));
        let mut steps = 0;
        while steps < 300 {
            s.run_for(Duration::from_millis(1));
            steps += 1;
            let who: Vec<NodeId> =
                ALL.iter().copied().filter(|n| !delivered(&s, *n).is_empty()).collect();
            if who.len() == 1 {
                s.crash(who[0]);
                settle(&mut s);
                return Some((seed, who[0], s));
            }
            if who.len() > 1 {
                return None;
            }
        }
        None
    });

    let (seed, first, s) = found.expect("no seed produced a lone first deliverer");
    for n in ALL.iter().filter(|n| **n != first) {
        assert_eq!(
            delivered(&s, *n),
            vec![(A, 1)],
            "seed {seed}: {n} must deliver what {first} delivered before crashing"
        );
    }
}

#[test]
fn nothing_is_delivered_that_was_not_broadcast_and_nothing_twice() {
    for seed in 0..10u64 {
        let mut s = sim(seed);
        s.command(A, Cmd::Broadcast(11));
        s.command(C, Cmd::Broadcast(22));
        s.command(E, Cmd::Broadcast(11)); // identical content, a different broadcast
        settle(&mut s);

        for n in ALL {
            let got = delivered(&s, n);
            assert_eq!(got.len(), 3, "seed {seed}: {n} delivered {got:?}");
            for (from, msg) in &got {
                let broadcast = matches!((from, msg), (&A, 11) | (&C, 22) | (&E, 11));
                assert!(broadcast, "seed {seed}: {n} delivered ({from}, {msg}), never broadcast");
            }
            let mut sorted = got.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), 3, "seed {seed}: {n} delivered a duplicate");
        }
    }
}

#[test]
fn uniform_agreement_holds_with_no_timing_assumption_at_all() {
    // Loss, duplication, reordering and unbounded jitter. The all-ack version's detector would
    // accuse the living here; this layer has nothing to be wrong about.
    for seed in 0..10u64 {
        let mut s = rough(seed);
        assert_eq!(s.delivery_bound(), None, "this run makes no timing promise");
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_secs(20));

        let sets: Vec<Vec<(NodeId, u32)>> = ALL.iter().map(|n| delivered(&s, *n)).collect();
        assert!(
            sets.iter().all(|d| *d == vec![(A, 1)]),
            "seed {seed}: uniform agreement must hold without any timing assumption: {sets:?}"
        );
    }
}

// ------------------------------------------------- the contrast with all-ack

/// The all-ack version, five processes, run through the partition schedule.
fn all_ack_split(seed: u64) -> Vec<usize> {
    type Aurb = UniformReliableBroadcast<u32>;
    let mut s: Sim<Aurb> = Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, |me| {
        UniformReliableBroadcast::new(me, ALL, retransmit(), heartbeat(), detect_after())
    });
    s.partition(&[&[A, B, C], &[D, E]]);
    s.command(A, allack::Cmd::Broadcast(1));
    s.run_for(detect_after() * 4);
    s.crash(A);
    s.crash(B);
    s.crash(C);
    s.run_for(detect_after() * 8);

    ALL.iter()
        .map(|n| {
            s.trace()
                .indications_at(*n)
                .filter(|i| matches!(i, allack::Ind::Deliver { .. }))
                .count()
        })
        .collect()
}

/// The same schedule against the majority version.
fn majority_split(seed: u64, heal: bool) -> Sim<Urb> {
    let mut s = sim(seed);
    s.partition(&[&[A, B, C], &[D, E]]);
    s.command(A, Cmd::Broadcast(1));
    s.run_for(detect_after() * 4);
    if heal {
        s.heal();
        settle(&mut s);
    }
    s
}

#[test]
fn the_schedule_that_splits_all_ack_does_not_split_this() {
    // All-ack: the majority side delivers and then crashes, and the minority side — having
    // accused it — delivers nothing, or delivers on its own. Whatever it does, this must not.
    let counts = all_ack_split(0);
    assert!(counts.iter().any(|c| *c > 0), "the all-ack run must deliver something to compare");

    let s = majority_split(0, false);
    let sets: Vec<Vec<(NodeId, u32)>> = ALL.iter().map(|n| delivered(&s, *n)).collect();
    let distinct: std::collections::BTreeSet<&Vec<(NodeId, u32)>> =
        sets.iter().filter(|d| !d.is_empty()).collect();
    assert!(distinct.len() <= 1, "no two processes may deliver different sets: {sets:?}");
}

#[test]
fn that_assertion_is_not_vacuous_the_majority_side_delivered() {
    // The trap this suite exists to avoid: with four processes split two and two, neither side is
    // a majority, nothing is delivered, and the assertion above passes for the wrong reason.
    let s = majority_split(0, false);
    for n in [A, B, C] {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} is in the majority and must deliver");
    }
    for n in [D, E] {
        assert!(delivered(&s, n).is_empty(), "{n} is in the minority and must not");
    }
}

#[test]
fn the_difference_is_attributable_no_process_is_ever_excluded() {
    // All-ack resolves the partition by accusing the far side, and the accusation is carried by
    // heartbeats it must keep sending. Under the same partition this layer sends nothing except
    // the broadcast it was given: there is no judgement to make and nothing to make it with,
    // which is exactly why the minority blocks instead of diverging.
    let s = majority_split(0, false);
    let payload_sends = s.trace().send_count();

    let mut idle = sim(0);
    idle.partition(&[&[A, B, C], &[D, E]]);
    idle.run_for(detect_after() * 4);
    assert_eq!(
        idle.trace().send_count(),
        0,
        "under the identical partition, with nothing broadcast, this layer sends nothing"
    );
    assert!(payload_sends > 0, "and what it does send when given a broadcast is that broadcast");
}

#[test]
fn the_minority_catches_up_once_the_partition_heals() {
    // Blocking was a pause, not a divergence.
    let s = majority_split(0, true);
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} delivered after the heal");
    }
}

// ------------------------------------------------- where the assumption fails

#[test]
fn without_a_majority_nothing_is_delivered_and_nothing_diverges() {
    // Three of five crash, leaving two: N ≤ 2f, the assumption gone. The layer must block, not
    // deliver something the survivors will never agree on.
    for seed in 0..6u64 {
        let mut s = sim(seed);
        for n in [C, D, E] {
            s.crash(n);
        }
        s.command(A, Cmd::Broadcast(1));
        settle(&mut s);

        for n in [A, B] {
            assert!(delivered(&s, n).is_empty(), "seed {seed}: {n} blocked rather than delivered");
        }
        // The distinction that matters: blocked, not inconsistent.
        assert_eq!(delivered(&s, A), delivered(&s, B), "seed {seed}: and the survivors agree");
    }
}

#[test]
fn progress_resumes_when_a_majority_is_available_again() {
    let mut s = sim(5);
    s.partition(&[&[A, B], &[C, D, E]]);
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(400));
    assert!(delivered(&s, A).is_empty(), "A's side is a minority and waits");

    s.heal();
    settle(&mut s);
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} delivered once a majority was reachable");
    }
}

#[test]
fn the_agreement_assertions_are_not_vacuous() {
    let mut s = sim(6);
    for (n, v) in ALL.iter().zip([1u32, 2, 3, 4, 5]) {
        s.command(*n, Cmd::Broadcast(v));
    }
    settle(&mut s);
    for n in ALL {
        assert_eq!(delivered(&s, n).len(), ALL.len(), "{n} delivered every broadcast");
    }
    assert!(s.trace().indication_count() >= ALL.len() * ALL.len());
}

#[test]
fn the_wire_survives_encoding() {
    let mut s = sim(7);
    s.enable_codec_check();
    s.command(A, Cmd::Broadcast(3));
    settle(&mut s);
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 3)], "{n}");
    }
}
