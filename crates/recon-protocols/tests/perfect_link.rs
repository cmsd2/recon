//! The perfect link against its stated guarantees (Module 2.3): reliable delivery,
//! no duplication, no creation.

use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, NodeId, Time, step};
use recon_protocols::perfect_link::{Cmd, Ind, MsgId, PerfectLink, Timer, Wire};
use recon_protocols::stubborn_link::Retransmit;
use recon_sim::{Config, DropReason, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);

fn interval() -> Duration {
    Duration::from_millis(10)
}

fn sim<P>(config: Config) -> Sim<PerfectLink<P>>
where
    P: Clone + PartialEq,
{
    Sim::new(config, &[A, B], |me| PerfectLink::new(me, interval()))
}

fn rng() -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(0)
}

// ------------------------------------------------ The wire header: task 5.1

#[test]
fn the_wire_carries_exactly_one_identifier_and_the_payload() {
    let mut p: PerfectLink<u32> = PerfectLink::new(A, interval());
    let fx = step(&mut p, Event::Cmd(Cmd::Send { to: B, msg: 7u32 }), Time::ZERO, &mut rng());

    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: Wire { id: MsgId { src: A, seq: 1 }, payload: 7 } },
            Effect::SetTimer { after: interval(), token: Timer::Stubborn(Retransmit) },
        ],
        "one identifier, one payload, and the child's timer re-wrapped"
    );
}

#[test]
fn identifiers_advance_per_send() {
    let mut p: PerfectLink<u32> = PerfectLink::new(A, interval());
    let mut r = rng();
    let ids: Vec<u64> = (0..3)
        .flat_map(|_| step(&mut p, Event::Cmd(Cmd::Send { to: B, msg: 0u32 }), Time::ZERO, &mut r))
        .filter_map(|e| match e {
            Effect::Send { msg: Wire { id, .. }, .. } => Some(id.seq),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn the_wire_survives_encoding() {
    let w = Wire { id: MsgId { src: A, seq: 3 }, payload: 9u32 };
    assert_eq!(recon_sim::codec::round_trip(&w).expect("round trip"), w);
}

// --------------------------- Reliable delivery and no duplication: task 5.2

#[test]
fn a_message_arrives_exactly_once_despite_loss() {
    let mut s = sim::<u32>(Config::default().seed(3).loss(0.8));
    s.command(A, Cmd::Send { to: B, msg: 5 });
    s.run_until(Time::from_millis(1000));

    assert!(s.trace().drops_because(DropReason::Lost) > 0, "the run must actually be lossy");
    let got: Vec<_> = s.trace().indications_at(B).collect();
    assert_eq!(got.len(), 1, "exactly once, not zero and not many");
    assert_eq!(*got[0], Ind::Deliver { from: A, msg: 5 });
}

#[test]
fn network_duplication_is_suppressed() {
    let mut s = sim::<u32>(Config::default().seed(4).duplication(1.0));
    s.command(A, Cmd::Send { to: B, msg: 5 });
    s.run_until(Time::from_millis(200));

    assert!(s.trace().duplicates() > 0, "the network must actually have duplicated");
    assert_eq!(s.trace().indications_at(B).count(), 1);
}

#[test]
fn retransmission_is_suppressed() {
    // The stubborn link below delivers infinitely often; exactly one must surface.
    let mut s = sim::<u32>(Config::default().seed(5));
    s.command(A, Cmd::Send { to: B, msg: 5 });
    s.run_until(Time::from_millis(500));

    assert!(s.trace().delivery_count() > 10, "the layer below must have retransmitted");
    assert_eq!(s.trace().indications_at(B).count(), 1, "and exactly one delivery surfaced");
}

#[test]
fn many_messages_all_arrive_exactly_once() {
    let mut s = sim::<u32>(
        Config::default()
            .seed(6)
            .loss(0.4)
            .duplication(0.3)
            .latency(Duration::from_millis(1), Duration::from_millis(12)),
    );
    for i in 0..20u32 {
        s.command(A, Cmd::Send { to: B, msg: i });
    }
    s.run_until(Time::from_millis(2000));

    let mut got: Vec<u32> =
        s.trace().indications_at(B).map(|Ind::Deliver { msg, .. }| *msg).collect();
    got.sort();
    assert_eq!(got, (0..20).collect::<Vec<_>>(), "every message exactly once");
    assert_eq!(s.protocol(B).unwrap().delivered_count(), 20);
}

// ------------------------------- A genuine resend is not swallowed: task 5.3

#[test]
fn identical_content_sent_twice_is_delivered_twice() {
    // The book deduplicates on message content, which swallows this. Deduplicating on an
    // identifier is what makes it work.
    let mut s = sim::<u32>(Config::default().seed(7));
    s.command(A, Cmd::Send { to: B, msg: 42 });
    s.command(A, Cmd::Send { to: B, msg: 42 });
    s.run_until(Time::from_millis(300));

    let got: Vec<_> = s.trace().indications_at(B).collect();
    assert_eq!(got.len(), 2, "two separate sends of identical content are two deliveries");
    assert!(got.iter().all(|i| **i == Ind::Deliver { from: A, msg: 42 }));
}

#[test]
fn identical_content_sent_twice_survives_loss_and_duplication() {
    let mut s = sim::<u32>(Config::default().seed(8).loss(0.5).duplication(0.5));
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.run_until(Time::from_millis(2000));
    assert_eq!(s.trace().indications_at(B).count(), 3);
}

// --------------------------------------------------------- No creation

#[test]
fn every_indication_corresponds_to_a_send() {
    let mut s = sim::<u32>(Config::default().seed(9).loss(0.3).duplication(0.3));
    s.command(A, Cmd::Send { to: B, msg: 11 });
    s.command(B, Cmd::Send { to: A, msg: 22 });
    s.run_until(Time::from_millis(1000));

    for ind in s.trace().indications_at(B) {
        assert_eq!(*ind, Ind::Deliver { from: A, msg: 11 });
    }
    for ind in s.trace().indications_at(A) {
        assert_eq!(*ind, Ind::Deliver { from: B, msg: 22 });
    }
}

#[test]
fn nothing_is_delivered_when_nothing_is_sent() {
    let mut s = sim::<u32>(Config::default().seed(10).duplication(0.5));
    s.run_until(Time::from_millis(500));
    assert_eq!(s.trace().indication_count(), 0);
}

// ------------------------------------- Isolation with a stand-in payload: task 5.4

/// A payload type that exists only for this test — nothing is layered above the link.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StandIn {
    label: &'static str,
}

#[test]
fn the_link_is_testable_with_no_layer_above_it() {
    let mut p: PerfectLink<StandIn> = PerfectLink::new(A, interval());
    let mut r = rng();
    let payload = StandIn { label: "arbitrary" };

    let fx =
        step(&mut p, Event::Cmd(Cmd::Send { to: B, msg: payload.clone() }), Time::ZERO, &mut r);
    assert!(matches!(fx[0], Effect::Send { to: B, .. }));

    // And an arriving copy surfaces exactly once, however many times it arrives.
    let wire = Wire { id: MsgId { src: B, seq: 1 }, payload: payload.clone() };
    let first = step(&mut p, Event::Msg { from: B, msg: wire.clone() }, Time::ZERO, &mut r);
    let second = step(&mut p, Event::Msg { from: B, msg: wire }, Time::ZERO, &mut r);

    assert_eq!(first, vec![Effect::Indicate(Ind::Deliver { from: B, msg: payload })]);
    assert_eq!(second, vec![], "the second copy is suppressed");
}

#[test]
fn deduplication_is_per_identifier_not_per_sender() {
    let mut p: PerfectLink<u32> = PerfectLink::new(A, interval());
    let mut r = rng();
    let one = Wire { id: MsgId { src: B, seq: 1 }, payload: 0u32 };
    let two = Wire { id: MsgId { src: B, seq: 2 }, payload: 0u32 };

    assert_eq!(step(&mut p, Event::Msg { from: B, msg: one }, Time::ZERO, &mut r).len(), 1);
    assert_eq!(
        step(&mut p, Event::Msg { from: B, msg: two }, Time::ZERO, &mut r).len(),
        1,
        "a different identifier with identical content is a different message"
    );
    assert_eq!(p.delivered_count(), 2);
}

// ------------------------- The scope of PL2, made observable

#[test]
fn no_duplication_does_not_survive_the_recipient_restarting() {
    // PL2 [incarnation(q)] — the deduplication set is volatile, so a process that crashes and
    // restarts has forgotten what it delivered and will deliver it again. The book states
    // "no message is delivered by a process more than once" without qualification; this is the
    // scope that qualification hides.
    let mut p: PerfectLink<u32> = PerfectLink::new(B, interval());
    let mut r = rng();
    let wire = Wire { id: MsgId { src: A, seq: 1 }, payload: 5u32 };

    let first = step(&mut p, Event::Msg { from: A, msg: wire.clone() }, Time::ZERO, &mut r);
    let again = step(&mut p, Event::Msg { from: A, msg: wire.clone() }, Time::ZERO, &mut r);
    assert_eq!(first.len(), 1);
    assert_eq!(again.len(), 0, "within one incarnation, the duplicate is suppressed");

    // The process restarts: fresh state, nothing remembered.
    let mut p: PerfectLink<u32> = PerfectLink::new(B, interval());
    let after_restart = step(&mut p, Event::Msg { from: A, msg: wire }, Time::ZERO, &mut r);
    assert_eq!(
        after_restart.len(),
        1,
        "across incarnations it is delivered again — PL2 is scoped, not absolute"
    );
}

#[test]
fn a_restarted_recipient_redelivers_in_a_run() {
    // The same thing end to end, driven by the simulator's crash rather than by hand.
    let mut s = sim::<u32>(Config::default().seed(11));
    s.command(A, Cmd::Send { to: B, msg: 5 });
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().indications_at(B).count(), 1);

    // B dies and comes back empty; A is still stubbornly retransmitting.
    s.crash(B);
    s.restart(B);
    s.run_until(Time::from_millis(400));

    assert!(
        s.trace().indications_at(B).count() > 1,
        "a restarted recipient re-delivers what it had already delivered"
    );
}

#[test]
fn no_duplication_holds_across_a_suspension() {
    // Contrast: a pause preserves the deduplication set, so PL2 holds across it.
    let mut s = sim::<u32>(Config::default().seed(12));
    s.command(A, Cmd::Send { to: B, msg: 5 });
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().indications_at(B).count(), 1);

    s.suspend(B);
    s.run_until(Time::from_millis(200));
    s.restart(B);
    s.run_until(Time::from_millis(400));

    assert_eq!(
        s.trace().indications_at(B).count(),
        1,
        "suspension is within the incarnation, so no duplication still holds"
    );
}
