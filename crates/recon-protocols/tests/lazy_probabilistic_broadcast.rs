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
        id: pb::BroadcastId { origin: B, incarnation: 0, seq: 1 },
        ttl: 1,
        payload: Data { origin: B, incarnation: 0, seq: 4, payload: 9u32 },
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
// ------------------------------------ The bounds, which are the project's and not the book's

/// Hand `p` a gossip carrying `[DATA, origin, payload, seq]`, as if it had arrived.
fn arrive(
    p: &mut Lpb,
    origin: NodeId,
    seq: u64,
    payload: u32,
    r: &mut rand_chacha::ChaCha8Rng,
    ids: &mut u64,
) {
    use recon_core::{Event, MemStore, Time, step_with};
    let inner = pb::Gossip {
        id: pb::BroadcastId { origin, incarnation: 0, seq },
        ttl: 1,
        payload: Data { origin, incarnation: 0, seq, payload },
    };
    step_with(
        p,
        Event::Msg { from: origin, msg: Wire::Gossip(inner) },
        Time::ZERO,
        r,
        &mut MemStore::default(),
        ids,
    );
}

fn seeded() -> rand_chacha::ChaCha8Rng {
    use rand::SeedableRng;
    rand_chacha::ChaCha8Rng::seed_from_u64(0)
}

#[test]
fn stored_and_pending_are_both_bounded_by_the_window() {
    // Page 100 omits collection, so this is the project's design and its cost is part of what the
    // module claims. Both collections are fed here: everything is stored, and everything sits ahead
    // of a gap that never closes, which is the worst case for each.
    let window = 12;
    let mut cfg = config(2, 3, 1.0);
    cfg.window = window;
    let mut p: Lpb = LazyProbabilisticBroadcast::new(A, ALL, cfg);
    let (mut r, mut ids) = (seeded(), 0);

    // Start at 2, so sequence 1 is missing and nothing can ever be delivered.
    for seq in 2..=1_000u64 {
        arrive(&mut p, B, seq, seq as u32, &mut r, &mut ids);
    }

    assert_eq!(p.stored_count(), window, "stored is the window, not the run");
    assert_eq!(p.pending_count(), window, "and so is pending");
    assert_eq!(p.next_expected(B), 1, "with nothing delivered, since the gap never closed");
}

#[test]
fn the_windows_are_per_sender() {
    let window = 6;
    let mut cfg = config(2, 3, 1.0);
    cfg.window = window;
    let mut p: Lpb = LazyProbabilisticBroadcast::new(A, ALL, cfg);
    let (mut r, mut ids) = (seeded(), 0);

    for seq in 2..=200u64 {
        for origin in [B, C, D] {
            arrive(&mut p, origin, seq, seq as u32, &mut r, &mut ids);
        }
    }

    assert_eq!(p.stored_count(), window * 3, "three senders, each with its own window");
    assert_eq!(p.pending_count(), window * 3);
}

#[test]
fn a_request_for_something_evicted_is_answered_as_unavailable() {
    // `if exists m such that [DATA, s, m, sn] ∈ stored` — and if not, the request is relayed while
    // it has rounds left and then dropped. No unbounded search, and the requester's timeout is what
    // eventually moves it past the gap.
    use recon_core::{Effect, Event, MemStore, Time, step_with};

    let window = 4;
    let mut cfg = config(2, 3, 1.0);
    cfg.window = window;
    let mut p: Lpb = LazyProbabilisticBroadcast::new(A, ALL, cfg);
    let (mut r, mut ids) = (seeded(), 0);

    for seq in 2..=20u64 {
        arrive(&mut p, B, seq, seq as u32, &mut r, &mut ids);
    }
    assert!(!p.has_stored(B, 2), "sequence 2 has long since left the window");

    // Ask for it with no rounds left, so the only possible answer is the message itself.
    let request = Recovery::Request { requester: C, origin: B, incarnation: 0, seq: 2, ttl: 0 };
    let fx = step_with(
        &mut p,
        Event::Msg { from: C, msg: Wire::Recovery(request) },
        Time::ZERO,
        &mut r,
        &mut MemStore::default(),
        &mut ids,
    );

    assert!(
        fx.iter().all(|e| !matches!(e, Effect::Send { .. })),
        "nothing is sent: it is not held, and there are no rounds left to relay: {fx:?}"
    );
}

#[test]
fn a_request_it_cannot_answer_is_relayed_while_it_has_rounds() {
    // `else if r > 0 then gossip([REQUEST, q, s, sn, r − 1])`, preserving `q` so the answer goes to
    // the original requester rather than back down the relay chain.
    use recon_core::{Effect, Event, MemStore, Time, step_with};

    let mut p: Lpb = LazyProbabilisticBroadcast::new(A, ALL, config(3, 3, 1.0));
    let (mut r, mut ids) = (seeded(), 0);

    let request = Recovery::Request { requester: C, origin: B, incarnation: 0, seq: 5, ttl: 2 };
    let fx = step_with(
        &mut p,
        Event::Msg { from: D, msg: Wire::Recovery(request) },
        Time::ZERO,
        &mut r,
        &mut MemStore::default(),
        &mut ids,
    );

    let relayed: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Send {
                msg: Wire::Recovery(Recovery::Request { requester, ttl, .. }), ..
            } => Some((*requester, *ttl)),
            _ => None,
        })
        .collect();

    assert_eq!(relayed.len(), 3, "relayed to the fanout");
    assert!(
        relayed.iter().all(|(q, ttl)| *q == C && *ttl == 1),
        "the requester is preserved and the rounds decrement: {relayed:?}"
    );
}

// ------------------------------------------------- identity is the originator in one incarnation

/// `arrive`, from a named incarnation of `origin`, returning what came out.
fn arrive_as(
    p: &mut Lpb,
    origin: NodeId,
    incarnation: u64,
    seq: u64,
    payload: u32,
    r: &mut rand_chacha::ChaCha8Rng,
    ids: &mut u64,
) -> Vec<u32> {
    use recon_core::{Effect, Event, MemStore, Time, step_with};
    let inner = pb::Gossip {
        id: pb::BroadcastId { origin, incarnation, seq },
        ttl: 1,
        payload: Data { origin, incarnation, seq, payload },
    };
    step_with(
        p,
        Event::Msg { from: origin, msg: Wire::Gossip(inner) },
        Time::ZERO,
        r,
        &mut MemStore::default(),
        ids,
    )
    .into_iter()
    .filter_map(|e| match e {
        Effect::Indicate(Ind::Deliver { msg, .. }) => Some(msg),
        _ => None,
    })
    .collect()
}

#[test]
fn a_restarted_originators_messages_are_delivered_in_sequence_everywhere() {
    // `next[s]` is per originator in the book, and `sn < next[s]` is the case the pseudocode does
    // not write. A restarted originator numbers from one again; without the incarnation in the
    // sender every receiver would drop 4, 5 and 6 as already delivered — silently.
    let mut s = sim(41, 0.0, config(ALL.len() - 1, 1, 1.0));
    for m in [1, 2, 3] {
        s.command(A, Cmd::Broadcast(m));
    }
    s.run_for(Duration::from_millis(300));
    s.crash(A);
    s.restart(A);
    for m in [4, 5, 6] {
        s.command(A, Cmd::Broadcast(m));
    }
    s.run_for(Duration::from_millis(300));

    for n in ALL {
        assert_eq!(delivered_at(&s, n), vec![1, 2, 3, 4, 5, 6], "{n}");
    }
    // Non-vacuity: the sequence numbers really did repeat, and only the incarnation told them apart.
    let seqs: std::collections::BTreeSet<(u64, u64)> = s
        .trace()
        .sends()
        .filter_map(|(from, _, m)| match m {
            Wire::Gossip(g) if from == A => Some((g.payload.incarnation, g.payload.seq)),
            _ => None,
        })
        .collect();
    let incarnations: std::collections::BTreeSet<u64> = seqs.iter().map(|(i, _)| *i).collect();
    assert_eq!(incarnations.len(), 2, "two incarnations of A named themselves differently");
    for i in incarnations {
        assert_eq!(
            seqs.iter().filter(|(inc, _)| *inc == i).count(),
            3,
            "each incarnation numbered its three messages one, two, three"
        );
    }
}

#[test]
fn a_straggler_from_the_previous_incarnation_still_lands() {
    // Two incarnations are remembered, not one: relayed copies from the incarnation being retired
    // can arrive after the new one has begun, and a one-deep memory would flip between them.
    let mut p: Lpb = LazyProbabilisticBroadcast::new(A, ALL, config(2, 3, 1.0));
    let (mut r, mut ids) = (seeded(), 0);

    assert_eq!(arrive_as(&mut p, B, 1, 1, 11, &mut r, &mut ids), vec![11]);
    assert_eq!(arrive_as(&mut p, B, 1, 2, 12, &mut r, &mut ids), vec![12]);
    assert_eq!(arrive_as(&mut p, B, 2, 1, 21, &mut r, &mut ids), vec![21], "the new incarnation");
    assert_eq!(arrive_as(&mut p, B, 1, 3, 13, &mut r, &mut ids), vec![13], "the straggler");
    assert_eq!(arrive_as(&mut p, B, 2, 2, 22, &mut r, &mut ids), vec![22], "undisturbed");
    assert_eq!(p.incarnations_of(B), 2);
}

#[test]
fn a_third_incarnation_retires_the_oldest() {
    let mut p: Lpb = LazyProbabilisticBroadcast::new(A, ALL, config(2, 3, 1.0));
    let (mut r, mut ids) = (seeded(), 0);
    for inc in [1, 2] {
        assert_eq!(arrive_as(&mut p, B, inc, 1, inc as u32, &mut r, &mut ids), vec![inc as u32]);
    }
    assert!(p.has_stored(B, 1), "the latest incarnation's message is stored");

    assert_eq!(arrive_as(&mut p, B, 3, 1, 3, &mut r, &mut ids), vec![3]);
    assert_eq!(p.incarnations_of(B), 2, "two remembered, not three");

    // Incarnation 1 is gone: its `next` was 2, and a message numbered 2 from it is now from a
    // sender never heard from — ahead of a gap at 1, held and requested, not delivered.
    let out = arrive_as(&mut p, B, 1, 2, 12, &mut r, &mut ids);
    assert!(out.is_empty(), "a retired incarnation's message was delivered: {out:?}");
    assert_eq!(p.pending_count(), 1, "held ahead of the gap the retirement opened");
    // The bound is the claim: three incarnations' worth of state would be a leak per restart.
    assert!(p.stored_count() <= 2 * 3, "stored holds at most two incarnations' windows");
}
