//! Majority-ack uniform reliable broadcast over session links.
//!
//! The interesting content is what dropping the detector removed: not just a dependency but a
//! whole liveness path. A peer that never returns was never waited for, so nothing has to judge
//! it crashed — and a peer absent for far longer than any timeout the all-ack version would have
//! used is not a stranger when it comes back.
//!
//! Five processes, for the reason set out in the sibling suite: a three-two partition has a real
//! majority side, so "no two processes disagree" can be asserted alongside a positive delivery
//! count instead of passing vacuously.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::majority_ack_uniform_reliable_broadcast::{
    Cmd, Ind, MajorityAckUniformReliableBroadcast,
};
use recon_protocols::session_link::SessionLink;
use recon_protocols::stacks::MajorityAckUniformReliableBroadcastOverSessions as MaurbOverSessions;
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const E: NodeId = NodeId::new(5);
const ALL: [NodeId; 5] = [A, B, C, D, E];

const BOUND: Duration = Duration::from_millis(20);

// The base module over a session link. The fork it replaces held the same algorithm with the
// link swapped underneath, which is what the link port made unnecessary.
type Urb = MaurbOverSessions<u32>;

fn sim(seed: u64) -> Sim<Urb> {
    let mut s: Sim<Urb> =
        Sim::new(Config::default().seed(seed).sessions().synchronous(BOUND), &ALL, |me| {
            MajorityAckUniformReliableBroadcast::with_link(me, ALL, SessionLink::new())
        });
    s.deliver_session_events();
    s
}

fn delivered(s: &Sim<Urb>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            Ind::Deliver { from, msg } => Some((*from, *msg)),
            _ => None,
        })
        .collect()
}

fn session_reports(s: &Sim<Urb>, node: NodeId) -> (usize, usize) {
    let mut ended = 0;
    let mut established = 0;
    for i in s.trace().indications_at(node) {
        match i {
            Ind::SessionEnded { .. } => ended += 1,
            Ind::SessionEstablished { .. } => established += 1,
            Ind::Deliver { .. } => {}
        }
    }
    (ended, established)
}

fn settle(s: &mut Sim<Urb>) {
    s.run_for(Duration::from_millis(2000));
}

// ---------------------------------------------------------------- the module

#[test]
fn a_run_with_sessions_holding_delivers_everywhere() {
    let mut s = sim(1);
    s.run_for(Duration::from_millis(50));
    s.command(A, Cmd::Broadcast(7));
    settle(&mut s);
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 7)], "{n}");
    }
}

#[test]
fn no_failure_detection_traffic_is_sent() {
    // One child, so no wire discriminant and no second sender. With nothing broadcast, an idle
    // run over established sessions sends nothing at all — the all-ack version's heartbeats would
    // have kept flowing.
    let mut s = sim(2);
    s.run_for(Duration::from_millis(600));
    assert_eq!(s.trace().send_count(), 0, "nothing broadcast, so nothing sent");

    s.command(A, Cmd::Broadcast(1));
    settle(&mut s);
    assert!(s.trace().send_count() > 0, "and what it sends when given a broadcast is that");
    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n}");
    }
}

#[test]
fn the_majority_boundary_is_pinned_at_an_even_membership() {
    // With an odd N, `2k > N` and `2k >= N` are the same predicate. Four processes make half a
    // real quantity, so the off-by-one that would let exactly half suffice is visible.
    const FOUR: [NodeId; 4] = [A, B, C, D];
    type Four = MaurbOverSessions<u32>;
    let four = |seed: u64| -> Sim<Four> {
        let mut s: Sim<Four> =
            Sim::new(Config::default().seed(seed).sessions().synchronous(BOUND), &FOUR, |me| {
                MajorityAckUniformReliableBroadcast::with_link(me, FOUR, SessionLink::new())
            });
        s.deliver_session_events();
        s
    };
    let got = |s: &Sim<Four>, n: NodeId| -> usize {
        s.trace().indications_at(n).filter(|i| matches!(i, Ind::Deliver { .. })).count()
    };

    assert_eq!(Four::with_link(A, FOUR, SessionLink::new()).majority(), 3);

    let mut half = four(1);
    half.run_for(Duration::from_millis(50));
    half.crash(C);
    half.crash(D);
    half.command(A, Cmd::Broadcast(1));
    half.run_for(Duration::from_millis(2000));
    for n in [A, B] {
        assert_eq!(got(&half, n), 0, "{n}: two of four is exactly half, and half is not more");
    }

    let mut more = four(1);
    more.run_for(Duration::from_millis(50));
    more.crash(D);
    more.command(A, Cmd::Broadcast(1));
    more.run_for(Duration::from_millis(2000));
    for n in [A, B, C] {
        assert_eq!(got(&more, n), 1, "{n}: three of four is a majority");
    }
}

// ------------------------------------------------------- across session endings

#[test]
fn validity_and_uniform_agreement_hold_across_repeated_endings() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.run_for(Duration::from_millis(50));
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(3));
        s.break_session(A, D);
        s.run_for(Duration::from_millis(7));
        s.break_session(B, C);
        s.run_for(Duration::from_millis(9));
        s.break_session(C, E);
        settle(&mut s);

        let sets: Vec<Vec<(NodeId, u32)>> = ALL.iter().map(|n| delivered(&s, *n)).collect();
        assert!(sets.iter().all(|d| *d == vec![(A, 1)]), "seed {seed}: {sets:?}");
    }
}

#[test]
fn uniform_agreement_holds_when_a_process_delivers_then_crashes() {
    let found = (0..40u64).find_map(|seed| {
        let mut s = sim(seed);
        s.run_for(Duration::from_millis(50));
        s.command(A, Cmd::Broadcast(1));
        // One event at a time: a delivery is one event, so the state "exactly one has delivered"
        // cannot be stepped over, where a millisecond could hold two.
        let mut steps = 0;
        while steps < 20_000 && s.step() {
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
        assert_eq!(delivered(&s, *n), vec![(A, 1)], "seed {seed}: {n} follows {first}");
    }
}

#[test]
fn a_lost_suffix_is_repaired_by_the_resend_and_nothing_is_attempted_on_the_ending() {
    let mut s = sim(3);
    s.run_for(Duration::from_millis(50));
    s.command(A, Cmd::Broadcast(1));
    s.step_now(); // the command runs; its sends are in flight
    for peer in [C, D, E] {
        s.break_session(A, peer);
        s.break_session(B, peer);
    }
    settle(&mut s);

    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} delivered after re-establishment");
    }
    assert!(s.trace().session_ends() > 0, "the endings really happened");
}

#[test]
fn a_resend_goes_only_to_the_peer_whose_session_returned() {
    let mut s = sim(4);
    s.run_for(Duration::from_millis(50));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(900)); // delivered everywhere
    assert_eq!(s.protocol(A).unwrap().pending_count(), 1);

    let broke_at = s.now();
    s.break_session(A, E);
    s.run_for(Duration::from_millis(400));

    let from_a_to = |to: NodeId| {
        s.trace()
            .events()
            .iter()
            .filter(|e| {
                matches!(e, recon_sim::TraceEvent::Sent { at, from, to: t, .. }
                    if *at > broke_at && *from == A && *t == to)
            })
            .count()
    };
    assert_eq!(from_a_to(E), 1, "one resend per pending message, to E alone");
    for other in [B, C, D] {
        assert_eq!(from_a_to(other), 0, "{other}'s session never ended, so it hears nothing");
    }
}

#[test]
fn both_session_reports_reach_the_layer_above() {
    let mut s = sim(5);
    s.run_for(Duration::from_millis(50));
    s.break_session(A, C);
    s.run_for(Duration::from_millis(400));

    for n in [A, C] {
        let (ended, established) = session_reports(&s, n);
        assert!(ended >= 1, "{n} was told the session ended");
        assert!(established >= 1, "{n} was told a later one was established");
    }
}

// ------------------------- what dropping the detector removed

#[test]
fn a_stalled_process_misses_nothing_because_its_session_never_ended() {
    // The fault the layer's guarantee actually rests on: a process stalls across a broadcast and
    // its sessions stay up throughout. Uniform agreement says every correct process delivers, and
    // a process that was merely descheduled is correct — so a message lost to the stall with no
    // `SessionEnded` to account for it would break the guarantee permanently, and silently.
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.run_for(Duration::from_millis(50)); // sessions up
        assert!(s.has_session(A, B), "seed {seed}");

        s.suspend(B);
        s.command(A, Cmd::Broadcast(9));
        s.run_for(Duration::from_millis(400));
        assert!(delivered(&s, B).is_empty(), "seed {seed}: B is stalled and delivers nothing");
        assert!(s.has_session(A, B), "seed {seed}: and its session never ended");

        s.resume(B);
        settle(&mut s);
        for n in ALL {
            assert_eq!(delivered(&s, n), vec![(A, 9)], "seed {seed}: {n}");
        }
        assert_eq!(session_reports(&s, B).0, 0, "seed {seed}: B was never told a session ended");
    }
}

#[test]
fn a_peer_that_never_returns_needs_no_accusation() {
    // E is cut off for good. The all-ack version cannot deliver until its detector accuses E.
    // Here nobody judges E at all: four of five is a majority, and that is the whole condition.
    let mut s = sim(6);
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C, D], &[E]]);
    s.command(A, Cmd::Broadcast(1));
    settle(&mut s);

    for n in [A, B, C, D] {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} delivered without waiting on E");
    }
    assert!(delivered(&s, E).is_empty(), "E, cut off and a minority of one, delivered nothing");
    // No message of any kind was needed to reach that conclusion about E.
    assert!(s.trace().send_count() > 0);
}

#[test]
fn a_long_absent_peer_is_not_a_stranger_when_it_returns() {
    // Absent for many times any timeout the all-ack version would have used. Nothing excluded it,
    // so nothing has to readmit it: it receives what it missed and delivers it.
    let mut s = sim(7);
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C, D], &[E]]);
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(3000)); // an age, by any detector's standards
    assert!(delivered(&s, E).is_empty(), "E missed it entirely");

    s.heal();
    settle(&mut s);
    assert_eq!(delivered(&s, E), vec![(A, 1)], "E caught up, no readmission required");
    for n in ALL {
        assert_eq!(delivered(&s, n).len(), 1, "{n} delivered exactly once");
    }
}

#[test]
fn resending_does_not_cause_double_delivery() {
    let mut s = sim(8);
    s.run_for(Duration::from_millis(50));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(2));
    for _ in 0..3 {
        for peer in [B, C, D, E] {
            s.break_session(A, peer);
        }
        s.run_for(Duration::from_millis(200));
    }
    settle(&mut s);

    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} delivered once despite repeated resends");
    }
}

// ------------------------------------------------- where the assumption fails

#[test]
fn a_minority_partition_delivers_nothing_and_the_majority_continues() {
    let mut s = sim(9);
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C], &[D, E]]);
    s.command(A, Cmd::Broadcast(1));
    settle(&mut s);

    for n in [A, B, C] {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} is in the majority and continues");
    }
    for n in [D, E] {
        assert!(delivered(&s, n).is_empty(), "{n} is in the minority and delivers nothing new");
    }
    assert_eq!(delivered(&s, D), delivered(&s, E), "and the minority agrees with itself");
}

#[test]
fn the_minority_catches_up_once_the_partition_heals() {
    let mut s = sim(9);
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C], &[D, E]]);
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(600));
    s.heal();
    settle(&mut s);

    for n in ALL {
        assert_eq!(delivered(&s, n), vec![(A, 1)], "{n} delivered after the heal");
    }
}

#[test]
fn without_a_majority_the_layer_blocks_rather_than_diverges() {
    // Three of five crash: N ≤ 2f, the assumption gone. Blocked, not inconsistent.
    for seed in 0..6u64 {
        let mut s = sim(seed);
        s.run_for(Duration::from_millis(50));
        for n in [C, D, E] {
            s.crash(n);
        }
        s.command(A, Cmd::Broadcast(1));
        settle(&mut s);
        for n in [A, B] {
            assert!(delivered(&s, n).is_empty(), "seed {seed}: {n} blocked");
        }
        assert_eq!(delivered(&s, A), delivered(&s, B), "seed {seed}: and the survivors agree");
    }
}

#[test]
fn the_agreement_assertions_are_not_vacuous() {
    let mut s = sim(10);
    s.run_for(Duration::from_millis(50));
    for (n, v) in ALL.iter().zip([1u32, 2, 3, 4, 5]) {
        s.command(*n, Cmd::Broadcast(v));
    }
    settle(&mut s);
    for n in ALL {
        assert_eq!(delivered(&s, n).len(), ALL.len(), "{n} delivered every broadcast");
    }
}
