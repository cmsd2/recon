//! Lazy probabilistic broadcast against Algorithms 3.10 and 3.11 — and against its reason to
//! exist, which is that recovery must measurably beat gossip alone.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::lazy_probabilistic_broadcast::{
    Cmd, Config, Data, Ind, LazyProbabilisticBroadcast, Recovery, Wire,
};
use recon_protocols::probabilistic_broadcast as pb;
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

fn config(fanout: usize, rounds: u32, store: f64) -> Config {
    Config {
        gossip: pb::Config::new(fanout, rounds, ROOMY),
        store_probability: store,
        request_rounds: 3,
        gap_timeout: Duration::from_millis(200),
        window: ROOMY,
    }
}

type Lpb = LazyProbabilisticBroadcast<u32>;

fn sim(seed: u64, loss: f64, config: Config) -> Sim<Lpb> {
    Sim::new(SimConfig::default().seed(seed).loss(loss), &ALL, move |me| {
        LazyProbabilisticBroadcast::new(me, ALL, config)
    })
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

fn reached_everyone(s: &Sim<Lpb>, msg: u32) -> bool {
    ALL.iter().all(|n| delivered_at(s, *n).contains(&msg))
}

// ------------------------------------------------- Algorithm 3.10: dissemination

#[test]
fn a_broadcast_reaches_everyone_when_nothing_is_lost() {
    let mut s = sim(1, 0.0, config(3, 4, 1.0));
    s.command(A, Cmd::Broadcast(7));
    s.run_for(Duration::from_millis(500));

    for n in ALL {
        assert_eq!(delivered_at(&s, n), vec![7], "{n}");
    }
}

#[test]
fn deliveries_from_one_sender_are_in_sequence() {
    // `if sn = next[s]` is the whole ordering mechanism. Out-of-order arrivals wait in `pending`
    // and are released by the standing condition, so what the layer above sees is in order.
    let mut s = sim(2, 0.15, config(3, 4, 1.0));
    for i in 1..=6u32 {
        s.command(A, Cmd::Broadcast(i));
        s.run_for(Duration::from_millis(30));
    }
    s.run_for(Duration::from_millis(2_000));

    for n in ALL {
        let got = delivered_at(&s, n);
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(got, sorted, "{n} delivered out of order: {got:?}");
    }
}

#[test]
fn nothing_is_delivered_twice() {
    let mut s = sim(3, 0.2, config(3, 4, 1.0));
    for i in 1..=5u32 {
        s.command(A, Cmd::Broadcast(i));
        s.run_for(Duration::from_millis(30));
    }
    s.run_for(Duration::from_millis(2_000));

    for n in ALL {
        let got = delivered_at(&s, n);
        let mut deduped = got.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(got.len(), deduped.len(), "{n} delivered something twice: {got:?}");
    }
}

#[test]
fn nothing_is_delivered_that_was_not_broadcast() {
    let mut s = sim(4, 0.2, config(3, 4, 1.0));
    s.command(A, Cmd::Broadcast(11));
    s.command(C, Cmd::Broadcast(22));
    s.run_for(Duration::from_millis(2_000));

    for n in ALL {
        for msg in delivered_at(&s, n) {
            assert!(msg == 11 || msg == 22, "{n} delivered {msg}, which nobody broadcast");
        }
    }
}

// ------------------------------------------------- Algorithm 3.11: recovery

#[test]
fn a_request_travels_over_the_link_and_not_through_the_gossip() {
    // `Uses: fll` is the half that makes this algorithm lazy. A request routed through the gossip
    // would flood the membership to repair one process's gap.
    let mut s = sim(5, 0.3, config(2, 3, 1.0));
    for i in 1..=4u32 {
        s.command(A, Cmd::Broadcast(i));
        s.run_for(Duration::from_millis(20));
    }
    s.run_for(Duration::from_millis(2_000));

    let requests = s
        .trace()
        .sends()
        .filter(|(_, _, m)| matches!(m, Wire::Recovery(Recovery::Request { .. })))
        .count();
    assert!(requests > 0, "a lossy run must produce gaps, or recovery is untested here");

    // That a request cannot travel through the gossip is structural rather than observed: the
    // gossip child's payload type is `Data`, which has no request variant, so `Wire::Gossip` cannot
    // hold one. The cost property — that one hop addresses the fanout rather than the membership —
    // is measured by `one_request_addresses_the_fanout_not_the_membership` below, by hand, because
    // requests are relayed (`else if r > 0 then gossip(…)`) and in aggregate a run's requests do
    // reach everyone. That is the algorithm working, not a flood.
}

#[test]
fn one_request_addresses_the_fanout_not_the_membership() {
    // Driven by hand, because a single hop is what the claim is about and a run mixes hops.
    use recon_core::{Effect, Event, MemStore, Time, step_with};

    let fanout = 3;
    let mut p: Lpb = LazyProbabilisticBroadcast::new(A, ALL, config(fanout, 3, 1.0));
    let mut r = {
        use rand::SeedableRng;
        rand_chacha::ChaCha8Rng::seed_from_u64(0)
    };
    let mut ids = 0;

    // A gossip arrives from B carrying B's fourth message, while A is still expecting B's first:
    // three gaps, and `forall missing ∈ [next[s], …, sn − 1]` requests each.
    let inner = pb::Gossip {
        id: pb::BroadcastId { origin: B, seq: 1 },
        ttl: 1,
        payload: Data { origin: B, seq: 4, payload: 9u32 },
    };
    let fx = step_with(
        &mut p,
        Event::Msg { from: B, msg: Wire::Gossip(inner) },
        Time::ZERO,
        &mut r,
        &mut MemStore::default(),
        &mut ids,
    );

    let mut per_gap: std::collections::BTreeMap<u64, Vec<NodeId>> = Default::default();
    for e in &fx {
        if let Effect::Send { to, msg: Wire::Recovery(Recovery::Request { seq, .. }) } = e {
            per_gap.entry(*seq).or_default().push(*to);
        }
    }

    assert_eq!(
        per_gap.keys().copied().collect::<Vec<_>>(),
        vec![1, 2, 3],
        "one request per missing sequence number, and none for the one that arrived"
    );
    for (seq, targets) in &per_gap {
        assert_eq!(targets.len(), fanout, "gap {seq} addressed {} peers", targets.len());
        assert!(targets.len() < ALL.len() - 1, "gap {seq} was flooded rather than pulled");
        assert!(!targets.contains(&A), "gap {seq} addressed the requester itself");
    }
}

#[test]
fn a_stored_copy_answers_a_request() {
    let mut s = sim(6, 0.3, config(2, 3, 1.0));
    for i in 1..=4u32 {
        s.command(A, Cmd::Broadcast(i));
        s.run_for(Duration::from_millis(20));
    }
    s.run_for(Duration::from_millis(2_000));

    let answers = s
        .trace()
        .sends()
        .filter(|(_, _, m)| matches!(m, Wire::Recovery(Recovery::Data(_))))
        .count();
    assert!(answers > 0, "a request that nobody answers is not a recovery mechanism");
}

#[test]
fn storing_nothing_means_answering_nothing() {
    // `store_probability` at zero is the book's α = 1: nobody keeps a copy, so no request can be
    // answered. The non-vacuity half of the test above — it shows the answers there came from the
    // store rather than from somewhere else.
    let mut s = sim(6, 0.3, config(2, 3, 0.0));
    for i in 1..=4u32 {
        s.command(A, Cmd::Broadcast(i));
        s.run_for(Duration::from_millis(20));
    }
    s.run_for(Duration::from_millis(2_000));

    let answers = s
        .trace()
        .sends()
        .filter(|(_, _, m)| matches!(m, Wire::Recovery(Recovery::Data(_))))
        .count();
    assert_eq!(answers, 0, "with nothing stored, no request can be answered");
}

#[test]
fn a_gap_nobody_can_close_does_not_stall_delivery_for_ever() {
    // `upon event ⟨ Timeout | s, sn ⟩ do if sn > next[s] then next[s] := sn + 1`. Without it a
    // process that misses one message waits for it for ever and delivers nothing after it, which
    // is worse than the miss.
    //
    // **The timeout skips `sn` as well.** `next[s] := sn + 1` moves past the message whose arrival
    // started the timer, not just the hole before it — so that message is abandoned even though it
    // is sitting in `pending`. This surprised the test that first asserted otherwise. It is the
    // page, and it is why the assertion below is about a *later* message rather than the one that
    // exposed the gap.
    let mut s = sim(7, 0.0, config(7, 3, 0.0));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(50));

    // Sever D, so it misses the second broadcast entirely.
    s.partition(&[&[A, B, C, E, F, G, H], &[D]]);
    s.command(A, Cmd::Broadcast(2));
    s.run_for(Duration::from_millis(100));
    s.heal();

    // Two more. The first exposes the gap and starts the timer; the second is what the timeout
    // releases once it has given up.
    s.command(A, Cmd::Broadcast(3));
    s.run_for(Duration::from_millis(50));
    s.command(A, Cmd::Broadcast(4));
    s.run_for(Duration::from_millis(3_000));

    let at_d = delivered_at(&s, D);
    assert!(at_d.contains(&1), "D had the first before the partition: {at_d:?}");
    assert!(!at_d.contains(&2), "and never gets the one it was severed from: {at_d:?}");
    assert!(
        at_d.contains(&4),
        "but must move past the gap rather than stalling on it for ever: {at_d:?}"
    );
    assert!(
        !at_d.contains(&3),
        "and 3 goes with the gap, because the timeout skips `sn` itself — surprising, and the \
         book's: {at_d:?}"
    );
}

#[test]
fn a_message_skipped_by_the_timeout_is_not_delivered_if_it_arrives_later() {
    // Having reported an order, the process cannot contradict it. `next[s]` only advances, so a
    // late arrival below it matches neither the delivery test nor the standing condition.
    let mut s = sim(11, 0.0, config(7, 3, 1.0));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(50));
    s.partition(&[&[A, B, C, E, F, G, H], &[D]]);
    s.command(A, Cmd::Broadcast(2));
    s.run_for(Duration::from_millis(100));
    s.heal();
    s.command(A, Cmd::Broadcast(3));
    s.run_for(Duration::from_millis(50));
    s.command(A, Cmd::Broadcast(4));
    s.run_for(Duration::from_millis(3_000));

    let at_d = delivered_at(&s, D);
    let mut sorted = at_d.clone();
    sorted.sort();
    assert_eq!(at_d, sorted, "whatever D delivered, it delivered in order: {at_d:?}");
    assert!(at_d.contains(&4), "and it did get past the gap: {at_d:?}");
}

// ------------------------------------ Why this abstraction exists

/// How many of `seeds` runs delivered the *first* message everywhere, with recovery and without.
///
/// Several broadcasts, not one, and the question is asked about the first. That is not incidental:
/// a gap is only visible as a hole in a sequence, so a process that misses the only message ever
/// sent has nothing to detect and recovery cannot possibly help. An earlier version of this helper
/// broadcast once and measured no benefit at all — the comment above it described sending several,
/// and the code did not.
fn coverage_with_and_without(seeds: u64, loss: f64, fanout: usize, rounds: u32) -> (usize, usize) {
    let run = |seed: u64, store: f64| {
        let mut s = sim(seed, loss, config(fanout, rounds, store));
        for i in 1..=5u32 {
            s.command(A, Cmd::Broadcast(i));
            s.run_for(Duration::from_millis(20));
        }
        s.run_for(Duration::from_millis(3_000));
        reached_everyone(&s, 1)
    };

    let with = (0..seeds).filter(|seed| run(*seed, 1.0)).count();
    // The same schedule with recovery disabled: nothing stored, so no request can ever be answered,
    // which leaves exactly the eager algorithm underneath.
    let without = (0..seeds).filter(|seed| run(*seed, 0.0)).count();
    (with, without)
}

#[test]
fn recovery_reaches_more_processes_than_gossip_alone() {
    // The claim the abstraction exists for, and the one that has to be shown rather than asserted.
    // 35% loss, fanout 2, three rounds. Observed 20/60 with recovery against 2/60 without.
    let (with, without) = coverage_with_and_without(60, 0.35, 2, 3);

    assert!(
        with > without,
        "recovery reached {with}/60 against gossip's {without}/60 — the abstraction has to earn \
         its second phase, and this is where it does"
    );
    assert!(with >= 12, "recovery reached only {with}/60 — observed 20 when written");
    assert!(
        without <= 8,
        "gossip alone reached {without}/60 — observed 2 when written; if the eager algorithm has \
         become this good, the comparison is no longer measuring recovery"
    );
}
