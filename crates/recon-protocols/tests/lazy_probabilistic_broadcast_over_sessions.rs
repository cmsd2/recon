//! Lazy gossip over session links — both halves, the gossip and the recovery, over one session per
//! peer pair. The real-world set's workhorse, held to its second standard.
//!
//! What only this configuration can show: a session ending is a loss the recovery phase repairs,
//! and the repair costs exactly the requests the gaps called for.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::lazy_probabilistic_broadcast::{
    self as lpb, Cmd, Config, Ind, LazyProbabilisticBroadcast, Recovery,
};
use recon_protocols::probabilistic_broadcast as pb;
use recon_protocols::session_link::SessionLink;
use recon_protocols::stacks::LazyProbabilisticBroadcastOverSessions;
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

type Lpb = LazyProbabilisticBroadcastOverSessions<u32>;

/// Every peer, one round, everything stored, one round of requests: the deterministic
/// configuration, in which the only way to miss a message is for the session carrying it to end.
fn direct(window: usize) -> Config {
    Config {
        gossip: pb::Config { fanout: ALL.len() - 1, rounds: 1, window: ROOMY },
        store_probability: 1.0,
        request_rounds: 1,
        gap_timeout: Duration::from_secs(2),
        window,
    }
}

fn sim(seed: u64, config: Config) -> Sim<Lpb> {
    let mut s: Sim<Lpb> = Sim::new(
        SimConfig::default()
            .seed(seed)
            .sessions()
            .latency(Duration::from_millis(1), Duration::from_millis(20)),
        &ALL,
        move |me| {
            LazyProbabilisticBroadcast::with_links(
                me,
                ALL,
                SessionLink::new(),
                SessionLink::new(),
                config,
            )
        },
    );
    s.deliver_session_events();
    s
}

fn delivered_at(s: &Sim<Lpb>, node: NodeId) -> Vec<u32> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            Ind::Deliver { msg, .. } => Some(*msg),
            _ => None,
        })
        .collect()
}

fn requests(s: &Sim<Lpb>) -> Vec<(NodeId, NodeId, NodeId, u64)> {
    s.trace()
        .sends()
        .filter_map(|(from, to, m)| match m {
            lpb::Wire::Recovery(Recovery::Request { requester, origin, seq, .. }) => {
                (*requester == from).then_some((from, to, *origin, *seq))
            }
            _ => None,
        })
        .collect()
}

fn answers(s: &Sim<Lpb>) -> usize {
    s.trace()
        .sends()
        .filter(|(_, _, m)| matches!(m, lpb::Wire::Recovery(Recovery::Data(_))))
        .count()
}

fn requests_delivered(s: &Sim<Lpb>) -> usize {
    s.trace()
        .deliveries()
        .filter(|(_, _, m)| matches!(m, lpb::Wire::Recovery(Recovery::Request { .. })))
        .count()
}

fn endings_reported_at(s: &Sim<Lpb>, node: NodeId, peer: NodeId) -> (usize, usize) {
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

/// A run in which the session to B ended with A's first message in flight, and A's second message
/// then exposed the gap. The cut is drawn from the seed and may spare everything, so this searches.
fn gap_from_an_ending() -> (u64, Sim<Lpb>) {
    (0..60u64)
        .find_map(|seed| {
            let mut s = sim(seed, direct(ROOMY));
            s.command(A, Cmd::Broadcast(1));
            // A command is scheduled, not run: dispatch this instant so the sends are in flight —
            // by event, not by a duration shorter than the latency.
            s.step_now();
            s.break_session(A, B);
            s.run_for(Duration::from_millis(100));
            s.command(A, Cmd::Broadcast(2));
            s.run_for(Duration::from_millis(500));
            let b_asked = requests(&s).iter().any(|(from, ..)| *from == B);
            (s.trace().suffix_losses() > 0 && b_asked).then_some((seed, s))
        })
        .expect("no seed lost A's message to B in the ending, so there was no gap to repair")
}

// ------------------------------------------------- recovery bridges a session ending: task 5.1

#[test]
fn a_gap_opened_by_a_session_ending_is_repaired_and_delivery_stays_in_sequence() {
    let (seed, s) = gap_from_an_ending();

    assert!(s.trace().session_ends() >= 1, "seed {seed}: a session ended");
    assert_eq!(delivered_at(&s, B), vec![1, 2], "seed {seed}: B did not deliver both, in order");
    for n in ALL {
        assert_eq!(delivered_at(&s, n), vec![1, 2], "seed {seed}: {n}");
    }
}

// ------------------------------------------------- what the repair costs: task 5.2

#[test]
fn requests_are_the_fanout_times_the_gaps_and_answers_never_exceed_requests_that_could_be_met() {
    let (seed, s) = gap_from_an_ending();
    let reqs = requests(&s);

    // One round of requests, so none is relayed: every request message is one of the `k` a
    // detected gap sends. Gaps are the distinct (requester, origin, seq) triples.
    let gaps: std::collections::BTreeSet<(NodeId, NodeId, u64)> =
        reqs.iter().map(|(from, _, origin, seq)| (*from, *origin, *seq)).collect();
    assert_eq!(
        reqs.len(),
        gaps.len() * (ALL.len() - 1),
        "seed {seed}: request messages are not fanout × gaps detected"
    );
    assert_eq!(gaps.len(), 1, "seed {seed}: exactly one gap — B missing (A, 1) — was detected");

    // Every process stores everything it receives, and a process that received the request holds
    // the message unless the ending took its copy too. So answers are at most the requests that
    // arrived, and at least one arrived somewhere that could answer.
    assert!(answers(&s) >= 1, "seed {seed}: nobody answered");
    assert!(
        answers(&s) <= requests_delivered(&s),
        "seed {seed}: {} answers to {} requests received",
        answers(&s),
        requests_delivered(&s)
    );
}

// ------------------------------------------------- quiet means silent, and bounded: task 5.3

#[test]
fn a_steady_workload_costs_the_same_every_window_and_quiet_means_silent() {
    let window = 4;
    let mut s = sim(3, direct(window));
    let mut per_window = Vec::new();
    for m in 0..8u32 {
        let before = s.trace().send_count();
        s.command(A, Cmd::Broadcast(m));
        s.run_for(Duration::from_millis(300));
        per_window.push(s.trace().send_count() - before);
    }
    // Every peer, one round, no loss: one broadcast is exactly `N − 1` sends, and no gap ever opens
    // so there is no recovery traffic at all.
    assert!(
        per_window.iter().all(|c| *c == ALL.len() - 1),
        "the cost of one broadcast varied, or recovery traffic appeared: {per_window:?}"
    );
    assert!(requests(&s).is_empty(), "requests were sent with nothing missing");

    let quiet = s.trace().send_count();
    s.run_for(Duration::from_secs(5));
    assert_eq!(s.trace().send_count(), quiet, "an idle gossip sent something");

    for n in ALL {
        assert_eq!(s.at(n).pending_count(), 0, "{n} is holding something ahead of a gap");
        assert!(s.at(n).stored_count() <= window, "{n} stores {}", s.at(n).stored_count());
    }
}

// ------------------------------------------------- one session, two links, one report: task 5.4

#[test]
fn both_links_see_the_session_and_its_ending_is_reported_once() {
    let mut s = sim(4, direct(ROOMY));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(200));
    let before = s.at(A).recovery_link().epoch(B).expect("A has a session with B");
    assert_eq!(s.at(A).gossip_link().epoch(B), Some(before), "the two links disagree on the epoch");

    s.break_session(A, B);
    s.run_for(Duration::from_millis(500));

    let after = s.at(A).recovery_link().epoch(B).expect("the session came back");
    assert!(after > before, "the re-established session has a higher epoch");
    assert_eq!(s.at(A).gossip_link().epoch(B), Some(after), "both links moved to the new epoch");

    // Two links, one session, one report: the layer above hears each boundary once.
    let (ended, established) = endings_reported_at(&s, A, B);
    assert_eq!(ended, 1, "A was told the session with B ended {ended} times");
    assert_eq!(established, 2, "A was told of {established} establishments — first, and again");
}

// ------------------------------------------------- identity survives the originator: task 7.2

#[test]
fn a_restarted_originators_messages_are_delivered_in_sequence_over_sessions() {
    // A restart ends every session the originator held and its sequence numbers start again. Both
    // are exactly what a deployment does. Every receiver must deliver all six, in order.
    let mut s = sim(5, direct(ROOMY));
    for m in [1, 2, 3] {
        s.command(A, Cmd::Broadcast(m));
    }
    s.run_for(Duration::from_millis(300));
    s.crash(A);
    s.restart(A);
    // A send in the instant a session ended goes nowhere — a transport does not reopen a connection
    // in the same instant it closed one. Wait for the sessions to be back, by that event.
    while ALL.iter().any(|n| *n != A && !s.has_session(A, *n)) {
        s.run_for(Duration::from_millis(10));
    }
    for m in [4, 5, 6] {
        s.command(A, Cmd::Broadcast(m));
    }
    s.run_for(Duration::from_millis(500));

    for n in ALL {
        assert_eq!(delivered_at(&s, n), vec![1, 2, 3, 4, 5, 6], "{n}");
    }
    assert!(s.trace().session_ends() >= ALL.len() - 1, "the crash ended A's sessions");
}
