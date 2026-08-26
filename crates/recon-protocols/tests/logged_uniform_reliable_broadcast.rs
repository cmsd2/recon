//! Logged uniform reliable broadcast against Module 3.6 — the second consumer of stable storage,
//! and the one that shows `ack` being rebuilt rather than written down.
//!
//! Five processes, for the reason the majority-ack suite records: `N > 2f` leaves room for two
//! failures, and an odd membership hides an off-by-one that an even one exposes.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::logged_uniform_reliable_broadcast::{
    Cmd, Ind, LoggedUniformReliableBroadcast,
};
use recon_protocols::uniform_reliable_broadcast::BroadcastId;
use recon_sim::{Config, Sim, TraceEvent};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const E: NodeId = NodeId::new(5);
const ALL: [NodeId; 5] = [A, B, C, D, E];

fn interval() -> Duration {
    Duration::from_millis(10)
}

type Urb = LoggedUniformReliableBroadcast<u32>;

fn sim(seed: u64) -> Sim<Urb> {
    Sim::new(Config::default().seed(seed), &ALL, |me| {
        LoggedUniformReliableBroadcast::new(me, ALL, interval())
    })
}

/// What `node` has log-delivered, read from its durable log.
fn log_of(s: &Sim<Urb>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.protocol(node).unwrap().log().delivered().map(|(id, p)| (id.origin, *p)).collect()
}

fn settle(s: &mut Sim<Urb>) {
    s.run_for(Duration::from_millis(800));
}

// ------------------------------------------------------- the guarantees

#[test]
fn a_run_with_no_faults_log_delivers_everywhere() {
    let mut s = sim(1);
    s.command(A, Cmd::Broadcast(7));
    settle(&mut s);
    for n in ALL {
        assert_eq!(log_of(&s, n), vec![(A, 7)], "{n}");
    }
}

#[test]
fn a_minority_crashing_and_recovering_does_not_prevent_it() {
    // Two of five crash and come back: N > 2f with N = 5, f = 2.
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(30));
        s.crash(D);
        s.crash(E);
        s.run_for(Duration::from_millis(100));
        s.restart(D);
        s.restart(E);
        settle(&mut s);
        for n in ALL {
            assert_eq!(log_of(&s, n), vec![(A, 1)], "seed {seed}: {n}");
        }
    }
}

#[test]
fn a_process_that_log_delivers_and_then_crashes_for_ever_does_not_split_the_rest() {
    // Loss, so that log-deliveries are staggered: on a clean network all five reach a majority in
    // the same instant and there is no lone first deliverer to crash.
    let found = (0..60u64).find_map(|seed| {
        let mut s: Sim<Urb> = Sim::new(Config::default().seed(seed).loss(0.4), &ALL, |me| {
            LoggedUniformReliableBroadcast::new(me, ALL, interval())
        });
        s.command(A, Cmd::Broadcast(1));
        let mut steps = 0;
        while steps < 300 {
            s.run_for(Duration::from_millis(1));
            steps += 1;
            let who: Vec<NodeId> =
                ALL.iter().copied().filter(|n| !log_of(&s, *n).is_empty()).collect();
            if who.len() == 1 {
                s.crash(who[0]);
                s.run_for(Duration::from_secs(4));
                return Some((seed, who[0], s));
            }
            if who.len() > 1 {
                return None;
            }
        }
        None
    });

    let (seed, first, s) = found.expect("no seed produced a lone first deliverer");
    for n in ALL.iter().filter(|n| **n != first) {
        assert_eq!(log_of(&s, *n), vec![(A, 1)], "seed {seed}: {n} follows {first}");
    }
}

#[test]
fn a_process_that_log_delivers_crashes_and_recovers_does_not_deliver_twice() {
    let mut s = sim(2);
    s.command(A, Cmd::Broadcast(4));
    settle(&mut s);
    assert_eq!(log_of(&s, C), vec![(A, 4)]);

    s.crash(C);
    s.restart(C);
    settle(&mut s); // the stubborn broadcast is still delivering it, over and over

    assert_eq!(log_of(&s, C), vec![(A, 4)], "still once, an incarnation later");
}

#[test]
fn nothing_is_log_delivered_that_was_not_broadcast() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.command(A, Cmd::Broadcast(11));
        s.command(C, Cmd::Broadcast(22));
        settle(&mut s);
        for n in ALL {
            let mut got = log_of(&s, n);
            got.sort();
            assert_eq!(got, vec![(A, 11), (C, 22)], "seed {seed}: {n}");
        }
    }
}

// ------------------------------------------- what is durable, and what is not

#[test]
fn acknowledgements_are_never_written_down() {
    // The book's point: `ack` is not logged because it is rebuilt on recovery. What is written is
    // `pending` and `delivered`, and a recovered process holds no acknowledgements at all.
    let mut s = sim(3);
    s.command(A, Cmd::Broadcast(5));
    settle(&mut s);

    let id = BroadcastId { origin: A, seq: 1 };
    assert!(
        s.protocol(C).unwrap().acknowledged_by(id).count() >= s.protocol(C).unwrap().majority(),
        "C had accumulated acknowledgements before the crash"
    );
    assert!(s.trace().writes() > 0, "and had written something — but not those");

    s.crash(C);
    s.restart(C);
    let recovered = s.protocol(C).unwrap();
    assert_eq!(
        recovered.acknowledged_by(id).count(),
        0,
        "on recovering it holds no acknowledgements: they were not durable"
    );
    assert_eq!(recovered.log().delivered_count(), 1, "but what it log-delivered survived");
}

#[test]
fn acknowledgements_are_rebuilt_by_re_broadcasting_on_recovery() {
    // A message pending but not yet log-delivered when the process crashes. On recovering it
    // re-broadcasts, the answers come back, and the acknowledgements accumulate again.
    let mut s = sim(4);
    s.command(A, Cmd::Broadcast(6));
    s.run_for(Duration::from_millis(2)); // A has it pending; nobody has a majority yet
    s.crash(A);
    s.restart(A);
    settle(&mut s);

    assert_eq!(log_of(&s, A), vec![(A, 6)], "A log-delivered its own message after recovering");
    let id = BroadcastId { origin: A, seq: 1 };
    assert!(
        s.protocol(A).unwrap().acknowledged_by(id).count() >= s.protocol(A).unwrap().majority(),
        "and the acknowledgements were rebuilt from the answers, not retrieved"
    );
}

#[test]
fn recovery_re_announces_what_was_already_log_delivered() {
    let mut s = sim(5);
    s.command(A, Cmd::Broadcast(3));
    settle(&mut s);

    let at = s.now();
    s.crash(B);
    s.restart(B);

    let after: Vec<usize> = s
        .trace()
        .events()
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Indicated { at: t, node, ind: Ind::Delivered(log) }
                if *node == B && *t >= at =>
            {
                Some(log.delivered_count())
            }
            _ => None,
        })
        .collect();
    assert!(after.first() == Some(&1), "told again on recovering: {after:?}");
}

// ------------------------------------------------- the majority boundary

#[test]
fn the_majority_boundary_is_pinned_at_an_even_membership() {
    // With an odd N, `2k > N` and `2k >= N` are the same predicate. Four makes half a real
    // quantity — the reason recorded in the majority-ack change.
    const FOUR: [NodeId; 4] = [A, B, C, D];
    type Four = LoggedUniformReliableBroadcast<u32>;
    let four = |seed: u64| -> Sim<Four> {
        Sim::new(Config::default().seed(seed), &FOUR, |me| {
            LoggedUniformReliableBroadcast::new(me, FOUR, interval())
        })
    };
    let delivered = |s: &Sim<Four>, n: NodeId| s.protocol(n).unwrap().log().delivered_count();

    assert_eq!(LoggedUniformReliableBroadcast::<u32>::new(A, FOUR, interval()).majority(), 3);

    let mut half = four(1);
    half.crash(C);
    half.crash(D);
    half.command(A, Cmd::Broadcast(1));
    half.run_for(Duration::from_millis(800));
    for n in [A, B] {
        assert_eq!(
            delivered(&half, n),
            0,
            "{n}: two of four is exactly half, and half is not more"
        );
    }

    let mut more = four(1);
    more.crash(D);
    more.command(A, Cmd::Broadcast(1));
    more.run_for(Duration::from_millis(800));
    for n in [A, B, C] {
        assert_eq!(delivered(&more, n), 1, "{n}: three of four is a majority");
    }
}

#[test]
fn without_a_majority_the_layer_blocks_rather_than_diverges() {
    for seed in 0..6u64 {
        let mut s = sim(seed);
        for n in [C, D, E] {
            s.crash(n);
        }
        s.command(A, Cmd::Broadcast(1));
        settle(&mut s);
        for n in [A, B] {
            assert!(log_of(&s, n).is_empty(), "seed {seed}: {n} blocked");
        }
        assert_eq!(log_of(&s, A), log_of(&s, B), "seed {seed}: and the survivors agree");
    }
}

#[test]
fn progress_resumes_when_enough_processes_recover() {
    let mut s = sim(6);
    for n in [C, D, E] {
        s.crash(n);
    }
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(200));
    assert!(log_of(&s, A).is_empty(), "no majority, so nothing yet");

    s.restart(C);
    settle(&mut s);
    for n in [A, B, C] {
        assert_eq!(log_of(&s, n), vec![(A, 1)], "{n} once a majority was available again");
    }
}

// ------------------------------------------------- bounds and non-vacuity

#[test]
fn the_durable_state_grows_with_messages_handled() {
    let mut s = sim(7);
    for v in 0..6u32 {
        s.command(A, Cmd::Broadcast(v));
    }
    settle(&mut s);
    let log = s.protocol(B).unwrap().log();
    assert_eq!(log.delivered_count(), 6, "one entry per message, for ever");
    assert_eq!(log.pending_count(), 6, "and pending is never pruned either");
}

#[test]
fn each_message_costs_a_fixed_number_of_appends_and_no_rewrites() {
    // What appending buys: the cost of recording a message does not depend on how many were
    // recorded before it. Rewriting the whole record would make it grow with every message.
    let counts: Vec<usize> = [1usize, 2, 4]
        .iter()
        .map(|n| {
            let mut s = sim(9);
            for v in 0..*n as u32 {
                s.command(A, Cmd::Broadcast(v));
            }
            settle(&mut s);
            s.trace().appends()
        })
        .collect();

    let per = counts[0];
    assert_eq!(counts, vec![per, 2 * per, 4 * per], "linear in messages: {counts:?}");

    let mut s = sim(9);
    s.command(A, Cmd::Broadcast(0));
    settle(&mut s);
    assert_eq!(
        s.trace().metadata_writes(),
        ALL.len(),
        "and the metadata is written once per process, at first start, never per message"
    );
}

#[test]
fn recovery_reads_re_announces_and_re_broadcasts_with_nothing_in_between() {
    // The three things recovery must do, done as one uninterruptible step: read what survived,
    // tell the layer above, and put back on the wire what was still pending.
    let mut s = sim(10);
    s.command(A, Cmd::Broadcast(2));
    s.run_for(Duration::from_micros(1)); // A has it pending; nothing has arrived anywhere
    assert_eq!(log_of(&s, A), vec![], "not yet log-delivered");

    let at = s.now();
    s.crash(A);
    s.restart(A);

    let after: Vec<_> = s
        .trace()
        .events()
        .iter()
        .skip_while(|e| {
            !matches!(e, TraceEvent::Recovered { node, at: t, .. }
            if *node == A && *t >= at)
        })
        .collect();
    assert!(matches!(after[0], TraceEvent::Recovered { had_state: true, .. }));
    assert!(
        matches!(after[1], TraceEvent::Indicated { node, .. } if *node == A),
        "the announcement is the very next thing"
    );
    assert!(
        after[2..].iter().take(ALL.len()).all(|e| matches!(e, TraceEvent::Sent { from, .. }
            if *from == A)),
        "and then it re-broadcast what was pending, still without handling anything"
    );

    settle(&mut s);
    assert_eq!(log_of(&s, A), vec![(A, 2)], "which is how it finished the job");
}

#[test]
fn the_agreement_assertions_are_not_vacuous() {
    let mut s = sim(8);
    for (n, v) in ALL.iter().zip([1u32, 2, 3, 4, 5]) {
        s.command(*n, Cmd::Broadcast(v));
    }
    settle(&mut s);
    for n in ALL {
        assert_eq!(log_of(&s, n).len(), ALL.len(), "{n} log-delivered every broadcast");
    }
}

#[test]
fn the_wire_survives_encoding() {
    let mut s = sim(9);
    s.enable_codec_check();
    s.command(A, Cmd::Broadcast(2));
    settle(&mut s);
    for n in ALL {
        assert_eq!(log_of(&s, n), vec![(A, 2)], "{n}");
    }
}
