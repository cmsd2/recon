//! The total-order log suite, written against the port.
//!
//! Every property here names [`TotalOrderLog`] and no implementation, which is what makes it one
//! suite for the pair. What differs between the members is what survives a restart, and nothing
//! else; that half is asserted where the fail-recovery member is.

use core::time::Duration;
use recon_core::{NodeId, Position};
use recon_protocols::consensus_based_total_order_broadcast::ConsensusBasedTotalOrderBroadcast;
use recon_protocols::logged_uniform_total_order_broadcast::LoggedUniformTotalOrderBroadcast;
use recon_protocols::total_order_log::{LogInd, TotalOrderLog};
use recon_sim::{Config, Sim};

mod common;
use common::{A, ALL, B, BOUND, C, D, E, assert_send_rate_flat, timing};

type Tob = ConsensusBasedTotalOrderBroadcast<u32>;
type Lutob = LoggedUniformTotalOrderBroadcast<u32>;

// The step budget is raised for the same reason the fail-recovery member's own suite raises it:
// a transcription's stubborn children retransmit for ever, the crash property runs two settle
// windows, and `run_for` stops dispatching at the budget without saying so.
fn crash_stop(seed: u64) -> Sim<Tob> {
    let config = Config::default().seed(seed).synchronous(BOUND).max_steps(10_000_000);
    Sim::new(config, &ALL, |me| Tob::new(me, ALL, timing()))
}

fn fail_recovery(seed: u64) -> Sim<Lutob> {
    let config = Config::default().seed(seed).synchronous(BOUND).max_steps(10_000_000);
    Sim::new(config, &ALL, |me| Lutob::new(me, ALL, timing()))
}

fn settle<P: Log>(s: &mut Sim<P>) {
    // Any later instant, not a sequencing device: rounds decide within ~300ms and detection takes
    // 120ms, so this is still generous — while the idle churn beneath a settle is the stubborn
    // children resending everything ever sent every tick, which is what makes a long window cost.
    s.run_for(Duration::from_millis(2000));
}

/// The bounds every property below needs, written once. `Log` is the port; the rest is what the
/// simulator asks of any protocol.
trait Log:
    TotalOrderLog<u32, Cmd: Clone, Msg: Clone + PartialEq, Ind: Clone, Meta: Clone, Entry: Clone>
{
}

impl<P> Log for P where
    P: TotalOrderLog<
            u32,
            Cmd: Clone,
            Msg: Clone + PartialEq,
            Ind: Clone,
            Meta: Clone,
            Entry: Clone,
        >
{
}

/// The sequence `node` has ordered, read from its own indications — through the **port**, so this
/// works for any implementation behind it.
fn ordered_at<P: Log>(s: &Sim<P>, node: NodeId) -> Vec<u32> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match P::classify(i.clone()) {
            LogInd::Ordered { value, .. } => Some(value),
            _ => None,
        })
        .collect()
}

/// What a read returned, most recent first.
fn reads_at<P: Log>(s: &Sim<P>, node: NodeId) -> Vec<(Position, Vec<u32>)> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match P::classify(i.clone()) {
            LogInd::Contents { from, entries } => Some((from, entries)),
            _ => None,
        })
        .collect()
}

fn is_prefix(a: &[u32], b: &[u32]) -> bool {
    a.len() <= b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}

// ------------------------------------------------------------------ the shared properties
//
// Each is generic over the port, so the same code runs against both members of the pair. What
// differs between them is what survives a restart, and that is asserted where the fail-recovery
// member is.

fn prop_every_process_sees_one_sequence<P: Log>(mut s: Sim<P>) {
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, P::append(i as u32));
    }
    settle(&mut s);

    let seqs: Vec<Vec<u32>> = ALL.iter().map(|n| ordered_at(&s, *n)).collect();
    assert!(!seqs[0].is_empty(), "nothing was ordered, so nothing was compared");
    for a in &seqs {
        for b in &seqs {
            assert!(
                is_prefix(a, b) || is_prefix(b, a),
                "sequences diverged rather than agreeing on a common prefix: {seqs:?}"
            );
        }
    }
}

fn prop_everything_appended_is_ordered_everywhere<P: Log>(mut s: Sim<P>) {
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, P::append(100 + i as u32));
    }
    settle(&mut s);

    for node in ALL {
        let seq = ordered_at(&s, node);
        for i in 0..ALL.len() {
            assert!(seq.contains(&(100 + i as u32)), "{node} is missing an entry: {seq:?}");
        }
    }
}

fn prop_nothing_invented_and_nothing_twice<P: Log>(mut s: Sim<P>) {
    s.command(A, P::append(7));
    s.command(B, P::append(8));
    settle(&mut s);

    for node in ALL {
        let seq = ordered_at(&s, node);
        for v in &seq {
            assert!(*v == 7 || *v == 8, "{node} ordered {v}, which nobody appended");
        }
        let mut sorted = seq.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), seq.len(), "{node} ordered something twice: {seq:?}");
    }
}

fn prop_a_read_returns_the_sequence_from_a_position<P: Log>(mut s: Sim<P>) {
    s.command(A, P::append(11));
    s.command(B, P::append(22));
    settle(&mut s);
    s.command(A, P::read(Position::START));
    s.step_now();

    let got = reads_at(&s, A);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, Position::START);
    assert_eq!(got[0].1, ordered_at(&s, A), "a read agrees with what this process ordered");
    assert!(!got[0].1.is_empty(), "the read returned nothing, so it asserted nothing");
}

fn prop_reads_are_prefixes_of_one_another<P: Log>(mut s: Sim<P>) {
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, P::append(i as u32));
    }
    for _ in 0..40 {
        s.run_for(Duration::from_millis(50));
        for node in ALL {
            s.command(node, P::read(Position::START));
        }
        s.step_now();
    }
    settle(&mut s);

    let mut seen: Vec<Vec<u32>> = Vec::new();
    for node in ALL {
        for (_, entries) in reads_at(&s, node) {
            seen.push(entries);
        }
    }
    assert!(seen.len() > 10, "only {} reads, so little was compared", seen.len());
    assert!(seen.iter().any(|r| !r.is_empty()), "every read was empty");
    for a in &seen {
        for b in &seen {
            assert!(is_prefix(a, b) || is_prefix(b, a), "two reads disagreed: {a:?} vs {b:?}");
        }
    }
}

fn prop_the_same_sequence_at_every_process<P: Log>(mut s: Sim<P>) {
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, P::append((i as u32 + 1) * 10));
    }
    settle(&mut s);

    let seqs: Vec<Vec<u32>> = ALL.iter().map(|n| ordered_at(&s, *n)).collect();
    assert_eq!(seqs[0].len(), ALL.len(), "not everything was ordered: {:?}", seqs[0]);
    for seq in &seqs {
        assert_eq!(*seq, seqs[0], "the sort was not deterministic: {seqs:?}");
    }
}

/// The half this suite needs more than most. A total-order property is satisfied by a run in which
/// nothing overlapped, which is exactly what `tests/method.rs` exists to reject — so the run is
/// required to have contained overlapping operations, using the invocation intervals the trace
/// gained for this purpose.
fn prop_the_run_contained_overlapping_operations<P: Log>(mut s: Sim<P>) {
    let first = s.command(A, P::append(1));
    let second = s.command(B, P::append(2));
    let third = s.command(C, P::append(3));
    settle(&mut s);

    let began = |op| s.trace().invoked_at(op).expect("the operation began");
    let ended = |node: NodeId| {
        s.trace()
            .events()
            .iter()
            .rev()
            .find_map(|e| match e {
                recon_sim::TraceEvent::Indicated { at, node: n, .. } if *n == node => Some(*at),
                _ => None,
            })
            .expect("the process ordered something")
    };

    for op in [first, second, third] {
        assert!(began(op) < ended(A), "operations did not overlap");
    }
    assert!(began(first) < ended(B) && began(third) < ended(A));
    assert!(!ordered_at(&s, A).is_empty());
}

/// Tolerating a crash is the algorithm's reason to exist — Algorithm 6.1 assumes fail-stop with a
/// perfect failure detector, and the logged member a majority of correct processes — so a suite
/// that only ever runs fault-free tests a protocol that consensus was not needed for. One process
/// crashes for good, and the survivors must order an entry appended *after* the crash.
fn prop_the_survivors_keep_ordering_after_a_crash<P: Log>(mut s: Sim<P>) {
    s.command(A, P::append(1));
    settle(&mut s);
    assert!(!ordered_at(&s, A).is_empty(), "nothing was ordered before the crash");

    s.crash(E);
    s.command(B, P::append(2));
    settle(&mut s);

    let survivors = [A, B, C, D];
    for node in survivors {
        let seq = ordered_at(&s, node);
        assert!(
            seq.contains(&2),
            "{node} never ordered the entry appended after the crash: {seq:?}"
        );
    }
    let seqs: Vec<Vec<u32>> = survivors.iter().map(|n| ordered_at(&s, *n)).collect();
    for seq in &seqs {
        assert_eq!(*seq, seqs[0], "the survivors diverged: {seqs:?}");
    }
}

/// The transcription's space statement, checked where it can be: what a run sends per window must
/// not grow once the rounds have decided. The collections do grow with entries handled — that is
/// the page, and both modules say so — but nothing re-sends more as time passes.
fn prop_the_send_rate_does_not_grow<P: Log>(mut s: Sim<P>) {
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, P::append(i as u32));
    }
    settle(&mut s);
    assert_eq!(ordered_at(&s, A).len(), ALL.len(), "decided first, so the rate measured is idle");
    assert_send_rate_flat!(s, Duration::from_millis(400), 4);
}

/// One suite, both implementations. The properties above name the port and no implementation, which
/// is the whole reason for having one.
macro_rules! suite {
    ($name:ident, $build:ident) => {
        mod $name {
            use super::*;

            #[test]
            fn every_process_sees_one_sequence() {
                prop_every_process_sees_one_sequence($build(1));
            }
            #[test]
            fn everything_appended_is_ordered_everywhere() {
                prop_everything_appended_is_ordered_everywhere($build(2));
            }
            #[test]
            fn nothing_invented_and_nothing_twice() {
                prop_nothing_invented_and_nothing_twice($build(3));
            }
            #[test]
            fn a_read_returns_the_sequence_from_a_position() {
                prop_a_read_returns_the_sequence_from_a_position($build(4));
            }
            #[test]
            fn reads_are_prefixes_of_one_another() {
                prop_reads_are_prefixes_of_one_another($build(5));
            }
            #[test]
            fn the_same_sequence_at_every_process() {
                prop_the_same_sequence_at_every_process($build(8));
            }
            #[test]
            fn the_run_contained_overlapping_operations() {
                prop_the_run_contained_overlapping_operations($build(6));
            }
            #[test]
            fn the_send_rate_does_not_grow() {
                prop_the_send_rate_does_not_grow($build(12));
            }
            #[test]
            fn the_survivors_keep_ordering_after_a_crash() {
                prop_the_survivors_keep_ordering_after_a_crash($build(7));
            }
        }
    };
}

suite!(crash_stop_member, crash_stop);
suite!(fail_recovery_member, fail_recovery);

// ------------------------------------------------------------------ what only one member shows

/// **The instance family is not exercised by the crash-stop member, and this pins that.**
///
/// The family exists because the page has one — `c.round` is indexed and `⟨ c.r, Decide ⟩` names an
/// arbitrary `r`. A second argument was offered for it while this was being written: that processes
/// drift, so a message for round `r+1` would reach a process still in `r` and be dropped once,
/// unresent, by the deduplicating link beneath.
///
/// Measured, that does not arise here. Algorithm 6.1 is fail-stop and its consensus needs a perfect
/// failure detector, so it runs synchronously; every process decides a round in the same instant and
/// starts the next in the same instant. One replaced instance would have sufficed *for this member*.
///
/// So this asserts the lock-step rather than the drift. If it ever fails, the drift has appeared and
/// the family is earning its place.
#[test]
fn under_synchrony_no_process_runs_ahead_of_another() {
    let mut s = crash_stop(9);
    for (i, node) in ALL.iter().enumerate() {
        s.command_at(*node, Duration::from_millis(i as u64 * 40), Tob::append(i as u32));
    }

    let mut ahead = false;
    for _ in 0..8000 {
        if !s.step() {
            break;
        }
        if ALL.iter().any(|n| s.at(*n).instances() as u64 > s.at(*n).round()) {
            ahead = true;
        }
    }
    settle(&mut s);

    assert!(
        !ahead,
        "a process held an instance for a round it had not reached — the drift has appeared, and \
         the family is now doing work the lock-step made unnecessary. Read this before deleting it."
    );
    assert!(s.at(A).round() > 3, "only {} rounds ran", s.at(A).round() - 1);
    assert_eq!(ordered_at(&s, A).len(), ALL.len());
}
