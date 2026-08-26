//! The perfect failure detector against Module 2.6: strong completeness and strong accuracy —
//! and the loss of accuracy when the timing assumption it rests on is withdrawn.

use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, NodeId, Protocol, Time, step};
use recon_protocols::perfect_failure_detector::{Heartbeat, Ind, PerfectFailureDetector, Tick};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

/// The network's promise. Everything else is derived from it rather than guessed.
const BOUND: Duration = Duration::from_millis(20);

/// Announce this often.
fn period() -> Duration {
    BOUND * 2
}

/// Accuse after this much silence. Accuracy needs timeout > period + BOUND, with margin.
fn timeout() -> Duration {
    period() * 3
}

fn detector(me: NodeId) -> PerfectFailureDetector {
    PerfectFailureDetector::new(me, ALL, period(), timeout())
}

fn rng() -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(0)
}

/// A synchronous run, with every detector configured from the network's own bound.
fn sync_sim(seed: u64) -> Sim<PerfectFailureDetector> {
    let s: Sim<PerfectFailureDetector> =
        Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, detector);
    assert_eq!(s.delivery_bound(), Some(BOUND), "the detector must be configured from this");
    s
}

/// Which processes `node` has accused, in order.
fn accused(s: &Sim<PerfectFailureDetector>, node: NodeId) -> Vec<NodeId> {
    s.trace().indications_at(node).map(|Ind::Crash { node }| *node).collect()
}

// --------------------------------------------------- handlers: tasks 2.1 to 2.3

#[test]
fn the_heartbeat_survives_encoding() {
    assert_eq!(recon_sim::codec::round_trip(&Heartbeat).expect("round trip"), Heartbeat);
}

#[test]
fn initialising_beats_to_every_peer_and_arms_the_timer() {
    let mut p = detector(A);
    let fx = step(&mut p, Event::Init, Time::ZERO, &mut rng());
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: Heartbeat },
            Effect::Send { to: C, msg: Heartbeat },
            Effect::Send { to: D, msg: Heartbeat },
            Effect::SetTimer { after: period(), token: Tick },
        ],
        "a heartbeat to each peer but not to itself, then the timer"
    );
}

#[test]
fn initialising_twice_does_not_arm_a_second_timer() {
    // Two timers would halve the effective timeout and accuse the living. The simulator
    // initialises each process exactly once, so this guards the protocol rather than the driver.
    let mut p = detector(A);
    let mut r = rng();
    step(&mut p, Event::Init, Time::ZERO, &mut r);
    let again = step(&mut p, Event::Init, Time::ZERO, &mut r);
    assert_eq!(again, vec![]);
}

#[test]
fn a_heartbeat_before_the_tick_prevents_accusation() {
    let mut p = detector(A);
    let mut r = rng();
    step(&mut p, Event::Init, Time::ZERO, &mut r);
    for peer in [B, C, D] {
        step(&mut p, Event::Msg { from: peer, msg: Heartbeat }, Time::from_millis(5), &mut r);
    }
    let fx = step(&mut p, Event::Timer(Tick), Time::from_offset(period()), &mut r);
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::Indicate(_))),
        "everyone was heard from, so nobody is accused"
    );
}

#[test]
fn silence_beyond_the_timeout_accuses_exactly_once() {
    let mut p = detector(A);
    let mut r = rng();
    step(&mut p, Event::Init, Time::ZERO, &mut r);

    let past = Time::from_offset(timeout() * 2);
    step(&mut p, Event::Msg { from: B, msg: Heartbeat }, past, &mut r);

    let first = step(&mut p, Event::Timer(Tick), past, &mut r);
    let accusations: Vec<_> = first
        .iter()
        .filter_map(|e| match e {
            Effect::Indicate(Ind::Crash { node }) => Some(*node),
            _ => None,
        })
        .collect();
    assert_eq!(accusations, vec![C, D], "the two long-silent peers, in a stable order");

    // B keeps announcing itself; C and D stay silent but are already reported.
    let later = Time::from_offset(timeout() * 3);
    step(&mut p, Event::Msg { from: B, msg: Heartbeat }, later, &mut r);
    let second = step(&mut p, Event::Timer(Tick), later, &mut r);
    assert!(!second.iter().any(|e| matches!(e, Effect::Indicate(_))), "reported once, not again");
    assert!(p.has_detected(C) && p.has_detected(D) && !p.has_detected(B));
}

// ------------------------------------- strong completeness: tasks 3.1 and 3.3

#[test]
fn every_crashed_process_is_detected_by_every_survivor() {
    for seed in 0..10u64 {
        let mut s = sync_sim(seed);
        s.run_for(Duration::from_millis(100));
        s.crash(D);
        s.run_for(timeout() * 4);

        for n in [A, B, C] {
            assert_eq!(accused(&s, n), vec![D], "seed {seed}: {n} must accuse exactly D");
        }
    }
}

#[test]
fn several_crashes_are_all_detected() {
    let mut s = sync_sim(11);
    s.run_for(Duration::from_millis(100));
    s.crash(C);
    s.crash(D);
    s.run_for(timeout() * 4);

    for n in [A, B] {
        let mut got = accused(&s, n);
        got.sort();
        assert_eq!(got, vec![C, D]);
    }
}

#[test]
fn the_detector_has_no_commands_at_all() {
    // Detection begins at initialisation, as Module 2.6 has it, so there is nothing to ask for.
    // An uninhabited command type says so in a way the compiler checks.
    fn _absurd(c: <PerfectFailureDetector as Protocol>::Cmd) -> ! {
        match c {}
    }
    let mut p = detector(A);
    let fx = step(&mut p, Event::Init, Time::ZERO, &mut rng());
    assert!(!fx.is_empty(), "and initialising is what starts it");
}

#[test]
fn detection_is_permanent_and_reported_once() {
    let mut s = sync_sim(12);
    s.run_for(Duration::from_millis(100));
    s.crash(D);
    s.run_for(timeout() * 12);

    for n in [A, B, C] {
        assert_eq!(accused(&s, n), vec![D], "{n} must report D exactly once however long we run");
        assert!(s.protocol(n).unwrap().has_detected(D));
        assert!(!s.protocol(n).unwrap().correct().any(|p| p == D));
    }
}

#[test]
fn a_brief_suspension_is_not_an_accusation() {
    // The boundary the timeout actually tests: silence shorter than the timeout is tolerated.
    let mut s = sync_sim(13);
    s.run_for(Duration::from_millis(100));
    s.suspend(D);
    s.run_for(timeout() - period() - BOUND * 2);
    s.restart(D);
    s.run_for(timeout() * 3);

    for n in [A, B, C] {
        assert!(accused(&s, n).is_empty(), "{n} accused D over a brief pause");
    }
}

#[test]
fn a_long_suspension_is_an_accusation() {
    // ...and silence longer than the timeout is not, which is what makes the test above meaningful.
    let mut s = sync_sim(14);
    s.run_for(Duration::from_millis(100));
    s.suspend(D);
    s.run_for(timeout() * 3);

    for n in [A, B, C] {
        assert_eq!(accused(&s, n), vec![D], "{n} should have accused a long-silent D");
    }
}

// --------------------------------------------------- strong accuracy: task 3.2

#[test]
fn no_correct_process_is_ever_accused() {
    for seed in 0..20u64 {
        let mut s = sync_sim(seed);
        s.run_for(timeout() * 20);
        for n in ALL {
            assert!(
                accused(&s, n).is_empty(),
                "seed {seed}: {n} accused {:?} with every process correct",
                accused(&s, n)
            );
        }
    }
}

#[test]
fn accuracy_holds_over_a_long_run() {
    let mut s = sync_sim(21);
    s.run_until(Time::from_secs(10));
    for n in ALL {
        assert!(accused(&s, n).is_empty());
    }
    assert!(s.trace().delivery_count() > 100, "the run must actually have exchanged heartbeats");
}

// ------------------------- accuracy depends on the assumption: task 3.4

#[test]
fn accuracy_is_lost_when_the_timing_assumption_is_withdrawn() {
    // The same detector on the asynchronous default. It accuses the living — which is the
    // assumption failing, not the implementation. This is what makes the synchronous mode
    // load-bearing rather than incidental.
    let accused_anywhere = (0..20u64).any(|seed| {
        let mut s: Sim<PerfectFailureDetector> = Sim::new(
            Config::default()
                .seed(seed)
                .loss(0.6)
                .latency(Duration::from_millis(1), Duration::from_millis(30)),
            &ALL,
            detector,
        );
        s.run_for(timeout() * 6);
        ALL.iter().any(|n| !accused(&s, *n).is_empty())
    });

    assert!(
        accused_anywhere,
        "on a lossy network a correct process must eventually be accused — if not, the \
         synchronous mode is not what makes the accuracy tests pass"
    );
}
