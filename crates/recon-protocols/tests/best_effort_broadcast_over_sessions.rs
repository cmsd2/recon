//! `BestEffortBroadcast` over a session link: validity while sessions hold, and honesty when they
//! do not.
//!
//! The protocol under test is `BestEffortBroadcast` itself, with a session link as its type
//! argument. There is no separate session implementation — that is what the link port removed — so
//! what these tests now pin is that the one implementation, given a link that reports scope
//! boundaries, behaves as the fork it replaced did.

use core::time::Duration;
use recon_core::{Event, MemStore, NodeId, SessionEvent, Time, step_with};
use recon_protocols::best_effort_broadcast::{BestEffortBroadcast, Cmd, Ind};
use recon_protocols::session_link::SessionLink;
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

type Beb = BestEffortBroadcast<u32, SessionLink<u32>>;

fn beb(me: NodeId) -> Beb {
    BestEffortBroadcast::with_link(me, ALL, SessionLink::new())
}

fn sim(seed: u64) -> Sim<Beb> {
    let mut s: Sim<Beb> = Sim::new(
        Config::default()
            .seed(seed)
            .sessions()
            .latency(Duration::from_millis(1), Duration::from_millis(20)),
        &ALL,
        beb,
    );
    s.deliver_session_events();
    s
}

fn delivered(s: &Sim<Beb>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            Ind::Deliver { from, msg } => Some((*from, *msg)),
            _ => None,
        })
        .collect()
}

fn session_reports(s: &Sim<Beb>, node: NodeId) -> (usize, usize) {
    let inds: Vec<_> = s.trace().indications_at(node).collect();
    (
        inds.iter().filter(|i| matches!(i, Ind::SessionEnded { .. })).count(),
        inds.iter().filter(|i| matches!(i, Ind::SessionEstablished { .. })).count(),
    )
}

// ------------------------------------------------------------ task 2.1

#[test]
fn state_holds_nothing_but_the_process_set() {
    let mut ids = 0;
    let mut p: Beb = beb(A);
    let mut r = rand_chacha::ChaCha8Rng::from_seed([0; 32]);
    for i in 0..500u32 {
        step_with(
            &mut p,
            Event::Cmd(Cmd::Broadcast(i)),
            Time::ZERO,
            &mut r,
            &mut store(),
            &mut ids,
        );
        step_with(
            &mut p,
            Event::Msg { from: B, msg: i },
            Time::ZERO,
            &mut r,
            &mut store(),
            &mut ids,
        );
    }
    assert_eq!(p.peers().count(), ALL.len(), "five hundred messages, no per-message state");
}

#[test]
fn both_session_reports_reach_the_layer_above() {
    let mut ids = 0;
    let mut p: Beb = beb(A);
    let mut r = rand_chacha::ChaCha8Rng::from_seed([0; 32]);

    let ended = step_with(
        &mut p,
        Event::ScopeEvent(SessionEvent::Ended { peer: B, epoch: 1 }),
        Time::ZERO,
        &mut r,
        &mut store(),
        &mut ids,
    );
    let established = step_with(
        &mut p,
        Event::ScopeEvent(SessionEvent::Established { peer: B, epoch: 2 }),
        Time::ZERO,
        &mut r,
        &mut store(),
        &mut ids,
    );

    assert_eq!(ended.len(), 1);
    assert_eq!(established.len(), 1);
    assert!(format!("{ended:?}").contains("SessionEnded"), "and the two are distinguishable");
    assert!(format!("{established:?}").contains("SessionEstablished"));
}

// ------------------------------------------------------------ task 2.2

#[test]
fn validity_holds_while_sessions_hold() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.run_for(Duration::from_millis(50)); // let sessions come up
        s.command(A, Cmd::Broadcast(7));
        s.run_for(Duration::from_millis(500));
        for n in ALL {
            assert_eq!(delivered(&s, n), vec![(A, 7)], "seed {seed}: {n}");
        }
    }
}

#[test]
fn no_duplication_and_no_creation() {
    let mut s = sim(9);
    s.run_for(Duration::from_millis(50));
    for i in 0..5u32 {
        s.command(A, Cmd::Broadcast(i));
    }
    s.command(B, Cmd::Broadcast(100));
    s.run_for(Duration::from_millis(1000));

    for n in ALL {
        let mut got: Vec<u32> = delivered(&s, n).into_iter().map(|(_, m)| m).collect();
        got.sort();
        assert_eq!(got, vec![0, 1, 2, 3, 4, 100], "{n} saw the wrong multiset");
    }
}

// ------------------------------------------------------------ task 2.3

#[test]
fn a_message_lost_to_a_session_ending_is_not_retried() {
    // Find a seed where a broadcast is caught by a break, and confirm the loss is permanent and
    // visible rather than silently repaired.
    let outcome = |seed: u64| {
        let mut s = sim(seed);
        s.run_for(Duration::from_millis(50));
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(1)); // in flight
        s.break_session(A, D);
        s.run_for(Duration::from_millis(1500)); // ample time for any retry to happen
        (delivered(&s, D).len(), session_reports(&s, D))
    };
    let seed = (0..40u64)
        .find(|s| outcome(*s).0 == 0)
        .expect("some seed must catch the broadcast with the break");

    let (got, (ended, established)) = outcome(seed);
    assert_eq!(got, 0, "seed {seed}: the message never arrives — this layer does not retry");
    assert!(ended >= 1, "and the loss is not silent: the ending was reported");
    assert!(established >= 1, "as was the reconnection that followed");
}

// ------------------------------------- the directed send

#[test]
fn a_directed_send_reaches_only_the_addressed_member() {
    // Not part of Module 3.1, which has only a broadcast. It exists so a layer above can answer a
    // session that has just come back without paying for a fan-out to everyone else.
    let mut s = sim(3);
    s.run_for(Duration::from_millis(50));
    s.command(A, Cmd::SendTo { to: C, msg: 7 });
    s.run_for(Duration::from_millis(200));

    for n in ALL {
        let want = if n == C { vec![(A, 7)] } else { Vec::new() };
        assert_eq!(delivered(&s, n), want, "{n}");
    }
}

use rand::SeedableRng;

/// A fresh store per call: these protocols write nothing durably.
fn store() -> MemStore<core::convert::Infallible, core::convert::Infallible> {
    MemStore::default()
}
