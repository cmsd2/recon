//! `ReliableBroadcast` and `UniformReliableBroadcast` over a session link, against each other.
//!
//! The interesting content is the contrast: the same schedule must be able to leave a correct
//! process without a message under the reliable version, and must never do so under the uniform
//! one. That difference is what a failure detector buys.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::reliable_broadcast::{self as srb, ReliableBroadcast};
use recon_protocols::session_link::SessionLink;
use recon_protocols::stacks::{
    ReliableBroadcastOverSessions, UniformReliableBroadcastOverSessions,
};
use recon_protocols::uniform_reliable_broadcast::{
    self as surb, BroadcastId, UniformReliableBroadcast,
};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

const BOUND: Duration = Duration::from_millis(20);
fn heartbeat() -> Duration {
    BOUND * 2
}
fn detect_after() -> Duration {
    heartbeat() * 3
}

// The base modules over a session link. There is no separate session implementation any more —
// that is what the link port removed — so what this suite pins is that the one implementation,
// given a link that reports scope boundaries, draws the same contrast the two forks did.
type Rb = ReliableBroadcastOverSessions<u32>;
type Urb = UniformReliableBroadcastOverSessions<u32>;

fn rb_sim(seed: u64) -> Sim<Rb> {
    let mut s: Sim<Rb> =
        Sim::new(Config::default().seed(seed).sessions().synchronous(BOUND), &ALL, |me| {
            ReliableBroadcast::with_link(me, ALL, SessionLink::new())
        });
    s.deliver_session_events();
    s
}

fn urb_sim(seed: u64) -> Sim<Urb> {
    let mut s: Sim<Urb> =
        Sim::new(Config::default().seed(seed).sessions().synchronous(BOUND), &ALL, |me| {
            UniformReliableBroadcast::with_link(
                me,
                ALL,
                SessionLink::new(),
                heartbeat(),
                detect_after(),
            )
        });
    s.deliver_session_events();
    s
}

fn rb_delivered(s: &Sim<Rb>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            srb::Ind::Deliver { from, msg } => Some((*from, *msg)),
            _ => None,
        })
        .collect()
}

fn urb_delivered(s: &Sim<Urb>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            surb::Ind::Deliver { from, msg } => Some((*from, *msg)),
            _ => None,
        })
        .collect()
}

// ------------------------------------------- reliable broadcast: group 3

#[test]
fn rb_delivers_and_relays_once() {
    let mut s = rb_sim(1);
    s.run_for(Duration::from_millis(50));
    s.command(A, srb::Cmd::Broadcast(5));
    s.run_for(Duration::from_millis(500));
    for n in ALL {
        assert_eq!(rb_delivered(&s, n), vec![(A, 5)], "{n} delivers exactly once");
    }
}

#[test]
fn rb_a_repeat_neither_delivers_nor_relays() {
    // Every process relays to every process, so each one receives the same message four times.
    // The first receipt delivers and relays; the other three must do neither.
    let mut s = rb_sim(9);
    s.run_for(Duration::from_millis(50));
    let before = s.trace().send_count();
    s.command(A, srb::Cmd::Broadcast(3));
    s.run_for(Duration::from_millis(500));

    for n in ALL {
        let receipts = s.trace().deliveries().filter(|(_, to, _)| *to == n).count();
        assert!(receipts > 1, "{n} must actually see repeats for this to test anything");
        assert_eq!(rb_delivered(&s, n), vec![(A, 3)], "{n} delivered once");

        let sent = s
            .trace()
            .events()
            .iter()
            .filter(|e| matches!(e, recon_sim::TraceEvent::Sent { from, .. } if *from == n))
            .count();
        // A fans out once for the command and once more when its own copy comes back to it, as
        // eager reliable broadcast does; everyone else fans out once.
        let expected = if n == A { 2 * ALL.len() } else { ALL.len() };
        assert_eq!(sent, expected, "{n} relayed exactly once");
    }
    assert_eq!(s.trace().send_count() - before, ALL.len() * (ALL.len() + 1));
}

#[test]
fn rb_agreement_holds_while_sessions_hold_even_if_the_sender_crashes() {
    for seed in 0..10u64 {
        let mut s = rb_sim(seed);
        s.run_for(Duration::from_millis(50));
        s.command(A, srb::Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(200));
        s.crash(A);
        s.run_for(Duration::from_millis(500));

        for n in [B, C, D] {
            assert_eq!(rb_delivered(&s, n), vec![(A, 1)], "seed {seed}: {n}");
        }
    }
}

/// Cut D off for long enough that A's fan-out and B's and C's relays are all lost to the
/// session ending, then heal. D is a correct process throughout: it is reachable again well
/// before the run ends, and nothing is wrong with it.
///
/// Returns what B, C and D each delivered.
fn relay_lost_to_a_session_ending(seed: u64) -> Vec<usize> {
    let mut s = rb_sim(seed);
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C], &[D]]);
    s.command(A, srb::Cmd::Broadcast(1));
    s.run_for(BOUND * 8); // the fan-out and every relay have been and gone
    s.crash(A);
    s.heal();
    s.run_for(Duration::from_millis(2000));
    assert!(s.trace().session_ends() > 0, "the loss must be a session ending, not plain loss");
    [B, C, D].iter().map(|n| rb_delivered(&s, *n).len()).collect()
}

#[test]
fn rb_no_duplication_is_scoped_to_an_incarnation() {
    // `RB2 [incarnation]`, and the reason it cannot be `[always]`: `delivered` is a set of
    // identifiers held in memory. A recipient that crashes forgets what it delivered, and a relay
    // that arrives afterwards — every peer relays to every peer, and a session ending keeps a
    // random prefix of what was in flight — is a first receipt as far as it can tell.
    //
    // The corresponding perfect-link fact is
    // `no_duplication_does_not_survive_the_recipient_restarting`; this is the same argument one
    // layer up, and `docs/scope-annotated-modules.md` Corollary 7.2 is why neither can do better
    // without stable storage.
    let found = (0..80u64).find_map(|seed| {
        let mut s = rb_sim(seed);
        s.run_for(Duration::from_millis(50));
        s.command(A, srb::Cmd::Broadcast(5));
        s.run_for(Duration::from_millis(4)); // mid-fanout: relays to B are in flight
        s.crash(B);
        s.restart(B);
        s.run_for(Duration::from_millis(500));
        (rb_delivered(&s, B).len() > 1).then_some((seed, s))
    });

    let (seed, s) = found.expect("no seed delivered a relay to B after it had forgotten");
    assert_eq!(
        rb_delivered(&s, B),
        vec![(A, 5), (A, 5)],
        "seed {seed}: twice, because the set that would have stopped it did not survive"
    );
    for n in [A, C, D] {
        assert_eq!(rb_delivered(&s, n), vec![(A, 5)], "seed {seed}: {n} still delivers once");
    }
}

#[test]
fn rb_agreement_is_scoped_a_lost_relay_is_never_retried() {
    // The stated limit, demonstrated rather than asserted. B and C deliver; D is correct, is
    // reachable again long before the run ends, and never hears of the message, because reliable
    // broadcast relays once and keeps identifiers rather than payloads.
    let counts = relay_lost_to_a_session_ending(1);
    assert_eq!(counts, vec![1, 1, 0], "B and C delivered, D — correct and reachable — did not");
}

// ------------------------------- uniform reliable broadcast: group 4

#[test]
fn urb_delivers_when_all_sessions_hold() {
    let mut s = urb_sim(2);
    s.run_for(Duration::from_millis(50));
    s.command(A, surb::Cmd::Broadcast(7));
    s.run_for(Duration::from_millis(1000));
    for n in ALL {
        assert_eq!(urb_delivered(&s, n), vec![(A, 7)], "{n}");
    }
}

#[test]
fn urb_resends_on_re_establishment_with_the_peer_still_correct() {
    // The resend path: a break well inside the detection timeout resolves because what the peer
    // missed is sent again, and it is never accused.
    let mut s = urb_sim(3);
    s.run_for(Duration::from_millis(50));
    s.command(A, surb::Cmd::Broadcast(1));
    s.step_now(); // the command runs; its sends are in flight
    s.break_session(A, D);
    s.break_session(B, D);
    s.break_session(C, D);
    s.run_for(Duration::from_millis(1500));

    for n in ALL {
        assert_eq!(urb_delivered(&s, n), vec![(A, 1)], "{n} must still deliver");
        assert!(
            s.protocol(n).unwrap().correct().any(|p| p == D),
            "{n} must not have accused D — this is the resend path, not the accusation path"
        );
    }
}

#[test]
fn urb_progresses_by_accusation_when_a_peer_never_returns() {
    // The other path: a partition well outside the detection timeout resolves because D leaves
    // `correct`, with no resend having reached it.
    let mut s = urb_sim(4);
    s.run_for(Duration::from_millis(50));
    let cut = s.now();
    s.partition(&[&[A, B, C], &[D]]);
    s.command(A, surb::Cmd::Broadcast(1));
    s.run_for(detect_after() * 6);

    for n in [A, B, C] {
        assert_eq!(urb_delivered(&s, n), vec![(A, 1)], "{n} delivers once D is excluded");
        assert!(!s.protocol(n).unwrap().correct().any(|p| p == D), "{n} accused D");
    }
    // Nothing reached D at all, so its exclusion — not a resend — is what unblocked the others.
    let reached_d = s.trace().events().iter().any(
        |e| matches!(e, recon_sim::TraceEvent::Delivered { at, to, .. } if *to == D && *at >= cut),
    );
    assert!(!reached_d, "the accusation path must not be shadowed by a message getting through");

    // D, alone in a minority, eventually suspects everyone and becomes its own majority. That is
    // the detector's accuracy assumption being withdrawn, not a violation of uniform agreement:
    // every process that delivers, delivers the same message.
    assert!(urb_delivered(&s, D).len() <= 1);
}

#[test]
fn urb_liveness_does_not_need_the_layer_above_to_send() {
    // Nothing is broadcast after the partition heals: the link reconnects on its own, the
    // establishment is reported, and the resend follows.
    let mut s = urb_sim(5);
    s.run_for(Duration::from_millis(50));
    s.command(A, surb::Cmd::Broadcast(1));
    s.step_now(); // the command runs; its sends are in flight
    s.partition(&[&[A, B, C], &[D]]);
    s.run_for(Duration::from_millis(60)); // inside the detection timeout
    s.heal();
    s.run_for(Duration::from_millis(1500)); // and nobody broadcasts anything more

    for n in ALL {
        assert_eq!(urb_delivered(&s, n), vec![(A, 1)], "{n}");
    }
}

#[test]
fn urb_resends_only_on_an_establishment_and_only_to_that_peer() {
    // What a re-establishment costs, measured. `pending` is never pruned — Algorithm 3.4 does not
    // prune it — so the resend is one message per pending broadcast. It goes to the peer whose
    // session came back and to nobody else, and nothing is sent while no session is running.
    let mut s = urb_sim(6);
    s.run_for(Duration::from_millis(50));
    for v in [1u32, 2, 3] {
        s.command(A, surb::Cmd::Broadcast(v));
    }
    s.run_for(Duration::from_millis(900)); // everything delivered everywhere
    assert_eq!(s.protocol(A).unwrap().pending_count(), 3);

    let broke_at = s.now();
    s.break_session(A, D);
    s.run_for(Duration::from_millis(400));

    let payloads_from_a = |to: NodeId| {
        s.trace()
            .events()
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    recon_sim::TraceEvent::Sent { at, from, to: t, msg: surb::Wire::Broadcast(_) }
                        if *at > broke_at && *from == A && *t == to
                )
            })
            .count()
    };
    assert_eq!(payloads_from_a(D), 3, "one resend per pending broadcast, on re-establishment");
    for other in [B, C] {
        assert_eq!(payloads_from_a(other), 0, "{other}'s session never ended, so it hears nothing");
    }
}

#[test]
fn urb_adds_no_message_type_and_no_state_beyond_the_four_collections() {
    let mut s = urb_sim(11);
    s.run_for(Duration::from_millis(50));
    for v in [1u32, 2, 3] {
        s.command(A, surb::Cmd::Broadcast(v));
        s.run_for(Duration::from_millis(300));
    }
    s.run_for(Duration::from_millis(300));

    // Nothing on the wire but the broadcast payloads and the detector's heartbeats. In
    // particular there is no acknowledgement message: acknowledgement is inferred from who
    // relayed, exactly as in Algorithm 3.4.
    for (_, _, m) in s.trace().sends() {
        match m {
            surb::Wire::Broadcast(_) | surb::Wire::Detector(_) => {}
        }
    }

    for n in ALL {
        let p = s.protocol(n).unwrap();
        assert_eq!(p.delivered_count(), 3, "{n} delivered each broadcast once");
        // `pending` is keyed by broadcast identifier, not by receipt: three broadcasts seen four
        // times each is still three entries.
        assert_eq!(p.pending_count(), 3, "{n} holds one entry per message, not per receipt");
        for seq in 1..=3u64 {
            let id = BroadcastId { origin: A, seq };
            let acked: Vec<NodeId> = p.acknowledged_by(id).collect();
            assert_eq!(acked, ALL.to_vec(), "{n} saw every process relay {seq}");
        }
    }
}

#[test]
fn urb_uniform_agreement_holds_across_endings_and_re_establishment() {
    for seed in 0..8u64 {
        let mut s = urb_sim(seed);
        s.run_for(Duration::from_millis(50));
        s.command(A, surb::Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(3));
        s.break_session(A, D);
        s.run_for(Duration::from_millis(7));
        s.break_session(B, C);
        s.run_for(Duration::from_millis(9));
        s.break_session(C, D);
        s.run_for(detect_after() * 6);

        let sets: Vec<Vec<(NodeId, u32)>> = ALL.iter().map(|n| urb_delivered(&s, *n)).collect();
        assert!(sets.iter().all(|d| *d == vec![(A, 1)]), "seed {seed}: {sets:?}");
        for n in ALL {
            assert!(
                s.protocol(n).unwrap().correct().count() == ALL.len(),
                "seed {seed}: {n} accused nobody — these were endings, not crashes"
            );
        }
    }
}

#[test]
fn urb_attempts_nothing_on_the_ending_itself() {
    let mut s = urb_sim(12);
    s.run_for(Duration::from_millis(50));
    s.command(A, surb::Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(2));
    s.break_session(A, D);
    s.run_for(Duration::from_millis(400));

    // Between A's session with D ending and the next one opening there is nothing to send over,
    // so nothing must be attempted. Any resend belongs to the establishment, not the ending.
    let ended = s
        .trace()
        .events()
        .iter()
        .find_map(|e| match e {
            recon_sim::TraceEvent::SessionEnded { at, a, b, .. }
                if (*a, *b) == (A, D) || (*a, *b) == (D, A) =>
            {
                Some(*at)
            }
            _ => None,
        })
        .expect("the break must be recorded");
    let reopened = s
        .trace()
        .events()
        .iter()
        .find_map(|e| match e {
            recon_sim::TraceEvent::SessionOpened { at, a, b, .. }
                if *at > ended && ((*a, *b) == (A, D) || (*a, *b) == (D, A)) =>
            {
                Some(*at)
            }
            _ => None,
        })
        .expect("the link reconnects on its own");
    let between = s.trace().events().iter().filter(|e| {
        matches!(
            e,
            recon_sim::TraceEvent::Sent { at, from, to, .. }
                if *at >= ended && *at < reopened && *from == A && *to == D
        )
    });
    assert_eq!(between.count(), 0, "nothing sent to D while there was no session to send over");
}

// ------------------------------------- that the two abstractions differ: group 5

#[test]
fn the_uniform_version_survives_a_schedule_that_splits_the_reliable_one() {
    // Reliable broadcast leaves D without the message on this schedule.
    assert_eq!(relay_lost_to_a_session_ending(1), vec![1, 1, 0]);

    // The same schedule against the uniform version. Sessions re-establish after the heal, what
    // D missed is sent again, and no correct process is left behind.
    let mut s = urb_sim(1);
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C], &[D]]);
    s.command(A, surb::Cmd::Broadcast(1));
    s.run_for(BOUND * 8);
    s.crash(A);
    s.heal();
    s.run_for(detect_after() * 8);

    assert!(s.trace().session_ends() > 0, "the same loss happened");
    let counts: Vec<usize> = [B, C, D].iter().map(|n| urb_delivered(&s, *n).len()).collect();
    assert_eq!(counts, vec![1, 1, 1], "no correct process is left without the message");
}

#[test]
fn the_agreement_assertions_are_not_vacuous() {
    let delivering = (0..20u64)
        .filter(|seed| {
            let mut s = urb_sim(*seed);
            s.run_for(Duration::from_millis(50));
            s.command(A, surb::Cmd::Broadcast(1));
            s.run_for(Duration::from_millis(800));
            ALL.iter().any(|n| !urb_delivered(&s, *n).is_empty())
        })
        .count();
    assert!(delivering > 0, "no seed delivered anything — the assertions pass vacuously");
}

#[test]
fn the_difference_is_attributable_to_resending_and_accusation() {
    // Uniform reliable broadcast has two ways out of a lost relay, and both are observable.
    // Reliable broadcast has neither: it keeps no payload to resend and consults no detector.
    let mut s = urb_sim(1);
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C], &[D]]);
    s.command(A, surb::Cmd::Broadcast(1));
    s.run_for(BOUND * 3); // long enough to lose every relay, well inside the detection timeout
    let before_heal = s.now();
    s.heal();
    s.run_for(detect_after() * 8);

    // D still holds nothing at the moment of healing, and everyone still calls it correct, so
    // what follows is the resend and not an accusation.
    assert!(ALL.iter().all(|n| s.protocol(*n).unwrap().correct().any(|p| p == D)));
    let resent = s.trace().events().iter().any(|e| {
        matches!(
            e,
            recon_sim::TraceEvent::Delivered { at, to, msg: surb::Wire::Broadcast(_), .. }
                if *to == D && *at > before_heal
        )
    });
    assert!(resent, "the payload D missed crossed the new session");

    // And the reliable version, run through the same schedule, resends nothing at all after the
    // heal — it has no payload left to send.
    let mut r = rb_sim(1);
    r.run_for(Duration::from_millis(50));
    r.partition(&[&[A, B, C], &[D]]);
    r.command(A, srb::Cmd::Broadcast(1));
    r.run_for(BOUND * 8);
    let heal_at = r.now();
    r.heal();
    r.run_for(Duration::from_millis(2000));
    let late = r
        .trace()
        .events()
        .iter()
        .filter(|e| matches!(e, recon_sim::TraceEvent::Sent { at, to, .. } if *to == D && *at > heal_at))
        .count();
    assert_eq!(late, 0, "reliable broadcast has no mechanism to send D anything after the heal");
}

#[test]
fn every_absence_property_is_paired_with_a_minimum_delivery_count() {
    // The absence properties above — no duplicate delivery, no creation, no split — are all
    // satisfied by a run that delivers nothing. Assert the floor explicitly.
    let mut s = urb_sim(13);
    s.run_for(Duration::from_millis(50));
    for v in [1u32, 2, 3] {
        s.command(A, surb::Cmd::Broadcast(v));
    }
    s.run_for(Duration::from_millis(1500));
    for n in ALL {
        assert_eq!(urb_delivered(&s, n), vec![(A, 1), (A, 2), (A, 3)], "{n}");
    }

    let mut r = rb_sim(13);
    r.run_for(Duration::from_millis(50));
    for v in [1u32, 2, 3] {
        r.command(A, srb::Cmd::Broadcast(v));
    }
    r.run_for(Duration::from_millis(1500));
    for n in ALL {
        assert_eq!(rb_delivered(&r, n), vec![(A, 1), (A, 2), (A, 3)], "{n}");
    }
}
