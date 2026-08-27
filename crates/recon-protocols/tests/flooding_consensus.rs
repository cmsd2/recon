//! Flooding consensus against Module 5.1 — and, because the point of this protocol is the
//! assumption underneath it, that a false suspicion genuinely splits the decision.

use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, MemStore, NodeId, Time, TimerId, step_with};
use recon_protocols::flooding_consensus::{BebMsg, Cmd, Flood, FloodingConsensus, Ind, Wire};
use recon_sim::{Config, Sim, TraceEvent};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

/// The network's promise; everything else is derived from it.
const BOUND: Duration = Duration::from_millis(20);

fn retransmit() -> Duration {
    Duration::from_millis(10)
}
fn heartbeat() -> Duration {
    BOUND * 2
}
fn detect_after() -> Duration {
    heartbeat() * 3
}

type Fc = FloodingConsensus<u32>;

fn fc(me: NodeId) -> Fc {
    FloodingConsensus::new(me, ALL, retransmit(), heartbeat(), detect_after())
}

/// A synchronous run — the assumption the detector, and therefore this layer, depends on.
fn sim(seed: u64) -> Sim<Fc> {
    let s: Sim<Fc> = Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, fc);
    assert_eq!(s.delivery_bound(), Some(BOUND));
    s
}

/// Every decision `node` reported. More than one would be an integrity violation.
fn decisions(s: &Sim<Fc>, node: NodeId) -> Vec<u32> {
    s.trace().indications_at(node).map(|Ind::Decide(v)| *v).collect()
}

fn decision(s: &Sim<Fc>, node: NodeId) -> Option<u32> {
    decisions(s, node).first().copied()
}

/// Propose one value per process, in the order of `ALL`.
fn propose_all(s: &mut Sim<Fc>, values: [u32; 4]) {
    for (n, v) in ALL.iter().zip(values) {
        s.command(*n, Cmd::Propose(v));
    }
}

fn settle(s: &mut Sim<Fc>) {
    s.run_for(detect_after() * 8);
}

/// How many processes crashed during the run, read from the trace rather than assumed.
fn crashes(s: &Sim<Fc>) -> usize {
    s.trace().events().iter().filter(|e| matches!(e, TraceEvent::Crashed { .. })).count()
}

/// Whether anything sent from `a` reached `b` after `at` — the observable form of "reachable".
fn delivered_between(s: &Sim<Fc>, a: NodeId, b: NodeId, at: Time) -> bool {
    s.trace().events().iter().any(|e| {
        matches!(e, TraceEvent::Delivered { at: t, from, to, .. } if *t >= at && *from == a && *to == b)
    })
}

// ------------------------------------------------------------------ termination

#[test]
fn every_process_decides_when_nothing_fails() {
    let mut s = sim(1);
    propose_all(&mut s, [7, 3, 9, 5]);
    settle(&mut s);

    for n in ALL {
        assert_eq!(decision(&s, n), Some(3), "{n} decides the minimum proposal");
    }
}

#[test]
fn a_decision_is_reached_despite_crashes() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        propose_all(&mut s, [7, 3, 9, 5]);
        s.run_for(BOUND / 2);
        s.crash(A);
        settle(&mut s);

        for n in [B, C, D] {
            assert!(decision(&s, n).is_some(), "seed {seed}: {n} must decide");
        }
        let vs: Vec<Option<u32>> = [B, C, D].iter().map(|n| decision(&s, *n)).collect();
        assert!(vs.windows(2).all(|w| w[0] == w[1]), "seed {seed}: {vs:?}");
    }
}

#[test]
fn termination_is_bounded_by_the_membership() {
    // Crash processes one after another, each far enough apart to be detected in a separate
    // round. A round without a decision costs a crash, and there are only so many to spend.
    let mut s = sim(2);
    propose_all(&mut s, [7, 3, 9, 5]);
    s.run_for(BOUND);
    s.crash(A);
    s.run_for(detect_after() * 2);
    s.crash(B);
    s.run_for(detect_after() * 2);
    s.crash(C);
    settle(&mut s);

    assert!(decision(&s, D).is_some(), "the last correct process still decides");
    assert!(
        s.protocol(D).unwrap().round() <= ALL.len() as u64,
        "a run enters at most N rounds, was {}",
        s.protocol(D).unwrap().round()
    );
}

#[test]
fn a_round_completes_on_a_crash_indication_alone() {
    // The guard is a standing condition over state, not a message handler, and the crash
    // indication is the only event in this step. Driven directly rather than through the
    // simulator, because in a run the stubborn link's retransmit timer also re-enters the
    // broadcast child and would re-evaluate the guard for the wrong reason — this assertion
    // would then hold whether or not the detector path checks it. See the module note.
    let mut r = ChaCha8Rng::seed_from_u64(0);
    let at = Time::from_millis(1);
    let mut a = fc(A);
    // A composition, so one identity source across every call: two layers each starting at zero
    // would each accept the other's expiry.
    let mut ids = 0;
    let init = step_with(&mut a, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let tick = armed(&init);

    // A proposes, and hears its own round-1 message plus B's and C's — but never D's.
    let own =
        step_with(&mut a, Event::Cmd(Cmd::Propose(4)), Time::ZERO, &mut r, &mut store(), &mut ids);
    let to_self = addressed_to(&own, A).expect("the broadcast reaches the sender too");
    step_with(&mut a, Event::Msg { from: A, msg: to_self }, at, &mut r, &mut store(), &mut ids);
    for peer in [B, C] {
        let mut p = fc(peer);
        let sent = step_with(
            &mut p,
            Event::Cmd(Cmd::Propose(4)),
            Time::ZERO,
            &mut r,
            &mut store(),
            &mut ids,
        );
        let msg = addressed_to(&sent, A).expect("a proposal addressed to A");
        let fx =
            step_with(&mut a, Event::Msg { from: peer, msg }, at, &mut r, &mut store(), &mut ids);
        assert!(round_broadcast(&fx, 2).is_none(), "the round cannot complete while D is awaited");
    }

    // A tick inside the timeout accuses nobody; one beyond it accuses every silent peer. Nothing
    // but the detector's own timer happens in either step — no consensus message arrives.
    let inside = Time::from_offset(detect_after() / 2);
    let quiet = step_with(&mut a, Event::Timer(tick), inside, &mut r, &mut store(), &mut ids);
    assert!(round_broadcast(&quiet, 2).is_none(), "nobody is accused inside the timeout");

    // The detector re-armed as it fired, so the handle to fire now is the one it just registered.
    let tick = armed(&quiet);
    let beyond = Time::from_offset(detect_after() * 2);
    let accused = step_with(&mut a, Event::Timer(tick), beyond, &mut r, &mut store(), &mut ids);
    assert!(
        round_broadcast(&accused, 2).is_some(),
        "the crash indication alone must complete the round and broadcast round 2; if the guard \
         is only checked on the message path, nothing is emitted here"
    );
    assert_eq!(a.round(), 2);
}

#[test]
fn a_decided_from_an_accused_sender_is_discarded() {
    // `such that p ∈ correct ∧ decision = ⊥`. A process that has wrongly accused the decider
    // throws its decision away — half of why a false suspicion is unrecoverable, and the arm the
    // end-to-end suite never reaches, because under a perfect detector nobody is wrongly accused.
    //
    // Driven directly: the fault is a detector that lies, and the simulator's synchronous mode
    // exists precisely to stop it lying.
    let mut s = sim(1);
    propose_all(&mut s, [4, 4, 4, 4]);
    settle(&mut s);
    let (decider, wire) = s
        .trace()
        .events()
        .iter()
        .find_map(|e| match e {
            TraceEvent::Sent { from, to, msg: Wire::Broadcast(w), .. }
                if *to != *from && matches!(w.payload, Flood::Decided(_)) =>
            {
                Some((*from, Wire::Broadcast(w.clone())))
            }
            _ => None,
        })
        .expect("some process broadcast its decision");

    let mut r = ChaCha8Rng::seed_from_u64(0);
    let at = Time::from_offset(detect_after() * 3);

    // A hears no heartbeat at all, so one tick beyond the timeout accuses every peer. It has not
    // proposed, so `correct = {A}` cannot complete a round either: A is undecided and wrong.
    let mut accuser = fc(A);
    let mut ids = 0;
    let init = step_with(&mut accuser, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let tick = armed(&init);
    step_with(&mut accuser, Event::Timer(tick), at, &mut r, &mut store(), &mut ids);
    assert!(!accuser.correct().any(|p| p == decider), "A has accused the decider");
    assert!(accuser.decision().is_none(), "and decided nothing of its own");

    let fx = step_with(
        &mut accuser,
        Event::Msg { from: decider, msg: wire.clone() },
        at,
        &mut r,
        &mut store(),
        &mut ids,
    );
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::Indicate(Ind::Decide(_)))),
        "the decision of an accused process is discarded, however correct it was"
    );
    assert_eq!(accuser.decision(), None, "so A is still undecided, and now permanently split");

    // Non-vacuity: the very same message, at a process that has accused nobody, decides. Without
    // this the assertion above would pass on a protocol that ignored DECIDED entirely.
    let mut listener = fc(A);
    let mut ids = 0;
    step_with(&mut listener, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let fx = step_with(
        &mut listener,
        Event::Msg { from: decider, msg: wire },
        Time::from_millis(1),
        &mut r,
        &mut store(),
        &mut ids,
    );
    assert!(
        fx.iter().any(|e| matches!(e, Effect::Indicate(Ind::Decide(_)))),
        "the same message from a process still in `correct` decides"
    );
    assert!(listener.decision().is_some());
}

/// The handle the composition just registered, read out of what it emitted rather than assumed.
///
/// A test firing a timer must name the one the protocol is actually waiting on. Identities come
/// from the driver's source, so they are not predictable from the test's side — which is the
/// point: a stale expiry is distinguishable from a live one, and a test that guessed would be
/// asserting against a timer nobody is waiting for.
fn armed<M, I>(fx: &[Effect<M, I>]) -> TimerId {
    fx.iter()
        .find_map(|e| match e {
            Effect::SetTimer { id, .. } => Some(*id),
            _ => None,
        })
        .expect("a timer was armed")
}

/// A fresh store per call: this stack writes nothing durably.
fn store() -> MemStore<core::convert::Infallible, core::convert::Infallible> {
    MemStore::default()
}

/// The message this broadcast addressed to `to`, if any.
fn addressed_to(
    fx: &[Effect<Wire<BebMsg<u32>>, Ind<u32>>],
    to: NodeId,
) -> Option<Wire<BebMsg<u32>>> {
    fx.iter().find_map(|e| match e {
        Effect::Send { to: t, msg } if *t == to => Some(msg.clone()),
        _ => None,
    })
}

/// A proposal broadcast for `round`, if these effects contain one.
fn round_broadcast(fx: &[Effect<Wire<BebMsg<u32>>, Ind<u32>>], round: u64) -> Option<()> {
    fx.iter().find_map(|e| match e {
        Effect::Send { msg: Wire::Broadcast(w), .. } => match &w.payload {
            Flood::Proposal { round: r, .. } if *r == round => Some(()),
            _ => None,
        },
        _ => None,
    })
}

#[test]
fn a_crashed_process_is_not_waited_for_and_the_round_carries_forward() {
    // The same property end to end, and the carry with it. A crashes before proposing, so no
    // process can complete round 1 — `receivedfrom[0]` is the full membership — and all three
    // survivors must accuse A and move to round 2. Nobody decides in round 1, so no DECIDED
    // message short-circuits anything: the round-2 broadcast must carry `proposals[1]` under the
    // new round number, or the accumulated set is empty and nothing can be decided at all.
    let mut s = sim(3);
    s.crash(A);
    for (n, v) in [(B, 7u32), (C, 9), (D, 5)] {
        s.command(n, Cmd::Propose(v));
    }
    settle(&mut s);

    for n in [B, C, D] {
        assert_eq!(decision(&s, n), Some(5), "{n} decided the minimum the survivors carried");
        assert!(!s.protocol(n).unwrap().correct().any(|p| p == A), "{n} accused A");
        // `receivedfrom[0]` is the full membership, so round 1 heard from three of four and
        // could not decide. Seeding it with the surviving set instead would decide a round early.
        assert_eq!(s.protocol(n).unwrap().round(), 2, "{n} needed a second round");
    }
}

// --------------------------------------------------------------------- validity

#[test]
fn nothing_is_decided_that_was_not_proposed() {
    for seed in 0..8u64 {
        let proposed = [11u32, 22, 33, 44];
        let mut s = sim(seed);
        propose_all(&mut s, proposed);
        settle(&mut s);

        for n in ALL {
            for v in decisions(&s, n) {
                assert!(proposed.contains(&v), "seed {seed}: {n} decided {v}, unproposed");
            }
        }
    }
}

#[test]
fn a_unanimous_proposal_is_the_decision() {
    let mut s = sim(4);
    propose_all(&mut s, [8, 8, 8, 8]);
    settle(&mut s);
    for n in ALL {
        assert_eq!(decision(&s, n), Some(8), "{n}");
    }
}

// -------------------------------------------------------------------- integrity

#[test]
fn no_process_decides_twice() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        propose_all(&mut s, [7, 3, 9, 5]);
        s.run_for(BOUND * 2);
        s.crash(C);
        settle(&mut s);
        for n in [A, B, D] {
            assert_eq!(decisions(&s, n).len(), 1, "seed {seed}: {n} decided more than once");
        }
    }
}

#[test]
fn a_decision_arriving_afterwards_does_not_re_decide() {
    // Every process broadcasts DECIDED on deciding, and every process relays it on adopting one,
    // so decisions keep arriving well after everyone has decided. None of them counts twice.
    let mut s = sim(5);
    propose_all(&mut s, [7, 3, 9, 5]);
    settle(&mut s);
    let after_first = s.trace().indication_count();
    settle(&mut s);

    assert_eq!(s.trace().indication_count(), after_first, "no indication after the decisions");
    for n in ALL {
        assert_eq!(decisions(&s, n).len(), 1, "{n}");
    }
}

// -------------------------------------------------------------------- agreement

#[test]
fn agreement_holds_under_crashes_while_the_detector_is_perfect() {
    for seed in 0..12u64 {
        let mut s = sim(seed);
        propose_all(&mut s, [7, 3, 9, 5]);
        s.run_for(BOUND / 3);
        s.crash(A);
        s.run_for(BOUND * 2);
        s.crash(B);
        settle(&mut s);

        let decided: Vec<u32> = [C, D].iter().filter_map(|n| decision(&s, *n)).collect();
        assert_eq!(decided.len(), 2, "seed {seed}: both survivors must decide");
        assert_eq!(decided[0], decided[1], "seed {seed}: and decide the same value");
    }
}

#[test]
fn a_decider_crashing_immediately_afterwards_does_not_split_the_rest() {
    // The case the DECIDED broadcast exists for: one process completes its round and crashes
    // before the others complete theirs.
    let split = (0..40u64).find_map(|seed| {
        let mut s = sim(seed);
        propose_all(&mut s, [7, 3, 9, 5]);
        // Advance until exactly one process has decided, then crash it.
        let mut steps = 0;
        while steps < 400 {
            s.run_for(Duration::from_millis(1));
            steps += 1;
            let decided: Vec<NodeId> =
                ALL.iter().copied().filter(|n| decision(&s, *n).is_some()).collect();
            if decided.len() == 1 {
                let first = decided[0];
                s.crash(first);
                settle(&mut s);
                return Some((seed, first, s));
            }
            if decided.len() > 1 {
                return None;
            }
        }
        None
    });

    let Some((seed, first, s)) = split else {
        panic!("no seed produced a lone first decider — the scenario was never exercised");
    };
    let survivors: Vec<NodeId> = ALL.iter().copied().filter(|n| *n != first).collect();
    let decided: Vec<u32> = survivors.iter().filter_map(|n| decision(&s, *n)).collect();
    assert_eq!(decided.len(), survivors.len(), "seed {seed}: every survivor decides");
    assert!(decided.windows(2).all(|w| w[0] == w[1]), "seed {seed}: and agrees: {decided:?}");
    assert_eq!(
        decided[0],
        decision(&s, first).expect("the crashed process had decided"),
        "seed {seed}: and agrees with the process that decided first and then crashed"
    );
}

#[test]
fn the_same_proposal_set_yields_the_same_decision() {
    // The decision rule is deterministic and agreed in advance, so two processes that reach the
    // end of a round holding the same set need no further communication to agree.
    let mut s = sim(6);
    propose_all(&mut s, [7, 3, 9, 5]);
    settle(&mut s);
    let vs: Vec<u32> = ALL.iter().filter_map(|n| decision(&s, *n)).collect();
    assert_eq!(vs.len(), ALL.len());
    assert!(vs.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(vs[0], 3, "and it is the minimum, which is the function chosen in advance");
}

// ------------------------------------------- what strong accuracy is worth

/// A partition inside synchronous mode: delivery within each side stays bounded, while across it
/// nothing arrives, so each side accuses the other of crashing. Nobody crashes.
///
/// Returns the run, so the caller can inspect what was decided and what was believed.
fn split_by_false_suspicion(seed: u64, heal_after: Option<Duration>) -> (Sim<Fc>, Time) {
    let mut s = sim(seed);
    s.partition(&[&[A, B], &[C, D]]);
    propose_all(&mut s, [7, 3, 9, 5]);
    s.run_for(detect_after() * 4);
    let healed_at = s.now();
    if let Some(d) = heal_after {
        s.heal();
        s.run_for(d);
    }
    (s, healed_at)
}

#[test]
fn a_false_suspicion_splits_the_decision() {
    let found = (0..20u64).find(|seed| {
        let (s, _) = split_by_false_suspicion(*seed, None);
        let ab = decision(&s, A).zip(decision(&s, B));
        let cd = decision(&s, C).zip(decision(&s, D));
        match (ab, cd) {
            (Some((a, _)), Some((c, _))) => a != c,
            _ => false,
        }
    });
    assert!(
        found.is_some(),
        "no schedule split the decision — if a false suspicion cannot split it, this protocol is \
         not the algorithm its specification describes"
    );
}

#[test]
fn the_split_is_an_agreement_failure_not_a_termination_failure() {
    let (s, _) = split_by_false_suspicion(0, None);

    // Both sides decided: this is not a liveness failure wearing a disguise.
    for n in ALL {
        assert!(decision(&s, n).is_some(), "{n} must have decided for this to be about agreement");
    }
    assert_ne!(decision(&s, A), decision(&s, C), "and the two sides decided differently");
    assert_eq!(decision(&s, A), decision(&s, B), "each side agreed with itself");
    assert_eq!(decision(&s, C), decision(&s, D));

    // Nobody crashed. The disagreement is between processes that are correct throughout.
    assert_eq!(crashes(&s), 0, "no process crashed in this run");
}

#[test]
fn the_correct_set_did_not_decay() {
    let (s, healed_at) = split_by_false_suspicion(0, Some(detect_after() * 8));

    assert_eq!(crashes(&s), 0, "no process crashed");
    for (a, b) in [(A, C), (C, A), (B, D), (D, B)] {
        assert!(
            delivered_between(&s, a, b, healed_at),
            "{a} and {b} are reachable again — messages cross the old boundary after the heal"
        );
    }

    // Each side holds a non-empty proper subset of the membership, wrongly. The model is not one
    // in which `correct` decays towards empty; it is one in which two views disagree.
    for (side, other) in [([A, B], [C, D]), ([C, D], [A, B])] {
        for n in side {
            let held: Vec<NodeId> = s.protocol(n).unwrap().correct().collect();
            assert_eq!(held, side.to_vec(), "{n} holds its own side and no more");
            assert!(!held.is_empty());
            assert!(held.len() < ALL.len(), "a proper subset");
            assert!(other.iter().all(|o| !held.contains(o)), "and the two views are disjoint");
        }
    }
}

#[test]
fn the_split_outlives_the_system_stabilising() {
    // Heal the partition and run on well past the point where every process can reach every
    // other. An eventually perfect detector would withdraw both false suspicions here and every
    // process would again be held correct by every other — and the decisions would still stand,
    // because a decision is irrevocable and both were taken before stability returned.
    let (during, _) = split_by_false_suspicion(0, None);
    let (ab, cd) = (decision(&during, A), decision(&during, C));
    assert_ne!(ab, cd, "the run split before healing");

    let (after, healed_at) = split_by_false_suspicion(0, Some(detect_after() * 12));
    assert!(
        delivered_between(&after, A, C, healed_at) && delivered_between(&after, C, A, healed_at),
        "the sides really did become reachable again"
    );
    assert_eq!(decision(&after, A), ab, "A's decision stands");
    assert_eq!(decision(&after, C), cd, "C's decision stands");
    assert_ne!(decision(&after, A), decision(&after, C), "and they still disagree");
    for n in ALL {
        assert_eq!(decisions(&after, n).len(), 1, "{n} did not revise or repeat its decision");
    }
}

#[test]
fn the_same_schedule_with_an_accurate_detector_does_not_split() {
    // The difference must be attributable to the accuracy failure and not to the partition's
    // effect on delivery. Same partition, same duration, same proposals — but healed well before
    // the detector's timeout expires, so no false suspicion is ever raised.
    let mut s = sim(0);
    s.partition(&[&[A, B], &[C, D]]);
    propose_all(&mut s, [7, 3, 9, 5]);
    s.run_for(detect_after() / 3);
    s.heal();
    settle(&mut s);

    for n in ALL {
        assert!(
            s.protocol(n).unwrap().correct().count() == ALL.len(),
            "{n} suspected nobody, so the detector stayed accurate"
        );
        assert_eq!(decision(&s, n), Some(3), "{n} decided with everyone else");
    }
}

// ----------------------------------------------------- bounds and non-vacuity

#[test]
fn state_does_not_grow_with_messages_handled() {
    // Force several rounds by crashing processes in sequence, and let the run continue long past
    // the decision so that DECIDED traffic keeps arriving. State is bounded by membership and
    // the rounds actually entered, and by nothing else.
    let mut s = sim(7);
    propose_all(&mut s, [7, 3, 9, 5]);
    s.run_for(BOUND);
    s.crash(A);
    s.run_for(detect_after() * 2);
    s.crash(B);
    settle(&mut s);

    let messages = s.trace().delivery_count();
    let entries = s.protocol(D).unwrap().state_entries();
    let rounds = s.protocol(D).unwrap().rounds_recorded();

    assert!(messages > 200, "the run must be long enough for growth to show, was {messages}");
    assert!(rounds <= ALL.len() + 1, "at most N rounds plus the seeded round 0, was {rounds}");
    // One heard-from entry and one proposal per process per round, plus `correct`.
    let bound = (ALL.len() + 1) * 2 * ALL.len() + ALL.len();
    assert!(entries <= bound, "state is {entries}, bound is {bound}, after {messages} messages");
}

#[test]
fn a_second_proposal_is_not_a_second_consensus() {
    let mut s = sim(8);
    propose_all(&mut s, [7, 3, 9, 5]);
    settle(&mut s);
    assert_eq!(decision(&s, A), Some(3));

    s.command(A, Cmd::Propose(1));
    settle(&mut s);
    for n in ALL {
        assert_eq!(decisions(&s, n).len(), 1, "{n} reported no second decision");
        assert_eq!(decision(&s, n), Some(3), "{n} kept the value it decided");
    }
}

#[test]
fn the_agreement_assertions_are_not_vacuous() {
    // Every property above is satisfied by a protocol that decides nothing. Assert the floor.
    let mut s = sim(9);
    propose_all(&mut s, [7, 3, 9, 5]);
    settle(&mut s);
    let decided = ALL.iter().filter(|n| decision(&s, **n).is_some()).count();
    assert_eq!(decided, ALL.len(), "every process decided; the suites above are not vacuous");
    assert!(s.trace().indication_count() >= ALL.len());
}

#[test]
fn the_wire_survives_encoding() {
    let mut s = sim(10);
    s.enable_codec_check();
    propose_all(&mut s, [7, 3, 9, 5]);
    settle(&mut s);
    for n in ALL {
        assert_eq!(decision(&s, n), Some(3), "{n}");
    }
}

// -------------------------------------- The obligation a handle does not enforce

#[test]
fn a_layer_does_nothing_with_another_layers_expiry() {
    // The cost of a handle that carries no routing: an expiry is offered to every layer, so most
    // of what reaches one belongs to somebody else. A layer that acted on another's would run its
    // timeout logic at a moment an unrelated layer chose — here, the detector would judge silence
    // on the stubborn link's retransmit schedule. Nothing in the type system prevents that; this
    // test is what prevents it.
    let mut r = ChaCha8Rng::seed_from_u64(0);
    let mut a = fc(A);
    let mut ids = 0;

    // Two layers of one stack, each with a timer outstanding: the detector's tick, armed at
    // initialisation, and the stubborn link's retransmit, armed by the first broadcast.
    let init = step_with(&mut a, Event::Init, Time::ZERO, &mut r, &mut store(), &mut ids);
    let detector_tick = armed(&init);
    let cmd = Event::Cmd(Cmd::Propose(4));
    let sent = step_with(&mut a, cmd, Time::ZERO, &mut r, &mut store(), &mut ids);
    let link_tick = armed(&sent);
    assert_ne!(detector_tick, link_tick, "two layers, two handles");

    // Beyond the detection timeout, so a tick the detector *is* waiting on would accuse.
    let late = Time::from_offset(detect_after() * 2);
    let fx = step_with(&mut a, Event::Timer(link_tick), late, &mut r, &mut store(), &mut ids);
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::Indicate(Ind::Decide(_)))),
        "the link's expiry must not carry the detector's round forward"
    );
    assert!(
        a.correct().count() == ALL.len(),
        "and must not accuse anybody: the detector did not register that timer"
    );

    // Its own timer is still outstanding, which the detector's handle firing now demonstrates.
    let fx = step_with(&mut a, Event::Timer(detector_tick), late, &mut r, &mut store(), &mut ids);
    assert!(a.correct().count() < ALL.len(), "the detector's own expiry does accuse the silent");
    assert!(!fx.is_empty(), "and is acted upon rather than ignored in turn");
}
