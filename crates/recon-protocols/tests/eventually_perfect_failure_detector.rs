//! ◇P against Module 2.8: completeness, a suspicion that is taken back, and a timeout that moves in
//! both directions inside a stated bound.
//!
//! The two departures from Algorithm 2.7 — the decrease and the cap — are what most of this suite
//! is about, because each trades a piece of the book's guarantee for something a deployment needs,
//! and a trade nobody measures is a trade nobody made.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::eventually_perfect_failure_detector::{
    Config, EventuallyPerfectFailureDetector, Ind,
};
use recon_sim::{Config as SimConfig, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

/// The network's promise where there is one. Everything else is derived from it.
const BOUND: Duration = Duration::from_millis(20);

fn period() -> Duration {
    BOUND * 2
}
fn initial() -> Duration {
    period() * 3
}
fn roomy() -> Config {
    Config::new(period(), initial(), initial() * 20)
}

type Dp = EventuallyPerfectFailureDetector;

fn dp(me: NodeId, config: Config) -> Dp {
    EventuallyPerfectFailureDetector::new(me, ALL, config)
}

/// A run in which the timing assumption holds.
fn sync_sim(seed: u64, config: Config) -> Sim<Dp> {
    Sim::new(SimConfig::default().seed(seed).synchronous(BOUND), &ALL, move |me| dp(me, config))
}

/// A run in which it does not: latency well past the initial delay, so correct processes are
/// suspected and then heard from.
fn noisy_sim(seed: u64, config: Config, max: Duration) -> Sim<Dp> {
    Sim::new(
        SimConfig::default().seed(seed).latency(Duration::from_millis(1), max),
        &ALL,
        move |me| dp(me, config),
    )
}

fn events_at(s: &Sim<Dp>, node: NodeId) -> Vec<Ind> {
    s.trace().indications_at(node).copied().collect()
}

fn suspicions_of(s: &Sim<Dp>, node: NodeId, of: NodeId) -> usize {
    events_at(s, node).iter().filter(|i| matches!(i, Ind::Suspect { node: n } if *n == of)).count()
}

fn restores_of(s: &Sim<Dp>, node: NodeId, of: NodeId) -> usize {
    events_at(s, node).iter().filter(|i| matches!(i, Ind::Restore { node: n } if *n == of)).count()
}

// ------------------------------------------------- completeness: task 2.2

#[test]
fn a_crash_is_suspected_everywhere_and_never_taken_back() {
    let mut s = sync_sim(1, roomy());
    s.run_for(initial() * 2);
    s.crash(D);
    s.run_for(initial() * 6);

    for n in [A, B, C] {
        assert!(s.at(n).suspects(D), "{n} does not suspect the crashed process");
        assert_eq!(suspicions_of(&s, n, D), 1, "{n} suspected D once");
        assert_eq!(restores_of(&s, n, D), 0, "{n} took it back while D was still down");
    }
}

#[test]
fn nobody_is_suspected_while_the_assumption_holds() {
    // Accuracy, which under `synchronous(BOUND)` with delay > period + BOUND is unconditional.
    let mut s = sync_sim(2, roomy());
    s.run_for(initial() * 8);
    for n in ALL {
        assert!(events_at(&s, n).is_empty(), "{n} said something about a correct process");
    }
}

// ------------------------------------------------- the indication P does not have: task 2.3

#[test]
fn a_correct_process_suspected_under_a_withdrawn_assumption_is_restored() {
    // The whole difference from `P`. Latency far beyond the initial delay makes correct processes
    // look silent; when their heartbeats land, the suspicion is withdrawn rather than standing for
    // ever.
    let found = (0..40u64).find_map(|seed| {
        let mut s = noisy_sim(seed, roomy(), initial() * 3);
        s.run_for(initial() * 10);
        let restored = ALL
            .iter()
            .flat_map(|n| ALL.iter().map(move |o| (*n, *o)))
            .find(|(n, o)| n != o && restores_of(&s, *n, *o) > 0);
        restored.map(|(n, o)| (seed, n, o, s))
    });
    let (seed, node, of, s) = found.expect("no seed produced a withdrawn suspicion");

    assert!(
        suspicions_of(&s, node, of) > 0,
        "seed {seed}: {node} withdrew a suspicion it never raised"
    );
    assert!(!s.at(of).suspects(of), "seed {seed}: {of} was correct throughout");
}

#[test]
fn a_recovered_process_is_restored() {
    // The case that matters for the layers above: under `P` this restoration is impossible, so a
    // recovered process is suspected for the rest of the run and can never lead again.
    let mut s = sync_sim(3, roomy());
    s.run_for(initial() * 2);
    s.crash(D);
    s.run_for(initial() * 4);
    for n in [A, B, C] {
        assert!(s.at(n).suspects(D), "D is suspected before it comes back");
    }

    s.restart(D);
    s.run_for(initial() * 6);

    for n in [A, B, C] {
        assert!(!s.at(n).suspects(D), "{n} still suspects a process that came back");
        assert_eq!(restores_of(&s, n, D), 1, "{n} withdrew the suspicion, once");
    }
}

#[test]
fn nothing_is_reported_when_the_suspected_set_does_not_change() {
    // A detector that re-announced an unchanged answer would cost the layer above an epoch per
    // round. The count is taken over a long quiet tail rather than asserted from the mechanism.
    let mut s = sync_sim(4, roomy());
    s.run_for(initial() * 2);
    s.crash(D);
    s.run_for(initial() * 4);
    let after_settling: Vec<usize> = [A, B, C].iter().map(|n| events_at(&s, *n).len()).collect();

    s.run_for(initial() * 12);
    let later: Vec<usize> = [A, B, C].iter().map(|n| events_at(&s, *n).len()).collect();
    assert_eq!(after_settling, later, "something was re-announced with nothing changed");
}

// ------------------------------------------------- the delay moves: tasks 2.5 and 2.6

#[test]
fn a_false_suspicion_raises_the_delay_and_sustained_accuracy_lowers_it() {
    // Up on being caught wrong, down after `quiet_rounds` clean rounds — the two departures, in
    // one run. Latency past the initial delay first, then a healed network.
    let mut cfg = roomy();
    cfg.quiet_rounds = 2;
    let raised = (0..40u64).find_map(|seed| {
        let mut s = noisy_sim(seed, cfg, initial() * 3);
        s.run_for(initial() * 8);
        (s.at(A).delay() > initial()).then_some((seed, s))
    });
    let (seed, mut s) =
        raised.expect("no seed made the detector wrong, so nothing raised the delay");
    let high = s.at(A).delay();

    // The network was bad; the delay went up. Nothing brings it down but quiet, and the run has
    // been anything but — so the assertion is about the mechanism, not about this run's luck.
    assert!(high > initial(), "seed {seed}: the delay rose above where it started");
    assert!(high <= cfg.max_delay, "seed {seed}: and not past the cap");

    // Now let it be quiet. `run_for` here is "long enough for many rounds", not a duration the
    // result depends on being short.
    s.heal();
    s.run_for(initial() * 40);
    assert!(
        s.at(A).delay() < high,
        "seed {seed}: the delay stayed at {high:?} through a long quiet period — the ratchet"
    );
}

#[test]
fn the_delay_never_falls_below_the_floor() {
    let mut cfg = roomy();
    cfg.quiet_rounds = 1;
    let mut s = sync_sim(5, cfg);
    s.run_for(initial() * 40);
    assert_eq!(s.at(A).delay(), cfg.min_delay, "a long quiet run rests on the floor, not below");
}

#[test]
fn the_delay_follows_the_network_up_and_then_down_without_thrashing() {
    // Rising faster than it falls is what damps the oscillation a symmetric rule would produce.
    // Measured as: the delay is higher while the network is bad than after it recovers, and the
    // recovery takes more rounds than the rise did.
    let mut cfg = roomy();
    cfg.quiet_rounds = 4;
    let run = (0..40u64).find_map(|seed| {
        let mut s = noisy_sim(seed, cfg, initial() * 3);
        s.run_for(initial() * 6);
        let bad = s.at(A).delay();
        if bad <= initial() {
            return None;
        }
        s.heal();
        s.run_for(initial() * 60);
        Some((seed, bad, s.at(A).delay()))
    });
    let (seed, bad, settled) = run.expect("no seed drove the delay up");
    assert!(settled < bad, "seed {seed}: {settled:?} after recovery is not below {bad:?}");
    assert!(settled >= cfg.min_delay, "seed {seed}: and not below the floor");
}

// ------------------------------------------------- the cap: tasks 2.7 and 2.8

#[test]
fn the_delay_reaches_the_cap_and_never_passes_it() {
    // A network far worse than the cap: without one the delay would grow without limit. Sampled
    // across the run rather than read at the end, because the delay does not *stay* at the cap —
    // once the suspicions clear, the decrease pulls it back down, which is the design and not a
    // failure of it. What the cap promises is a ceiling, not a resting place.
    let cfg = Config { max_delay: initial() * 2, ..roomy() };
    let mut s = noisy_sim(6, cfg, initial() * 12);
    let mut seen = Vec::new();
    for _ in 0..30 {
        s.run_for(initial());
        for n in ALL {
            seen.push((n, s.at(n).delay()));
        }
    }
    assert!(
        seen.iter().all(|(_, d)| *d <= cfg.max_delay),
        "the delay passed the cap: {:?}",
        seen.iter().filter(|(_, d)| *d > cfg.max_delay).collect::<Vec<_>>()
    );
    assert!(
        seen.iter().any(|(_, d)| *d == cfg.max_delay),
        "the delay never reached the cap, so the ceiling was never tested: {:?}",
        seen.iter().map(|(_, d)| d.as_millis()).collect::<Vec<_>>()
    );
    assert!(
        seen.iter().any(|(_, d)| *d > cfg.min_delay),
        "and it moved off the floor, so something drove it"
    );
}

#[test]
fn accuracy_is_lost_when_the_network_settles_above_the_cap() {
    // `◇P2` is conditional on `Δ ≤ max_delay`, and this is the condition failing — asserted rather
    // than described. The detector keeps suspecting correct processes, for ever.
    let cfg = Config { max_delay: initial() * 2, ..roomy() };
    let mut s = noisy_sim(7, cfg, initial() * 12);
    s.run_for(initial() * 30);
    let before: usize = ALL.iter().map(|n| events_at(&s, *n).len()).sum();
    s.run_for(initial() * 30);
    let after: usize = ALL.iter().map(|n| events_at(&s, *n).len()).sum();

    assert!(after > before, "the detector settled, so the condition did not actually fail");
    assert!(
        ALL.iter().all(|n| !s.at(*n).suspects(*n)),
        "nothing crashed: every one of these suspicions is of a correct process"
    );
}

#[test]
fn false_suspicions_rise_as_the_cap_falls_below_the_true_delay() {
    // The cap is a number someone picked unless it is measured, and this is the measurement: sweep
    // it against a fixed latency and count what it costs. The simulator is this project's standard
    // of evidence, so the trade is a curve rather than a judgement.
    let latency = initial() * 4;
    let cost = |cap: Duration| -> usize {
        (0..12u64)
            .map(|seed| {
                let cfg = Config { max_delay: cap, ..roomy() };
                let mut s = noisy_sim(seed, cfg, latency);
                s.run_for(initial() * 20);
                ALL.iter().map(|n| events_at(&s, *n).len()).sum::<usize>()
            })
            .sum()
    };

    let tight = cost(initial());
    let generous = cost(latency * 3);
    assert!(
        generous < tight,
        "a cap above the true delay ({generous}) cost no less than one below it ({tight}), so the \
         cap is not the thing being measured"
    );
    assert!(tight > 0, "the tight cap produced no suspicions at all, so nothing was traded");
}

// ------------------------------------------------- bounded: task 2.9

#[test]
fn state_is_bounded_by_membership_and_the_send_rate_does_not_grow() {
    let mut s = sync_sim(8, roomy());
    s.run_for(initial() * 4);
    let mut counts = Vec::new();
    let mut prev = s.trace().send_count();
    for _ in 0..4 {
        s.run_for(initial() * 4);
        let now = s.trace().send_count();
        counts.push(now - prev);
        prev = now;
    }
    let first = counts[0];
    assert!(first > 0, "nothing was sent");
    assert!(
        *counts.last().unwrap() <= first + first / 10,
        "the send rate grew with time: {counts:?}"
    );
    for n in ALL {
        assert!(s.at(n).suspected().count() <= ALL.len(), "{n} suspects more than the membership");
    }
}

#[test]
fn a_bad_network_does_not_lower_the_delay() {
    // The rule that had to be corrected. Easing off after rounds with no suspicion *withdrawn*
    // lowers the delay exactly when the detector is being consistently wrong, because a network
    // that bad produces no withdrawals at all. The condition is "nothing suspected", so a round in
    // which somebody is suspected holds the delay where it is.
    let mut cfg = roomy();
    cfg.quiet_rounds = 1; // as eager to decrease as the configuration allows
    let mut s = sync_sim(9, cfg);
    s.run_for(initial() * 8);
    assert_eq!(s.at(A).delay(), cfg.min_delay, "a quiet run rests on the floor");

    // A crash: D is suspected from now on, and nothing is ever withdrawn.
    s.crash(D);
    s.run_for(initial() * 4);
    assert!(s.at(A).suspects(D));
    let frozen = s.at(A).delay();

    s.run_for(initial() * 20);
    assert_eq!(
        s.at(A).delay(),
        frozen,
        "the delay moved while a suspicion stood — under the withdrawn-based rule it would have \
         been eased off every round"
    );
}
