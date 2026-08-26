//! The session link: reliable ordered delivery within a session, honesty across one, and state
//! that does not grow with messages.

use core::time::Duration;
use recon_core::{Effect, Event, NodeId, SessionEvent, Time, step};
use recon_protocols::session_link::{Cmd, Ind, SessionLink};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const ALL: [NodeId; 3] = [A, B, C];

fn rng() -> rand_chacha::ChaCha8Rng {
    use rand::SeedableRng;
    rand_chacha::ChaCha8Rng::seed_from_u64(0)
}

fn sim(seed: u64) -> Sim<SessionLink<u32>> {
    let mut s: Sim<SessionLink<u32>> = Sim::new(
        Config::default()
            .seed(seed)
            .sessions()
            .latency(Duration::from_millis(1), Duration::from_millis(30)),
        &ALL,
        |_| SessionLink::new(),
    );
    s.deliver_session_events();
    s
}

/// What `node` reported to the layer above, in order.
fn reported(s: &Sim<SessionLink<u32>>, node: NodeId) -> Vec<Ind<u32>> {
    s.trace().indications_at(node).cloned().collect()
}

fn delivered(s: &Sim<SessionLink<u32>>, node: NodeId) -> Vec<u32> {
    reported(s, node)
        .into_iter()
        .filter_map(|i| match i {
            Ind::Deliver { msg, .. } => Some(msg),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------- the handlers: task 3.1

#[test]
fn sending_puts_the_payload_on_the_wire_unchanged() {
    // No sequence number, no identifier: the session supplies ordering and reliability.
    let mut p: SessionLink<u32> = SessionLink::new();
    let fx = step(&mut p, Event::Cmd(Cmd::Send { to: B, msg: 9u32 }), Time::ZERO, &mut rng());
    assert_eq!(fx, vec![Effect::Send { to: B, msg: 9 }], "the thinnest possible link");
}

#[test]
fn receiving_delivers_without_deduplicating() {
    // Within a session the transport does not duplicate, so there is nothing to suppress — and
    // therefore nothing to remember.
    let mut p: SessionLink<u32> = SessionLink::new();
    let mut r = rng();
    let first = step(&mut p, Event::Msg { from: A, msg: 5u32 }, Time::ZERO, &mut r);
    assert_eq!(first, vec![Effect::Indicate(Ind::Deliver { from: A, msg: 5 })]);
    assert_eq!(p.tracked_peers(), 0, "receiving records nothing");
}

#[test]
fn state_does_not_grow_with_messages() {
    // The rule from docs/bounded-space.md, asserted rather than asserted-about.
    let mut p: SessionLink<u32> = SessionLink::new();
    let mut r = rng();
    for i in 0..1_000u32 {
        step(&mut p, Event::Cmd(Cmd::Send { to: B, msg: i }), Time::ZERO, &mut r);
        step(&mut p, Event::Msg { from: A, msg: i }, Time::ZERO, &mut r);
    }
    assert_eq!(p.tracked_peers(), 0, "a thousand messages, no per-message state");

    step(
        &mut p,
        Event::ScopeEvent(SessionEvent::Established { peer: A, epoch: 2 }),
        Time::ZERO,
        &mut r,
    );
    step(
        &mut p,
        Event::ScopeEvent(SessionEvent::Established { peer: B, epoch: 2 }),
        Time::ZERO,
        &mut r,
    );
    assert_eq!(p.tracked_peers(), 2, "state is one entry per peer, and that is all");
}

// --------------------------- within a session: tasks 3.2 and 3.4

#[test]
fn delivery_is_reliable_and_ordered_within_a_session() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        for i in 0..50u32 {
            s.command(A, Cmd::Send { to: B, msg: i });
        }
        s.run_until(Time::from_millis(3000));
        assert_eq!(
            delivered(&s, B),
            (0..50).collect::<Vec<_>>(),
            "seed {seed}: every message, in order, exactly once"
        );
    }
}

#[test]
fn nothing_is_delivered_that_was_not_sent() {
    let mut s = sim(1);
    s.command(A, Cmd::Send { to: B, msg: 7 });
    s.command(B, Cmd::Send { to: C, msg: 8 });
    s.run_until(Time::from_millis(500));

    assert_eq!(delivered(&s, B), vec![7]);
    assert_eq!(delivered(&s, C), vec![8]);
    assert!(delivered(&s, A).is_empty());
}

// ------------------------------- across a session: task 3.3

#[test]
fn a_session_ending_is_reported_with_the_peer_and_the_new_epoch() {
    let mut s = sim(2);
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.run_for(Duration::from_millis(100));
    let epoch_before = s.session_epoch(A, B).expect("a session");

    s.break_session(A, B);
    s.run_for(Duration::from_millis(50));

    // The ending names the epoch that ended; the establishment that follows names the next.
    let ended: Vec<(NodeId, u64)> = reported(&s, A)
        .into_iter()
        .filter_map(|i| match i {
            Ind::SessionEnded { peer, epoch } => Some((peer, epoch)),
            _ => None,
        })
        .collect();
    assert_eq!(ended, vec![(B, epoch_before)], "the ending names the epoch that ended");

    // A has sessions with every peer, because the link reconnects on its own rather than waiting
    // to be asked. Only B's are of interest here.
    let established: Vec<u64> = reported(&s, A)
        .into_iter()
        .filter_map(|i| match i {
            Ind::SessionEstablished { peer, epoch } if peer == B => Some(epoch),
            _ => None,
        })
        .collect();
    assert_eq!(
        established,
        vec![epoch_before, epoch_before + 1],
        "one establishment for the original session with B and one for its replacement"
    );
}

#[test]
fn a_lost_suffix_is_never_reported_as_delivered() {
    // Find a seed where the break actually loses something, then check the arithmetic.
    let outcome = |seed: u64| {
        let mut s = sim(seed);
        for i in 0..30u32 {
            s.command(A, Cmd::Send { to: B, msg: i });
        }
        s.run_for(Duration::from_millis(1));
        s.break_session(A, B);
        s.run_until(Time::from_millis(2000));
        (s.trace().suffix_losses(), delivered(&s, B))
    };
    let seed = (0..40u64).find(|s| outcome(*s).0 > 0).expect("a seed that loses something");
    let (lost, got) = outcome(seed);

    assert!(lost > 0);
    assert_eq!(got.len() + lost, 30, "every message either arrived or was lost, none invented");
    assert_eq!(got, (0..got.len() as u32).collect::<Vec<_>>(), "and what arrived is a prefix");
}

#[test]
fn delivery_resumes_in_the_new_session() {
    let mut s = sim(3);
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.run_for(Duration::from_millis(100));
    s.break_session(A, B);
    s.run_for(Duration::from_millis(50));

    for i in 100..110u32 {
        s.command(A, Cmd::Send { to: B, msg: i });
    }
    s.run_until(Time::from_millis(3000));

    let got = delivered(&s, B);
    let after: Vec<u32> = got.into_iter().filter(|n| *n >= 100).collect();
    assert_eq!(after, (100..110).collect::<Vec<_>>(), "the new session delivers normally");
}

#[test]
fn the_link_tracks_the_epoch_it_was_told() {
    let mut s = sim(4);
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.run_for(Duration::from_millis(100));
    s.break_session(A, B);
    s.run_for(Duration::from_millis(50));

    let epoch = s.protocol(A).unwrap().epoch(B).expect("A learned B's new epoch");
    assert_eq!(epoch, 2);
    // One entry per peer it has a session with — and it has one with everybody, because the link
    // establishes on its own. State is still bounded by membership, which is the point.
    assert_eq!(s.protocol(A).unwrap().tracked_peers(), ALL.len() - 1);
}

#[test]
fn a_crash_ends_the_session_and_the_survivor_is_told() {
    let mut s = sim(5);
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.run_for(Duration::from_millis(100));
    s.crash(B);
    s.run_for(Duration::from_millis(50));

    let endings =
        reported(&s, A).into_iter().filter(|i| matches!(i, Ind::SessionEnded { .. })).count();
    assert_eq!(endings, 1, "A is told its session with B is gone");
}

#[test]
fn a_dead_session_is_not_reported_as_current() {
    // `epoch()` answers "what is in force", not "what was the last number I saw". A layer asking
    // it is deciding whether it can send; answering with a dead session's epoch would say yes.
    let mut s = sim(21);
    s.run_for(Duration::from_millis(50));
    let live = s.protocol(A).unwrap().epoch(B).expect("a session with B is up");

    s.break_session(A, B);
    s.run_for(Duration::from_micros(1)); // the ending reaches the layer, nothing has reconnected
    assert_eq!(s.protocol(A).unwrap().epoch(B), None, "no session, so no epoch in force");

    s.run_for(Duration::from_millis(500));
    let again = s.protocol(A).unwrap().epoch(B).expect("the link reconnects on its own");
    assert!(again > live, "and the successor is a later epoch: {live} then {again}");
}
