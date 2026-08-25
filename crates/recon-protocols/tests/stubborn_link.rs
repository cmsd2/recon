//! The stubborn link against its stated guarantees (Module 2.2): stubborn delivery and
//! no creation.

use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, NodeId, Time, step};
use recon_protocols::stubborn_link::{Cmd, Ind, Retransmit, SendId, StubbornLink};
use recon_sim::{Config, DropReason, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);

type Payload = u32;

fn interval() -> Duration {
    Duration::from_millis(10)
}

fn link() -> StubbornLink<Payload> {
    StubbornLink::new(interval())
}

fn sim(config: Config) -> Sim<StubbornLink<Payload>> {
    Sim::new(config, &[A, B], |_| link())
}

fn rng() -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(0)
}

// ------------------------------------------------------ Retransmission: task 4.1

#[test]
fn a_send_transmits_immediately_and_arms_the_timer() {
    let mut p = link();
    let fx = step(
        &mut p,
        Event::Cmd(Cmd::Send { id: SendId(1), to: B, msg: 7 }),
        Time::ZERO,
        &mut rng(),
    );
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: 7 },
            Effect::SetTimer { after: interval(), token: Retransmit },
        ]
    );
    assert_eq!(p.outstanding(), 1);
}

#[test]
fn the_timer_retransmits_everything_outstanding_and_re_arms() {
    let mut p = link();
    let mut r = rng();
    step(&mut p, Event::Cmd(Cmd::Send { id: SendId(1), to: B, msg: 1 }), Time::ZERO, &mut r);
    step(&mut p, Event::Cmd(Cmd::Send { id: SendId(2), to: B, msg: 2 }), Time::ZERO, &mut r);

    let fx = step(&mut p, Event::Timer(Retransmit), Time::from_millis(10), &mut r);
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: 1 },
            Effect::Send { to: B, msg: 2 },
            Effect::SetTimer { after: interval(), token: Retransmit },
        ],
        "every outstanding transmission is resent, and the timer re-arms"
    );
}

#[test]
fn a_second_send_does_not_arm_a_second_timer() {
    // Two timers would double the retransmission rate and compound on every send.
    let mut p = link();
    let mut r = rng();
    step(&mut p, Event::Cmd(Cmd::Send { id: SendId(1), to: B, msg: 1 }), Time::ZERO, &mut r);
    let fx = step(&mut p, Event::Cmd(Cmd::Send { id: SendId(2), to: B, msg: 2 }), Time::ZERO, &mut r);
    assert_eq!(fx, vec![Effect::Send { to: B, msg: 2 }], "no second timer");
}

#[test]
fn retransmission_ceases_when_stopped() {
    let mut p = link();
    let mut r = rng();
    step(&mut p, Event::Cmd(Cmd::Send { id: SendId(1), to: B, msg: 1 }), Time::ZERO, &mut r);
    step(&mut p, Event::Cmd(Cmd::Stop { id: SendId(1) }), Time::ZERO, &mut r);
    assert_eq!(p.outstanding(), 0);

    let fx = step(&mut p, Event::Timer(Retransmit), Time::from_millis(10), &mut r);
    assert_eq!(fx, vec![], "nothing outstanding, so nothing resent and no timer re-armed");
}

#[test]
fn stopping_one_transmission_leaves_the_others() {
    let mut p = link();
    let mut r = rng();
    step(&mut p, Event::Cmd(Cmd::Send { id: SendId(1), to: B, msg: 1 }), Time::ZERO, &mut r);
    step(&mut p, Event::Cmd(Cmd::Send { id: SendId(2), to: B, msg: 2 }), Time::ZERO, &mut r);
    step(&mut p, Event::Cmd(Cmd::Stop { id: SendId(1) }), Time::ZERO, &mut r);

    let fx = step(&mut p, Event::Timer(Retransmit), Time::from_millis(10), &mut r);
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: 2 },
            Effect::SetTimer { after: interval(), token: Retransmit },
        ]
    );
}

#[test]
fn transmission_repeats_over_time() {
    let mut s = sim(Config::default().seed(1));
    s.command(A, Cmd::Send { id: SendId(1), to: B, msg: 42 });
    s.run_until(Time::from_millis(105));

    let sends = s.trace().sends().count();
    // One immediate send plus one per interval elapsed.
    assert!(sends >= 10, "expected repeated transmission, saw {sends}");
    assert_eq!(s.protocol(A).unwrap().outstanding(), 1);
}

#[test]
fn a_stopped_transmission_stops_in_a_run() {
    let mut s = sim(Config::default().seed(1));
    s.command(A, Cmd::Send { id: SendId(1), to: B, msg: 42 });
    s.run_until(Time::from_millis(50));
    let before = s.trace().sends().count();

    s.command(A, Cmd::Stop { id: SendId(1) });
    s.run_until(Time::from_millis(500));
    let after = s.trace().sends().count();

    assert!(before >= 4, "should have retransmitted before stopping, saw {before}");
    assert!(
        after - before <= 1,
        "after stopping, transmissions must cease: {before} then {after}"
    );
}

// -------------------------------------------------- Stubborn delivery: task 4.2

#[test]
fn a_message_survives_heavy_loss() {
    // SL1: a correct process sending to a correct process gets through, eventually.
    let mut s = sim(Config::default().seed(4).loss(0.9));
    s.command(A, Cmd::Send { id: SendId(1), to: B, msg: 99 });
    s.run_until(Time::from_millis(1000));

    assert!(s.trace().drops_because(DropReason::Lost) > 0, "the run must actually be lossy");
    let delivered: Vec<_> = s.trace().indications_at(B).collect();
    assert!(!delivered.is_empty(), "the message must arrive despite 90% loss");
    assert!(delivered.iter().all(|i| **i == Ind::Deliver { from: A, msg: 99 }));
}

#[test]
fn delivery_repeats_infinitely_often() {
    // The distinguishing property of a stubborn link, and the reason a perfect link is needed.
    let mut s = sim(Config::default().seed(5));
    s.command(A, Cmd::Send { id: SendId(1), to: B, msg: 3 });
    s.run_until(Time::from_millis(500));
    let n = s.trace().indications_at(B).count();
    assert!(n > 10, "expected repeated delivery, saw {n}");
}

#[test]
fn delivery_resumes_after_a_partition_heals() {
    let mut s = sim(Config::default().seed(6));
    s.partition(&[&[A], &[B]]);
    s.command(A, Cmd::Send { id: SendId(1), to: B, msg: 5 });
    s.run_until(Time::from_millis(200));

    assert_eq!(s.trace().indications_at(B).count(), 0, "nothing crosses a partition");
    assert!(s.trace().drops_because(DropReason::Partitioned) > 0);

    s.heal();
    s.run_until(Time::from_millis(400));
    assert!(
        s.trace().indications_at(B).count() > 0,
        "retransmission must get through once the partition heals"
    );
}

#[test]
fn no_delivery_is_required_to_a_crashed_process() {
    let mut s = sim(Config::default().seed(7));
    s.crash(B);
    s.command(A, Cmd::Send { id: SendId(1), to: B, msg: 5 });
    s.run_until(Time::from_millis(200));

    assert_eq!(s.trace().indications_at(B).count(), 0);
    assert!(s.trace().drops_because(DropReason::RecipientCrashed) > 0);
    // The sender keeps trying rather than failing.
    assert_eq!(s.protocol(A).unwrap().outstanding(), 1);
    assert!(s.trace().sends().count() > 5);
}

// --------------------------------------------------------- No creation: task 4.3

#[test]
fn every_delivery_corresponds_to_an_earlier_send() {
    let mut s = sim(
        Config::default()
            .seed(8)
            .loss(0.3)
            .duplication(0.3)
            .latency(Duration::from_millis(1), Duration::from_millis(15)),
    );
    s.command(A, Cmd::Send { id: SendId(1), to: B, msg: 11 });
    s.command(B, Cmd::Send { id: SendId(2), to: A, msg: 22 });
    s.run_until(Time::from_millis(400));

    // Nothing is delivered that was not sent, by the process it is attributed to.
    let mut sent_pairs: Vec<(NodeId, NodeId, Payload)> =
        s.trace().sends().map(|(f, t, m)| (f, t, *m)).collect();
    sent_pairs.sort();
    sent_pairs.dedup();

    for (from, to, msg) in s.trace().deliveries().map(|(f, t, m)| (f, t, *m)) {
        assert!(
            sent_pairs.contains(&(from, to, msg)),
            "delivered {msg} from {from} to {to}, which was never sent"
        );
    }

    // And the indication attributes it to the true sender.
    for ind in s.trace().indications_at(B) {
        assert_eq!(*ind, Ind::Deliver { from: A, msg: 11 });
    }
    for ind in s.trace().indications_at(A) {
        assert_eq!(*ind, Ind::Deliver { from: B, msg: 22 });
    }
}

#[test]
fn nothing_is_delivered_when_nothing_is_sent() {
    let mut s = sim(Config::default().seed(9).duplication(0.5));
    s.run_until(Time::from_millis(500));
    assert_eq!(s.trace().delivery_count(), 0);
    assert_eq!(s.trace().indication_count(), 0);
}
