//! Eager probabilistic broadcast against Module 3.7 — and against the fact that its headline
//! guarantee is probabilistic, so a single seeded run cannot be the evidence for it.
//!
//! The mechanism assertions come first because they are what will localise a failure when the
//! coverage sweep at the end goes red.

use core::time::Duration;
use recon_core::{Effect, Event, MemStore, NodeId, Time, step_with};
use recon_protocols::probabilistic_broadcast::{
    BroadcastId, Cmd, Config, Gossip, Ind, ProbabilisticBroadcast,
};
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

/// A window far larger than any test broadcasts, so deduplication is never the thing under test
/// except where a test says it is.
const ROOMY: usize = 1_000;

type Pb = ProbabilisticBroadcast<u32>;

fn pb(me: NodeId, config: Config) -> Pb {
    ProbabilisticBroadcast::new(me, ALL, config)
}

fn store() -> MemStore<core::convert::Infallible, core::convert::Infallible> {
    MemStore::default()
}

fn rng(seed: u64) -> rand_chacha::ChaCha8Rng {
    use rand::SeedableRng;
    rand_chacha::ChaCha8Rng::seed_from_u64(seed)
}

fn sim(seed: u64, config: Config) -> Sim<Pb> {
    Sim::new(SimConfig::default().seed(seed), &ALL, move |me| pb(me, config))
}

/// Every process that delivered `msg`, by the trace rather than by protocol internals.
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

// ------------------------------------------------- The mechanism: task 2.2 to 2.6

#[test]
fn a_broadcast_reaches_everyone_when_the_fanout_is_generous() {
    // Not the probabilistic claim — one seed, a fanout and round count chosen so that coverage is
    // overwhelming. This exists so that a total failure of the algorithm is obvious before the
    // sweep has to distinguish "below threshold" from "broken".
    let mut s = sim(1, Config::new(4, 5, ROOMY));
    s.command(A, Cmd::Broadcast(7));
    s.run_for(Duration::from_millis(500));

    assert_eq!(delivered_by(&s, 7), ALL, "seed 1, fanout 4, rounds 5");
}

#[test]
fn the_sender_delivers_to_itself_without_the_network() {
    // `trigger ⟨ pb, Deliver | self, m ⟩` sits in the broadcast handler, before any gossip.
    let mut p = pb(A, Config::new(2, 3, ROOMY));
    let mut ids = 0;
    let cmd = Event::Cmd(Cmd::Broadcast(1u32));
    let fx = step_with(&mut p, cmd, Time::ZERO, &mut rng(0), &mut store(), &mut ids);

    assert!(
        matches!(fx.first(), Some(Effect::Indicate(Ind::Deliver { from, msg: 1 })) if *from == A),
        "the sender's own delivery is the first thing that happens: {fx:?}"
    );
}

#[test]
fn a_relay_addresses_the_fanout_and_never_the_membership() {
    // The whole trade. A fan-out to all of Π is best-effort broadcast wearing this module's name.
    let fanout = 3;
    let mut p = pb(A, Config::new(fanout, 3, ROOMY));
    let mut ids = 0;
    let cmd = Event::Cmd(Cmd::Broadcast(1u32));
    let fx = step_with(&mut p, cmd, Time::ZERO, &mut rng(0), &mut store(), &mut ids);

    let mut addressed: Vec<NodeId> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Send { to, .. } => Some(*to),
            _ => None,
        })
        .collect();
    let sent = addressed.len();
    addressed.sort();
    addressed.dedup();

    assert_eq!(sent, fanout, "exactly the fanout, in a membership of {}", ALL.len());
    assert_eq!(addressed.len(), fanout, "and no peer addressed twice");
    assert!(!addressed.contains(&A), "and never itself — picktargets draws from Π \\ {{self}}");
    assert!(sent < ALL.len() - 1, "and strictly fewer than every peer, or this is not gossip");
}

#[test]
fn gossip_terminates() {
    // `if r > 1` is what bounds it. Without the decrement this runs for as long as the simulation
    // does, which is the failure this asserts against.
    //
    // Measurable only over a fair-loss link, which is the one Algorithm 3.9 names. Over a perfect
    // link this cannot be observed at all: the stubborn link beneath re-sends everything it has
    // ever sent on every tick, so transmissions never cease no matter what the gossip does.
    let mut s = sim(2, Config::new(3, 4, ROOMY));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(200));
    let settled = s.trace().len();

    s.run_for(Duration::from_millis(2_000));
    assert_eq!(s.trace().len(), settled, "ten times the time produced no further events");
}

#[test]
fn a_message_arriving_many_times_is_delivered_once() {
    // The redundancy is the algorithm's — the book names it on the same page — so a process
    // receiving the same message repeatedly is expected. Delivering it upward twice is not.
    let mut s = sim(3, Config::new(5, 6, ROOMY));
    s.command(A, Cmd::Broadcast(9));
    s.run_for(Duration::from_millis(500));

    for n in ALL {
        let count = s
            .trace()
            .indications_at(n)
            .filter(|i| matches!(i, Ind::Deliver { msg: 9, .. }))
            .count();
        assert_eq!(count, 1, "{n} delivered it {count} times");
    }
}

#[test]
fn the_redundancy_is_real_so_that_test_is_not_vacuous() {
    // If no process ever received a message twice, deduplicating would be untested. The book's
    // "any given process may receive the same message many times" is checked, not assumed.
    let mut s = sim(3, Config::new(5, 6, ROOMY));
    s.command(A, Cmd::Broadcast(9));
    s.run_for(Duration::from_millis(500));

    let deliveries = s.trace().deliveries().count();
    assert!(
        deliveries > ALL.len(),
        "{deliveries} network deliveries for {} processes — the relay is not redundant, so the \
         deduplication assertion above proves nothing",
        ALL.len()
    );
}

#[test]
fn nothing_is_delivered_that_was_not_broadcast() {
    let mut s = sim(4, Config::new(3, 4, ROOMY));
    s.command(A, Cmd::Broadcast(11));
    s.command(C, Cmd::Broadcast(22));
    s.run_for(Duration::from_millis(500));

    for n in ALL {
        for ind in s.trace().indications_at(n) {
            if let Ind::Deliver { msg, .. } = ind {
                assert!(*msg == 11 || *msg == 22, "{n} delivered {msg}, which nobody broadcast");
            }
        }
    }
}

#[test]
fn a_delivery_names_the_originator_not_the_relayer() {
    // A relayed message must be attributed to whoever broadcast it, which is why the identifier
    // carries the origin rather than the relay carrying the sender.
    let mut s = sim(5, Config::new(2, 5, ROOMY));
    s.command(A, Cmd::Broadcast(3));
    s.run_for(Duration::from_millis(500));

    for n in ALL {
        for ind in s.trace().indications_at(n) {
            if let Ind::Deliver { from, msg: 3 } = ind {
                assert_eq!(*from, A, "{n} attributed A's broadcast to {from}");
            }
        }
    }
}

#[test]
fn identical_content_broadcast_twice_is_delivered_twice() {
    // The documented departure: identity is an identifier, not the message. Under the book's
    // content-keyed dedup the second broadcast would be swallowed.
    let mut s = sim(6, Config::new(5, 6, ROOMY));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(200));
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(500));

    let at_b =
        s.trace().indications_at(B).filter(|i| matches!(i, Ind::Deliver { msg: 1, .. })).count();
    assert_eq!(at_b, 2, "two broadcasts of the same value are two deliveries");
}

#[test]
fn the_same_seed_chooses_the_same_peers() {
    let run = |seed: u64| {
        let mut s = sim(seed, Config::new(3, 4, ROOMY));
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(500));
        s.trace().events().iter().map(|e| format!("{e:?}")).collect::<Vec<_>>()
    };
    assert_eq!(run(7), run(7), "the peer choice comes from the seeded rng and nowhere else");
}

#[test]
fn different_seeds_choose_differently() {
    // Non-vacuity for the test above: if every seed gossiped identically, reproducibility would be
    // trivially true and would say nothing about where the randomness comes from.
    let run = |seed: u64| {
        let mut s = sim(seed, Config::new(3, 4, ROOMY));
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(500));
        s.trace().events().iter().map(|e| format!("{e:?}")).collect::<Vec<_>>()
    };
    let traces: Vec<_> = (0..8).map(run).collect();
    assert!(!traces.iter().all(|t| *t == traces[0]), "differing seeds must gossip differently");
}

// ------------------------------------------------- The wire

#[test]
fn the_wire_survives_encoding() {
    let m = Gossip { id: BroadcastId { origin: A, incarnation: 0, seq: 3 }, ttl: 2, payload: 7u32 };
    assert_eq!(recon_sim::codec::round_trip(&m).expect("round trip"), m);
}

// ------------------------- The probabilistic guarantee itself: tasks 4.1 to 4.3
//
// PB1 holds *with high probability*, so no single seeded run is evidence for it. These sweeps run
// many seeds and count. Two halves are asserted every time: that coverage clears a stated
// threshold, and that it is **not total** — a configuration reaching everyone on every run has
// stopped being probabilistic, and an assertion that cannot fail is the failure mode
// `tests/method.rs` exists to guard against.
//
// # Where the thresholds come from
//
// One part is derived and one part is measured, and the difference is stated rather than blurred.
//
// **Derived.** With fanout `k` and `r` rounds, a broadcast reaches at most `1 + k + k² + … + k^(r−1)`
// processes, because each hop multiplies the frontier by at most `k` and the count decrements at
// each. For `k = 2, r = 2` that ceiling is `1 + 2 + 4 = 7`, below the eight processes here, so full
// coverage is *impossible* rather than unlucky — and measured at 0/200, as it must be. Any
// configuration whose ceiling is below `|Π|` is a configuration mistake, not a flaky test.
//
// **Measured.** Above that ceiling the actual rate is an epidemic process over a random graph, and
// this file does not pretend to derive it in closed form. The thresholds below are observed rates
// with a wide margin subtracted, and the observation is recorded beside each so that a failure is
// legible: if a sweep drops from 193 to 150, that is a regression to investigate, not a number to
// re-paste. Re-pasting a threshold is how a suite stops being evidence.

const SEEDS: u64 = 200;

/// How many of `SEEDS` runs reached every correct process.
fn coverage(fanout: usize, rounds: u32, loss: f64) -> usize {
    (0..SEEDS)
        .filter(|seed| {
            let mut s: Sim<Pb> =
                Sim::new(SimConfig::default().seed(*seed).loss(loss), &ALL, move |me| {
                    pb(me, Config::new(fanout, rounds, ROOMY))
                });
            s.command(A, Cmd::Broadcast(1));
            s.run_for(Duration::from_millis(500));
            delivered_by(&s, 1).len() == ALL.len()
        })
        .count()
}

#[test]
fn a_broadcast_usually_reaches_everyone() {
    // k=2, r=4, no loss. Ceiling 1+2+4+8 = 15 ≥ 8, so full coverage is possible.
    // Observed 193/200. Threshold 170 leaves a margin of 23 runs.
    let reached = coverage(2, 4, 0.0);
    assert!(reached >= 170, "{reached}/{SEEDS} reached everyone — observed 193 when written");
    assert!(
        reached < SEEDS as usize,
        "{reached}/{SEEDS}: coverage is total, so this is no longer a probabilistic broadcast and \
         the assertion above proves nothing"
    );
}

#[test]
fn the_guarantee_survives_a_lossy_link() {
    // The case the abstraction exists for, and the one a perfect link cannot show: 20% loss under
    // the fair-loss link Algorithm 3.9 names, with no retransmission anywhere in the stack.
    // k=3, r=3, loss 0.2. Observed 151/200. Threshold 120 leaves a margin of 31 runs.
    let reached = coverage(3, 3, 0.2);
    assert!(reached >= 120, "{reached}/{SEEDS} under 20% loss — observed 151 when written");
    assert!(reached < SEEDS as usize, "{reached}/{SEEDS}: loss is not costing anything");
}

#[test]
fn a_fanout_that_cannot_reach_everyone_never_does() {
    // The derived half. k=2, r=2 has a reach ceiling of 1+2+4 = 7 in a membership of 8, so this is
    // an arithmetic fact rather than an observation, and it pins the ceiling the thresholds above
    // are justified against.
    assert!(1 + 2 + 4 < ALL.len(), "the ceiling this test rests on");
    assert_eq!(coverage(2, 2, 0.0), 0, "a configuration below its ceiling reaches everyone never");
}

#[test]
fn widening_the_fanout_to_the_membership_defeats_the_assertion() {
    // Why the non-vacuity half is there. A fanout of |Π|−1 is best-effort broadcast wearing this
    // module's name: it reaches everyone every time, and every coverage assertion still "passes".
    // This test is what makes that a failure rather than a quiet success.
    let reached = coverage(ALL.len() - 1, 3, 0.0);
    assert_eq!(
        reached, SEEDS as usize,
        "a full fanout reaches everyone on every run — which is exactly why the sweeps above assert \
         coverage is not total"
    );
}

#[test]
fn a_run_that_missed_someone_reproduces_from_its_seed() {
    // A count is only useful if a failure inside it can be examined. This finds a seed that did not
    // reach everyone and replays it.
    let missed = (0..SEEDS)
        .find(|seed| {
            let mut s: Sim<Pb> = Sim::new(SimConfig::default().seed(*seed).loss(0.2), &ALL, |me| {
                pb(me, Config::new(3, 3, ROOMY))
            });
            s.command(A, Cmd::Broadcast(1));
            s.run_for(Duration::from_millis(500));
            delivered_by(&s, 1).len() < ALL.len()
        })
        .expect("some seed must fall short, or the sweep is not measuring anything");

    let replay = |seed: u64| {
        let mut s: Sim<Pb> = Sim::new(SimConfig::default().seed(seed).loss(0.2), &ALL, |me| {
            pb(me, Config::new(3, 3, ROOMY))
        });
        s.command(A, Cmd::Broadcast(1));
        s.run_for(Duration::from_millis(500));
        (
            delivered_by(&s, 1),
            s.trace().events().iter().map(|e| format!("{e:?}")).collect::<Vec<_>>(),
        )
    };
    let (first, trace) = replay(missed);
    let (again, trace_again) = replay(missed);

    assert!(first.len() < ALL.len(), "seed {missed} is the one that fell short");
    assert_eq!(first, again, "and it falls short the same way every time");
    assert_eq!(trace, trace_again, "down to the whole trace");
}

// ------------------------------- The retention window: tasks 3.1 to 3.3
//
// The book omits garbage collection explicitly — page 100, "omitted in the pseudo code for
// simplicity" — so this mechanism is the project's own and its cost is part of what the module
// claims. `docs/postmortem.md` records that the previous implementation's collector rebuilt the
// whole delivered-set on every event, making the cost of receiving one message linear in
// everything ever received. That is the one defect in that code which survived scrutiny.

/// Hand `p` a gossip as if it had arrived, and return what it emitted.
fn arrive(
    p: &mut Pb,
    origin: NodeId,
    seq: u64,
    payload: u32,
    r: &mut rand_chacha::ChaCha8Rng,
    ids: &mut u64,
) -> Vec<Effect<Gossip<u32>, Ind<u32>>> {
    let wire = Gossip { id: BroadcastId { origin, incarnation: 0, seq }, ttl: 1, payload };
    step_with(p, Event::Msg { from: B, msg: wire }, Time::ZERO, r, &mut store(), ids)
}

#[test]
fn state_does_not_grow_with_messages_handled() {
    let window = 16;
    let mut p = pb(A, Config::new(2, 3, window));
    let (mut r, mut ids) = (rng(0), 0);

    for seq in 1..=2_000u64 {
        arrive(&mut p, B, seq, seq as u32, &mut r, &mut ids);
    }

    assert_eq!(
        p.remembered(),
        window,
        "two thousand messages, one sender, a window of {window} — state is the window, not the run"
    );
}

#[test]
fn the_window_is_per_sender() {
    let window = 8;
    let mut p = pb(A, Config::new(2, 3, window));
    let (mut r, mut ids) = (rng(0), 0);

    for seq in 1..=100u64 {
        for origin in [B, C, D] {
            arrive(&mut p, origin, seq, seq as u32, &mut r, &mut ids);
        }
    }

    assert_eq!(p.remembered(), window * 3, "three senders, each keeping its own window");
}

#[test]
fn reclaiming_holds_the_cap_rather_than_sweeping_to_empty() {
    // This is what distinguishes eviction-on-insert from a periodic pass over the whole set. Under
    // a sweeping collector the size sawtooths — it grows, then collapses. Under this one it rises
    // to the cap and stays there, every single step. Observing the size after each insert is the
    // cheapest way to tell the two designs apart from the outside.
    let window = 10;
    let mut p = pb(A, Config::new(2, 3, window));
    let (mut r, mut ids) = (rng(0), 0);

    let mut sizes = Vec::new();
    for seq in 1..=200u64 {
        arrive(&mut p, B, seq, seq as u32, &mut r, &mut ids);
        sizes.push(p.remembered());
    }

    assert_eq!(&sizes[..window], &(1..=window).collect::<Vec<_>>()[..], "it fills one at a time");
    assert!(
        sizes[window..].iter().all(|n| *n == window),
        "and then never moves: a size that drops is a collector sweeping the set, which is the \
         shape whose cost grows with the run"
    );
}

#[test]
fn no_duplication_holds_within_the_window_and_not_beyond_it() {
    // The scope the window imposes, stated in the module's guarantee table as `PB2 [window]`. A
    // message re-arriving after its identifier has been evicted is delivered again — that is the
    // guarantee, not a violation of it.
    let window = 4;
    let mut p = pb(A, Config::new(2, 3, window));
    let (mut r, mut ids) = (rng(0), 0);

    let delivers = |fx: &[Effect<Gossip<u32>, Ind<u32>>]| {
        fx.iter().filter(|e| matches!(e, Effect::Indicate(Ind::Deliver { .. }))).count()
    };

    let first = arrive(&mut p, B, 1, 100, &mut r, &mut ids);
    assert_eq!(delivers(&first), 1, "the first arrival delivers");

    let again = arrive(&mut p, B, 1, 100, &mut r, &mut ids);
    assert_eq!(delivers(&again), 0, "and within the window a repeat does not");

    // Push it out of the window.
    for seq in 2..=(window as u64 + 2) {
        arrive(&mut p, B, seq, seq as u32, &mut r, &mut ids);
    }
    assert!(
        !p.has_delivered(BroadcastId { origin: B, incarnation: 0, seq: 1 }),
        "seq 1 has been evicted"
    );

    let beyond = arrive(&mut p, B, 1, 100, &mut r, &mut ids);
    assert_eq!(
        delivers(&beyond),
        1,
        "beyond the window it is delivered again, which is what `PB2 [window]` says"
    );
}

// ------------------------------------------------- identity survives the originator: task 1.2

#[test]
fn a_restarted_originators_broadcasts_are_delivered_not_discarded_as_duplicates() {
    // `BroadcastId` carries the originator's incarnation, drawn at `Init`. Without it a restarted
    // originator restarts its sequence at one and every receiver's window — still holding
    // `(A, 1..=3)` from before — would discard the new broadcasts as duplicates of the old.
    //
    // Fanout of the whole peer set and one round, so every broadcast reaches everyone directly and
    // the only thing that can stop a delivery is the window.
    let mut s = sim(31, Config { fanout: ALL.len() - 1, rounds: 1, window: ROOMY });
    for m in [1, 2, 3] {
        s.command(A, Cmd::Broadcast(m));
    }
    s.run_for(Duration::from_millis(200));
    s.crash(A);
    s.restart(A);
    for m in [4, 5, 6] {
        s.command(A, Cmd::Broadcast(m));
    }
    s.run_for(Duration::from_millis(200));

    for n in ALL {
        let got: Vec<u32> = s
            .trace()
            .indications_at(n)
            .filter_map(|i| match i {
                Ind::Deliver { from: A, msg } => Some(*msg),
                _ => None,
            })
            .collect();
        assert_eq!(got, vec![1, 2, 3, 4, 5, 6], "{n} lost the restarted originator's broadcasts");
    }

    // Non-vacuity: the sequence numbers really did collide, and only the incarnation told them
    // apart. Read from the wire.
    let ids: std::collections::BTreeSet<BroadcastId> =
        s.trace().sends().filter(|(from, _, _)| *from == A).map(|(_, _, g)| g.id).collect();
    let seqs: std::collections::BTreeSet<u64> = ids.iter().map(|id| id.seq).collect();
    let incarnations: std::collections::BTreeSet<u64> =
        ids.iter().map(|id| id.incarnation).collect();
    assert_eq!(seqs, [1, 2, 3].into_iter().collect(), "the sequence restarted at one both times");
    assert_eq!(incarnations.len(), 2, "two incarnations named themselves differently");
    assert_eq!(ids.len(), 6, "six distinct identifiers, where a bare (origin, seq) gives three");
}
