//! The scenario shrinker, run against a defect this project actually had.
//!
//! A shrinker demonstrated only on a toy is a shrinker nobody has tested, so this reintroduces one
//! of the defects the leader-driven work already found and fixed —
//! [`LoggedEpochConsensus::with_reply_per_redelivery_defect`], where every redelivered `READ` and
//! `WRITE` was answered on a fresh stubborn transmission — and requires the reduction to survive
//! it. The scenario it starts from is the shape a search would hand over: the propose that matters,
//! buried in nine faults across five processes.
//!
//! # What came out
//!
//! ```text
//! steps 9 -> 1, nodes 5 -> 1, horizon 1.6s -> 400ms, 50 candidates run
//! ```
//!
//! One process, one `Propose`, four hundred milliseconds. Half a second of wall clock in a debug
//! build, six hundredths in release.
//!
//! The membership is the interesting part and was not what anyone expected. **The defect needs no
//! peers at all**: a single process that answers its own redelivered broadcast grows its own send
//! rate. That is a much stronger statement than "five processes and nine faults", and it is not one
//! the original investigation made.
//!
//! # The judgement, against the probe that originally found it
//!
//! Written down whichever way it came out, because a demonstration that only reports success is
//! not a demonstration.
//!
//! The reduction is worth **less** than `README.md`'s roadmap claimed for shrinking, and more than
//! nothing:
//!
//! - **It answers "with how little", and the answer was worth having.** Nine faults to one command,
//!   five processes to one. The hand-written probe
//!   (`the_send_rate_does_not_grow_after_the_epoch_has_decided`) never asked that question and so
//!   never discovered that one process suffices.
//! - **It does not answer "why".** The reduced scenario says a single process proposing grows its
//!   send rate. It does not say that the cause is a fresh stubborn transmission per redelivery.
//!   Getting from one to the other was still reading the code, which is item `F`, not this.
//! - **It charged for a predicate that names the property, and charged twice.** The first predicate
//!   here compared the two halves of a run, and the search returned a 17 ms scenario that satisfied
//!   it — and which the *sound* stack satisfied too, because a run that short is all startup. The
//!   second skipped the first quarter and returned an 80 ms one, which failed the same way: an
//!   epoch consensus is supposed to send more as it proceeds through `READ`, `WRITE` and `DECIDED`.
//!   Only the third — compare the last quarter with the third, in a run long enough to have gone
//!   idle — names the property the module actually claims. The shrinker did not mislead; it
//!   *exposed* that the predicate named a symptom, which is a real service. But getting the
//!   predicate right cost about what writing the probe cost.
//!
//! So: the honest claim is that a shrinker turns a found failure into a short one, and that a
//! short one is easier to read and occasionally says something the long one hid. It does not
//! replace the probe, and on this defect it would not have reached the diagnosis on its own.

use core::time::Duration;
use recon_core::{NodeId, Time};
use recon_protocols::logged_epoch_consensus::{Cmd, LoggedEpochConsensus, State};
use recon_sim::scenario::{Scenario, Step};
use recon_sim::{Config, Sim, shrink};

mod common;
use common::{A, ALL, B, BOUND, C, D, E, assert_send_rate_flat, retransmit};

const EPOCH: u64 = 7;

type Lep = LoggedEpochConsensus<u32>;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Every process runs the defective instance: it answers every redelivered `READ` and `WRITE`.
fn defective(config: Config, nodes: &[NodeId]) -> Sim<Lep> {
    let members: Vec<NodeId> = nodes.to_vec();
    Sim::new(config, nodes, move |me| {
        LoggedEpochConsensus::new(me, members.clone(), EPOCH, E, State::default(), retransmit())
            .with_reply_per_redelivery_defect()
    })
}

/// The same stack as it actually is.
fn sound(config: Config, nodes: &[NodeId]) -> Sim<Lep> {
    let members: Vec<NodeId> = nodes.to_vec();
    Sim::new(config, nodes, move |me| {
        LoggedEpochConsensus::new(me, members.clone(), EPOCH, E, State::default(), retransmit())
    })
}

/// The shortest run in which "work grows with time" is a claim about anything.
///
/// An epoch consensus is *supposed* to send more as it proceeds: `READ`, then `WRITE`, then
/// `DECIDED`, each joining a stubborn set that nothing empties. That is a bounded transient, and
/// it is growth by any measure taken across it. So the rate this predicate compares has to be the
/// *idle* one, which means the run must be long enough to have reached idle — the same reason the
/// hand-written probe calls `settle()` before it measures anything.
const OBSERVATION: Duration = Duration::from_millis(400);

/// **Work grows with how long the run has been going.**
///
/// The property, not the symptom: an epoch consensus' cost is bounded by membership, so what it
/// sends in a window should not depend on which window. Compares the third quarter of the run with
/// the fourth — the first half is where the epoch is still deciding — and allows the same tenth of
/// slack the suite's `assert_send_rate_flat!` allows.
///
/// Total, as a predicate must be: a run too short to judge, or one that sends nothing in the
/// window it measures from, returns `false` rather than asserting.
fn work_grows_with_time(sim: &Sim<Lep>) -> bool {
    let end = sim.now().as_offset();
    if end < OBSERVATION {
        return false;
    }
    let q = end / 4;
    let sends_in = |from: Duration, to: Duration| {
        sim.trace()
            .events()
            .iter()
            .filter(|e| matches!(e, recon_sim::TraceEvent::Sent { .. }))
            .filter(|e| e.at() > Time::from_offset(from) && e.at() <= Time::from_offset(to))
            .count()
    };
    let early = sends_in(q * 2, q * 3);
    let late = sends_in(q * 3, q * 4);
    early > 0 && late > early + early / 10
}

/// The scenario a hand-written probe would have started from: the propose that matters, buried in
/// the faults a search would have thrown at it.
fn found_by_search() -> Scenario<Cmd<u32>> {
    Scenario::new(Config::default().seed(20).synchronous(BOUND), ALL)
        .at(ms(0), Step::Command { node: E, cmd: Cmd::Propose(9) })
        .at(ms(50), Step::Suspend(B))
        .at(ms(70), Step::Resume(B))
        .at(ms(100), Step::Sever(C, D))
        .at(ms(150), Step::Reconnect(C, D))
        .at(ms(200), Step::Crash(D))
        .at(ms(250), Step::Restart(D))
        .at(ms(300), Step::Partition(vec![vec![A, B, C], vec![D], vec![E]]))
        .at(ms(350), Step::Heal)
        .horizon(ms(1600))
}

/// The switch puts the defect back, and the sound stack does not have it.
///
/// Without this the reduction below could be reducing nothing: a predicate that holds of every run
/// is satisfied by the empty one, and the shrinker would dutifully return it.
#[test]
fn the_reintroduced_defect_is_the_defect() {
    let s = found_by_search();
    assert!(work_grows_with_time(&Sim::run_scenario(&s, defective)));
    assert!(!work_grows_with_time(&Sim::run_scenario(&s, sound)));
}

/// The counterpart, in the suite's own vocabulary: what the defect breaks, the stack as it stands
/// keeps. The predicate above is a re-statement of this assertion in a form a search can use, so
/// the two had better agree.
#[test]
fn the_sound_stack_stays_flat_where_the_defect_grows() {
    let mut s = Sim::run_scenario(&found_by_search(), sound);
    assert_send_rate_flat!(s, Duration::from_millis(400), 4);
}

#[test]
fn the_shrinker_reduces_it() {
    let s = found_by_search();
    let r = shrink(
        &s,
        "work grows with how long the run has been going",
        defective,
        work_grows_with_time,
    );

    println!("{}", r.to_rust("minimal"));

    assert!(r.reduced(), "{}", r.report());
    assert!(work_grows_with_time(&Sim::run_scenario(&r.scenario, defective)), "{}", r.report());
    // What the reduction is claimed to buy: the faults go, and the run gets short enough to read.
    assert!(r.scenario.steps.len() <= 2, "{}", r.report());
    assert!(r.scenario.horizon < s.horizon, "{}", r.report());
    // And it is still the same defect: the sound stack does not exhibit it.
    assert!(!work_grows_with_time(&Sim::run_scenario(&r.scenario, sound)), "{}", r.report());
}
