//! Eager gossip over a session link — the real-world set's form of Algorithm 3.9, held to the
//! set's second standard: what it costs is asserted, not assumed.
//!
//! Within a session nothing is lost, so this is the one configuration in which a broadcast's cost
//! is an exact number rather than a distribution, and the suite says what that number is.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::probabilistic_broadcast::{Cmd, Config, Ind, ProbabilisticBroadcast};
use recon_protocols::session_link::SessionLink;
use recon_protocols::stacks::ProbabilisticBroadcastOverSessions;
use recon_sim::{Config as SimConfig, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const E: NodeId = NodeId::new(5);
const F: NodeId = NodeId::new(6);
const G: NodeId = NodeId::new(7);
const H: NodeId = NodeId::new(8);
const ALL: [NodeId; 8] = [A, B, C, D, E, F, G, H];

const ROOMY: usize = 1_000;

type Pb = ProbabilisticBroadcastOverSessions<u32>;

fn config(fanout: usize, rounds: u32) -> Config {
    Config { fanout, rounds, window: ROOMY }
}

fn sim(seed: u64, config: Config) -> Sim<Pb> {
    let mut s: Sim<Pb> = Sim::new(
        SimConfig::default()
            .seed(seed)
            .sessions()
            .latency(Duration::from_millis(1), Duration::from_millis(20)),
        &ALL,
        move |me| ProbabilisticBroadcast::with_link(me, ALL, SessionLink::new(), config),
    );
    s.deliver_session_events();
    s
}

fn delivered_by(s: &Sim<Pb>, msg: u32) -> Vec<NodeId> {
    ALL.iter()
        .copied()
        .filter(|n| {
            s.trace()
                .indications_at(*n)
                .any(|i| matches!(i, Ind::Deliver { msg: m, .. } if *m == msg))
        })
        .collect()
}

fn boundaries_at(s: &Sim<Pb>, node: NodeId, peer: NodeId) -> (usize, usize) {
    let inds: Vec<_> = s.trace().indications_at(node).collect();
    (
        inds.iter()
            .filter(|i| matches!(i, Ind::SessionEnded { peer: p, .. } if *p == peer))
            .count(),
        inds.iter()
            .filter(|i| matches!(i, Ind::SessionEstablished { peer: p, .. } if *p == peer))
            .count(),
    )
}

/// `Σ_{i=1..R} kⁱ` — what Algorithm 3.9 sends for one broadcast when nothing is lost.
fn closed_form(fanout: usize, rounds: u32) -> usize {
    (1..=rounds).map(|i| fanout.pow(i)).sum()
}

// ------------------------------------------------- coverage stays probabilistic: task 4.1

#[test]
fn a_generous_fanout_usually_reaches_everyone_and_a_starved_one_never_does() {
    // Within a session nothing is lost, but `picktargets` is still random, so `PB1` is still
    // probabilistic here: the only thing that changed is that loss is no longer a reason.
    let generous = (0..40u64)
        .filter(|seed| {
            let mut s = sim(*seed, config(3, 3));
            s.command(A, Cmd::Broadcast(1));
            s.run_for(Duration::from_millis(300));
            delivered_by(&s, 1).len() == ALL.len()
        })
        .count();
    assert!(generous >= 30, "fanout 3 over 3 rounds reached everyone on only {generous}/40 runs");

    let starved = (0..40u64)
        .filter(|seed| {
            let mut s = sim(*seed, config(1, 1));
            s.command(A, Cmd::Broadcast(1));
            s.run_for(Duration::from_millis(300));
            delivered_by(&s, 1).len() == ALL.len()
        })
        .count();
    assert_eq!(starved, 0, "one peer, one round, and yet {starved} runs reached all eight");
}

// ------------------------------------------------- what a broadcast costs: task 4.2

#[test]
fn a_broadcast_sends_exactly_the_closed_form_and_every_send_arrives() {
    for (fanout, rounds) in [(2, 3), (3, 2), (1, 4), (4, 1)] {
        for seed in 0..5u64 {
            let mut s = sim(seed, config(fanout, rounds));
            s.command(A, Cmd::Broadcast(1));
            s.run_for(Duration::from_millis(500));

            let expected = closed_form(fanout, rounds);
            assert_eq!(
                s.trace().send_count(),
                expected,
                "k={fanout} R={rounds} seed {seed}: the algorithm specifies {expected} sends"
            );
            assert_eq!(
                s.trace().delivery_count(),
                expected,
                "k={fanout} R={rounds} seed {seed}: within sessions nothing is lost"
            );
        }
    }
}

#[test]
fn sends_are_the_fanout_times_one_more_than_the_receipts_still_to_relay() {
    // The identity that holds over *any* run, lost messages or not: the originator sends `k`, and
    // every receipt with rounds still to live sends `k` more. A stray retransmission or a doubled
    // relay would break equality where a ceiling would hide it.
    for seed in 0..10u64 {
        let mut s = sim(seed, config(3, 3));
        for (i, n) in [A, D, G].iter().enumerate() {
            s.command(*n, Cmd::Broadcast(i as u32));
        }
        s.run_for(Duration::from_millis(500));

        let relaying_receipts = s.trace().deliveries().filter(|(_, _, g)| g.ttl > 1).count();
        let broadcasts = 3;
        assert_eq!(
            s.trace().send_count(),
            3 * (broadcasts + relaying_receipts),
            "seed {seed}: sends are not k × (broadcasts + receipts with ttl > 1)"
        );
    }
}

// ------------------------------------------------- an idle gossip is silent: task 4.3

#[test]
fn an_idle_gossip_sends_nothing() {
    // Nothing beneath a session link retransmits, and this layer keeps no timer. Once every
    // broadcast has finished relaying, the wire is silent — not flat, silent.
    let mut s = sim(1, config(3, 3));
    for n in ALL {
        s.command(n, Cmd::Broadcast(n.0 as u32));
    }
    s.run_for(Duration::from_millis(500));
    let quiet = s.trace().send_count();
    assert!(quiet > 0, "nothing was sent at all, so silence would mean nothing");

    s.run_for(Duration::from_secs(5));
    assert_eq!(s.trace().send_count(), quiet, "an idle gossip sent something");
}

// ------------------------------------------------- a session ending: task 4.4

#[test]
fn a_session_ending_is_propagated_and_costs_only_what_was_in_flight() {
    // One round and the whole peer set, so every send is direct and in flight together; then the
    // session to B ends before they land. The cut is drawn from the seed and may spare everything,
    // so search for a run in which it did not.
    let run = (0..60u64).find_map(|seed| {
        let mut s = sim(seed, config(ALL.len() - 1, 1));
        s.command(A, Cmd::Broadcast(1));
        // A command is scheduled, not run: dispatch this instant so the sends are in flight —
        // by event, not by a duration shorter than the latency.
        s.step_now();
        s.break_session(A, B);
        s.run_for(Duration::from_millis(500));
        (s.trace().suffix_losses() > 0).then_some((seed, s))
    });
    let (seed, s) = run.expect("no seed lost anything to the ending, so nothing was tested");

    // Reported once at each end, as this layer's own indication.
    assert_eq!(boundaries_at(&s, A, B).0, 1, "seed {seed}: A was told once");
    assert_eq!(boundaries_at(&s, B, A).0, 1, "seed {seed}: B was told once");

    // The suffix is the whole cost: every other send arrived, and nothing else was dropped.
    assert_eq!(s.trace().drops(), 0, "seed {seed}: something was dropped for another reason");
    assert_eq!(
        s.trace().delivery_count() + s.trace().suffix_losses(),
        s.trace().send_count(),
        "seed {seed}: sends are not accounted for by deliveries plus the lost suffix"
    );
    assert!(!delivered_by(&s, 1).contains(&B), "seed {seed}: B delivered what the ending lost");
    assert_eq!(delivered_by(&s, 1).len(), ALL.len() - 1, "seed {seed}: everyone else delivered");
}

// ------------------------------------------------- flat and bounded, over sessions: task 4.5

#[test]
fn a_steady_workload_costs_the_same_every_window_and_the_window_bounds_the_state() {
    let window = 4;
    let mut s = sim(2, Config { fanout: 2, rounds: 3, window });
    let mut per_window = Vec::new();
    for m in 0..8u32 {
        let before = s.trace().send_count();
        s.command(A, Cmd::Broadcast(m));
        s.run_for(Duration::from_millis(300));
        per_window.push(s.trace().send_count() - before);
    }
    assert!(
        per_window.iter().all(|c| *c == closed_form(2, 3)),
        "the cost of one broadcast varied across windows: {per_window:?}"
    );
    for n in ALL {
        assert!(
            s.at(n).remembered() <= window,
            "{n} remembers {} identifiers with a window of {window}",
            s.at(n).remembered()
        );
    }
}
