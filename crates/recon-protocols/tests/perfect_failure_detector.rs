//! The perfect failure detector against Module 2.6: strong completeness and strong accuracy —
//! and the loss of accuracy when the timing assumption it rests on is withdrawn.

use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, MemStore, NodeId, Protocol, Time, TimerId, step_with};
use recon_protocols::perfect_failure_detector::{Heartbeat, Ind, PerfectFailureDetector};
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

/// The handle the protocol just registered, read out of what it emitted rather than assumed.
fn armed<M, I>(fx: &[Effect<M, I>]) -> TimerId {
    fx.iter()
        .find_map(|e| match e {
            Effect::SetTimer { id, .. } => Some(*id),
            _ => None,
        })
        .expect("a timer was armed")
}

/// A fresh store per call: this protocol writes nothing durably.
fn store() -> MemStore<core::convert::Infallible, core::convert::Infallible> {
    MemStore::default()
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
    let mut ids = 0;
    let fx = step_with(&mut p, Event::Init, Time::ZERO, &mut rng(), &mut store(), &mut ids);
    assert_eq!(
        fx,
        vec![
            Effect::Send { to: B, msg: Heartbeat },
            Effect::Send { to: C, msg: Heartbeat },
            Effect::Send { to: D, msg: Heartbeat },
            Effect::SetTimer { after: period(), id: TimerId(0) },
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
    let mut ids = 0;
    step_with(&mut p, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let again = step_with(&mut p, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    assert_eq!(again, vec![]);
}

#[test]
fn a_heartbeat_before_the_tick_prevents_accusation() {
    let mut p = detector(A);
    let mut r = rng();
    let mut ids = 0;
    let init = step_with(&mut p, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let tick = armed(&init);
    for peer in [B, C, D] {
        let ev = Event::Msg { from: peer, msg: Heartbeat };
        step_with(&mut p, ev, Time::from_millis(5), &mut r, &mut store(), &mut ids);
    }
    let at = Time::from_offset(period());
    let fx = step_with(&mut p, Event::Timer(tick), at, &mut r, &mut store(), &mut ids);
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::Indicate(_))),
        "everyone was heard from, so nobody is accused"
    );
}

#[test]
fn silence_beyond_the_timeout_accuses_exactly_once() {
    let mut p = detector(A);
    let mut r = rng();
    let mut ids = 0;
    let init = step_with(&mut p, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let tick = armed(&init);

    let past = Time::from_offset(timeout() * 2);
    let ev = Event::Msg { from: B, msg: Heartbeat };
    step_with(&mut p, ev, past, &mut r, &mut store(), &mut ids);

    let first = step_with(&mut p, Event::Timer(tick), past, &mut r, &mut store(), &mut ids);
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
    let ev = Event::Msg { from: B, msg: Heartbeat };
    step_with(&mut p, ev, later, &mut r, &mut store(), &mut ids);
    // The tick re-armed itself when it fired, so the one to fire now is the newer handle.
    let tick = armed(&first);
    let second = step_with(&mut p, Event::Timer(tick), later, &mut r, &mut store(), &mut ids);
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
    let mut ids = 0;
    let fx = step_with(&mut p, Event::Init, Time::ZERO, &mut rng(), &mut store(), &mut ids);
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
    s.resume(D);
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

#[test]
fn a_stalled_process_accuses_its_peers_when_it_comes_back() {
    // The other side of a stall, which the two tests above do not look at: they check what A, B
    // and C make of a silent D, never what D makes of them.
    //
    // A suspension holds what arrived, so nothing is lost — but it does not hold the clock. D's
    // deferred tick fires having measured, correctly, that it heard nothing for the whole stall,
    // and accuses every one of them. PFD2 says a detected process has crashed, and none had; the
    // synchrony assumption is what failed, and a process descheduled past its own timeout is
    // exactly that failure, seen from the inside. Pinned rather than fixed: a detector that
    // discounts its own stall is a departure from the page and wants a proposal.
    let mut s = sync_sim(15);
    s.run_for(Duration::from_millis(100));
    s.suspend(D);
    s.run_for(timeout() * 3);
    s.resume(D);
    s.run_for(period());

    let mut by_d = accused(&s, D);
    by_d.sort();
    assert_eq!(by_d, vec![A, B, C], "D accuses everyone it could not hear while it was away");
    assert_eq!(
        s.trace().drops(),
        0,
        "and not because anything was lost: the heartbeats were held, not dropped"
    );
}

#[test]
fn a_stall_shorter_than_the_timeout_costs_the_stalled_process_nothing() {
    // The boundary from the inside, and what makes the test above a statement about the timeout
    // rather than about suspension.
    let mut s = sync_sim(16);
    s.run_for(Duration::from_millis(100));
    s.suspend(D);
    s.run_for(timeout() - period() - BOUND * 2);
    s.resume(D);
    s.run_for(timeout() * 3);

    assert!(accused(&s, D).is_empty(), "D accused nobody over a brief stall");
    for n in [A, B, C] {
        assert!(accused(&s, n).is_empty(), "{n} accused nobody either");
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

// ------------------------------------- What the handle makes possible

#[test]
fn a_superseded_expiry_accuses_nobody() {
    // The detector's whole judgement is "has this peer been heard from since the last tick", so
    // acting on a stale expiry would evaluate that question at a moment it did not choose — and
    // for this protocol the consequence of getting it wrong is accusing a living process.
    let mut p = detector(A);
    let mut r = rng();
    let mut ids = 0;

    let init = step_with(&mut p, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let stale = armed(&init);

    // The tick fires and re-arms, so the detector is now waiting on a different handle.
    let at = Time::from_offset(period());
    let live = armed(&step_with(&mut p, Event::Timer(stale), at, &mut r, &mut store(), &mut ids));
    assert_ne!(live, stale, "re-arming registers a new timer, not the same one again");

    // Long enough that a tick the detector *is* waiting on would accuse every silent peer.
    let late = Time::from_offset(timeout() * 3);
    let fx = step_with(&mut p, Event::Timer(stale), late, &mut r, &mut store(), &mut ids);
    assert_eq!(fx, vec![], "the superseded expiry accuses nobody and beats to nobody");
    assert!(![B, C, D].iter().any(|n| p.has_detected(*n)), "and nobody has been detected");

    // Non-vacuity: the live handle at the same instant does accuse, so the assertion above is
    // not satisfied by a detector that has stopped judging altogether.
    let fx = step_with(&mut p, Event::Timer(live), late, &mut r, &mut store(), &mut ids);
    assert!(
        fx.iter().any(|e| matches!(e, Effect::Indicate(Ind::Crash { .. }))),
        "the live expiry at the same instant does accuse the silent"
    );
}
