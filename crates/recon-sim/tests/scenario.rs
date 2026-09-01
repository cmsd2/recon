//! Verifies that a run can be described as a value, executed from it, and reduced.
//!
//! The protocol is deliberately trivial — one process sends a number to another — so that
//! anything observed here is the scenario machinery's behaviour and not an algorithm's. The
//! demonstration against a defect this project actually had lives in `recon-protocols`, where
//! that defect is; see `logged_epoch_consensus_shrinking.rs`.

use core::convert::Infallible;
use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use recon_sim::scenario::{Scenario, Step};
use recon_sim::{Config, DropReason, Sim, TraceEvent, shrink};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);

const HOP: Duration = Duration::from_millis(10);

/// Sends one number when told to, and says what arrived.
struct Parrot;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cmd {
    SendTo(NodeId, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Wire(u32);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Got(NodeId, u32);

impl Protocol for Parrot {
    type Cmd = Cmd;
    type Ind = Got;
    type Msg = Wire;
    type Scope = core::convert::Infallible;
    type Note = core::convert::Infallible;
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::SendTo(to, n): Cmd, cx: &mut ProtoCx<'_, Self>) {
        cx.send(to, Wire(n));
    }

    fn on_msg(&mut self, from: NodeId, Wire(n): Wire, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(Got(from, n));
    }

    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}
}

fn config() -> Config {
    Config::default().latency(HOP, HOP)
}

fn build(config: Config, nodes: &[NodeId]) -> Sim<Parrot> {
    Sim::new(config, nodes, |_| Parrot)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// `C` was told that `A` sent it 7 — the property every reduction below is hunting.
fn c_got_seven(sim: &Sim<Parrot>) -> bool {
    sim.trace().indications_at(C).any(|g| *g == Got(A, 7))
}

fn events(sim: &Sim<Parrot>) -> Vec<TraceEvent<Wire, Got, Infallible>> {
    sim.trace().events().to_vec()
}

// ------------------------------------------------------------------ a run as data

/// The equivalence everything else rests on: a description does what the calls do.
#[test]
fn a_described_run_and_an_imperative_one_agree() {
    let described = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(5), Step::Sever(A, B))
        .at(ms(20), Step::Crash(B))
        .at(ms(30), Step::Restart(B))
        .at(ms(40), Step::Heal)
        .at(ms(50), Step::Command { node: B, cmd: Cmd::SendTo(A, 1) })
        .horizon(ms(100));

    let from_data = Sim::run_scenario(&described, build);

    let mut by_hand = build(config(), &[A, B, C]);
    by_hand.run_until(recon_core::Time::from_offset(ms(0)));
    by_hand.command(A, Cmd::SendTo(C, 7));
    by_hand.run_until(recon_core::Time::from_offset(ms(5)));
    by_hand.sever(A, B);
    by_hand.run_until(recon_core::Time::from_offset(ms(20)));
    by_hand.crash(B);
    by_hand.run_until(recon_core::Time::from_offset(ms(30)));
    by_hand.restart(B);
    by_hand.run_until(recon_core::Time::from_offset(ms(40)));
    by_hand.heal();
    by_hand.run_until(recon_core::Time::from_offset(ms(50)));
    by_hand.command(B, Cmd::SendTo(A, 1));
    by_hand.run_until(recon_core::Time::from_offset(ms(100)));

    assert_eq!(events(&from_data), events(&by_hand));
    // Non-vacuity: the two agreeing on an empty trace would prove nothing.
    assert!(c_got_seven(&from_data));
    assert!(events(&from_data).len() > 5);
}

#[test]
fn a_description_executes_identically_every_time() {
    let s = Scenario::new(config().loss(0.3).duplication(0.2).seed(11), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(1), Step::Command { node: B, cmd: Cmd::SendTo(C, 8) })
        .at(ms(30), Step::Partition(vec![vec![A], vec![B, C]]))
        .horizon(ms(80));

    assert_eq!(events(&Sim::run_scenario(&s, build)), events(&Sim::run_scenario(&s, build)));
    assert!(!events(&Sim::run_scenario(&s, build)).is_empty());
}

/// Every fault the simulator accepts is expressible, and expressing it does something.
///
/// Without the second half this passes for a `Step` enum whose variants are all no-ops.
#[test]
fn every_fault_is_a_step() {
    let s = Scenario::new(config().sessions(), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(20), Step::Suspend(B))
        .at(ms(25), Step::Resume(B))
        .at(ms(30), Step::CrashOnNextWrite(B))
        .at(ms(35), Step::Sever(A, C))
        .at(ms(40), Step::Reconnect(A, C))
        .at(ms(45), Step::Partition(vec![vec![A, B], vec![C]]))
        .at(ms(50), Step::Heal)
        .at(ms(55), Step::BreakSession(A, B))
        .at(ms(60), Step::Crash(B))
        .at(ms(65), Step::Restart(B))
        .horizon(ms(90));

    let sim = Sim::run_scenario(&s, build);
    let seen = events(&sim);
    let count =
        |f: fn(&TraceEvent<Wire, Got, Infallible>) -> bool| seen.iter().filter(|e| f(e)).count();

    assert_eq!(count(|e| matches!(e, TraceEvent::Suspended { .. })), 1);
    assert_eq!(count(|e| matches!(e, TraceEvent::Resumed { .. })), 1);
    assert_eq!(count(|e| matches!(e, TraceEvent::Crashed { .. })), 1);
    assert_eq!(count(|e| matches!(e, TraceEvent::Restarted { .. })), 1);
    assert!(count(|e| matches!(e, TraceEvent::SessionEnded { .. })) > 0);
    assert!(count(|e| matches!(e, TraceEvent::SessionOpened { .. })) > 0);
}

// ------------------------------------------------------------------ the reduction

/// The non-vacuity guard for the whole module: a shrinker that returns its input is no shrinker.
#[test]
fn irrelevant_steps_are_removed() {
    let padded = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Sever(A, B))
        .at(ms(1), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(2), Step::Heal)
        .at(ms(3), Step::Sever(B, C))
        .at(ms(4), Step::Command { node: B, cmd: Cmd::SendTo(A, 1) })
        .at(ms(5), Step::Suspend(B))
        .at(ms(6), Step::Resume(B))
        .horizon(ms(500));

    let r = shrink(&padded, "C was told A sent 7", build, c_got_seven);

    assert!(r.reduced(), "{}", r.report());
    assert_eq!(
        r.scenario.steps.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>(),
        vec![Step::Command { node: A, cmd: Cmd::SendTo(C, 7) }],
        "{}",
        r.report()
    );
}

#[test]
fn the_horizon_shrinks_to_when_the_predicate_first_holds() {
    let long = Scenario::new(config(), [A, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .horizon(ms(5_000));

    let r = shrink(&long, "C was told A sent 7", build, c_got_seven);

    // The message is delivered one hop after it is sent, and not before.
    assert!(r.scenario.horizon >= HOP, "{:?}", r.scenario.horizon);
    assert!(r.scenario.horizon < HOP + Duration::from_millis(1), "{:?}", r.scenario.horizon);
}

#[test]
fn what_is_returned_still_fails() {
    let padded = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(1), Step::Sever(B, C))
        .at(ms(2), Step::Crash(B))
        .horizon(ms(400));

    let r = shrink(&padded, "C was told A sent 7", build, c_got_seven);

    assert!(c_got_seven(&Sim::run_scenario(&r.scenario, build)), "{}", r.report());
}

#[test]
fn reducing_twice_gives_the_same_answer() {
    let padded = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Sever(A, B))
        .at(ms(1), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(2), Step::Crash(B))
        .at(ms(3), Step::Restart(B))
        .horizon(ms(300));

    let first = shrink(&padded, "p", build, c_got_seven);
    let second = shrink(&padded, "p", build, c_got_seven);

    assert_eq!(first.scenario, second.scenario);
    assert_eq!(first.candidates, second.candidates);
    // Termination is by a measure that every accepted reduction strictly decreases; this is the
    // regression guard on it, since a search that thrashed would still terminate but would run
    // candidates without end.
    assert!(first.candidates < 200, "{} candidates for four steps", first.candidates);
}

#[test]
fn an_already_minimal_scenario_is_returned_unchanged() {
    let minimal = Scenario::new(config(), [A, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .horizon(HOP);

    let r = shrink(&minimal, "C was told A sent 7", build, c_got_seven);

    assert_eq!(r.scenario, minimal, "{}", r.report());
    assert!(!r.reduced());
}

/// Dropping a process is tried, and is tried last: what comes back is two processes and the one
/// command between them.
#[test]
fn a_process_the_predicate_does_not_need_is_dropped() {
    let padded = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(1), Step::Command { node: B, cmd: Cmd::SendTo(C, 8) })
        .horizon(ms(200));

    let r = shrink(&padded, "C was told A sent 7", build, c_got_seven);

    assert_eq!(r.scenario.nodes, vec![A, C], "{}", r.report());
    assert_eq!(r.scenario.steps.len(), 1, "{}", r.report());
}

/// A partition is the one fault with internal structure, and the search takes it apart.
#[test]
fn a_partition_is_simplified_by_merging_groups() {
    let cut = |sim: &Sim<Parrot>| {
        sim.trace()
            .events()
            .iter()
            .any(|e| matches!(e, TraceEvent::Dropped { reason: DropReason::Partitioned, .. }))
    };

    let severe = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Partition(vec![vec![A], vec![B], vec![C]]))
        .at(ms(1), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .horizon(ms(200));

    let r = shrink(&severe, "A's message to C was dropped as partitioned", build, cut);

    let groups: Vec<usize> = r
        .scenario
        .steps
        .iter()
        .filter_map(|(_, s)| match s {
            Step::Partition(g) => Some(g.len()),
            _ => None,
        })
        .collect();
    assert_eq!(groups, vec![2], "{}", r.report());
    assert_eq!(r.scenario.nodes, vec![A, C], "{}", r.report());
    assert!(cut(&Sim::run_scenario(&r.scenario, build)));
}

/// Two steps that only matter together survive, and everything around them does not.
#[test]
fn steps_that_matter_only_together_survive() {
    // Delivered only if the partition is healed before the message arrives — so the sever and
    // the heal are each pointless alone, and the pair is what the predicate needs.
    let holds = |sim: &Sim<Parrot>| {
        let seen = sim.trace().events();
        seen.iter()
            .any(|e| matches!(e, TraceEvent::Dropped { reason: DropReason::Partitioned, .. }))
            && sim.trace().indications_at(C).any(|g| *g == Got(A, 7))
    };

    let padded = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Sever(A, C))
        .at(ms(1), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(2), Step::Suspend(B))
        .at(ms(3), Step::Resume(B))
        .at(ms(20), Step::Reconnect(A, C))
        .at(ms(21), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(22), Step::Crash(B))
        .horizon(ms(300));

    let r = shrink(&padded, "a partitioned drop, then a delivery", build, holds);

    let kept: Vec<Step<Cmd>> = r.scenario.steps.iter().map(|(_, s)| s.clone()).collect();
    assert!(kept.contains(&Step::Sever(A, C)), "{}", r.report());
    assert!(kept.contains(&Step::Reconnect(A, C)), "{}", r.report());
    assert!(!kept.contains(&Step::Suspend(B)), "{}", r.report());
    assert!(!kept.contains(&Step::Crash(B)), "{}", r.report());
}

/// Deleting steps is what a reduction does, and a `Resume` without its `Suspend` is a run the
/// simulator refuses outright — so a reduction that did not repair the pairing would spend most of
/// its candidates on panics rather than on answers. It cost a real failure here to find that.
#[test]
fn a_reduction_never_leaves_a_resume_without_its_suspend() {
    let padded = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(2), Step::Suspend(B))
        .at(ms(3), Step::Resume(B))
        .at(ms(4), Step::Crash(B))
        .at(ms(5), Step::Restart(B))
        .horizon(ms(200));

    let r = shrink(&padded, "C was told A sent 7", build, c_got_seven);

    assert!(r.scenario.is_well_formed(), "{}", r.report());
    assert!(r.scenario.steps.len() < padded.steps.len(), "{}", r.report());

    // Non-vacuity: the check is capable of saying no.
    let broken = Scenario::<Cmd>::new(config(), [A, B]).at(ms(0), Step::Resume(B)).horizon(ms(1));
    assert!(!broken.is_well_formed());
}

#[test]
#[should_panic(expected = "does not satisfy")]
fn a_scenario_that_does_not_fail_is_refused() {
    let quiet = Scenario::new(config(), [A, C]).horizon(ms(50));
    let _ = shrink(&quiet, "C was told A sent 7", build, c_got_seven);
}

// ------------------------------------------------------------------ reporting

#[test]
fn the_report_names_the_predicate() {
    let padded = Scenario::new(config(), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(1), Step::Crash(B))
        .horizon(ms(200));

    let r = shrink(&padded, "C was told A sent 7", build, c_got_seven);
    let report = r.report();

    assert!(report.contains("C was told A sent 7"), "{report}");
    assert!(report.contains("candidates run"), "{report}");
}

/// The scenario the rendering below was produced from.
fn original() -> Scenario<Cmd> {
    Scenario::new(config().seed(4).loss(0.1), [A, B, C])
        .at(ms(0), Step::Command { node: A, cmd: Cmd::SendTo(C, 7) })
        .at(ms(5), Step::Partition(vec![vec![A, B], vec![C]]))
        .at(ms(9), Step::Heal)
        .at(ms(12), Step::Crash(B))
        .horizon(ms(60))
}

// The rendered command is `SendTo(..)`, which is what its derived `Debug` prints; the variants
// have to be in scope where a rendering is pasted, and this is that import.
use Cmd::*;

include!("rendered_scenario.rs.inc");
const RENDERED: &str = include_str!("rendered_scenario.rs.inc");

/// The rendering is checked by *being compiled and run*, not by being eyeballed.
///
/// `rendered_scenario.rs.inc` is the renderer's own output, committed, `include!`d so the compiler
/// checks it is valid Rust, and `include_str!`d so this test can check it is still what the
/// renderer produces. Change the renderer and this fails; paste the new output and it passes,
/// having been compiled on the way.
#[test]
fn the_rendering_reconstructs_the_scenario() {
    assert_eq!(original().to_rust("rendered"), RENDERED, "regenerate rendered_scenario.rs.inc");
    assert_eq!(rendered(), original());
    assert_eq!(
        events(&Sim::run_scenario(&rendered(), build)),
        events(&Sim::run_scenario(&original(), build))
    );
    assert!(!events(&Sim::run_scenario(&rendered(), build)).is_empty());
}

/// Regenerates `rendered_scenario.rs.inc` when the renderer changes:
/// `cargo test -p recon-sim --test scenario -- --ignored print_rendering --nocapture`.
#[test]
#[ignore = "a generator, not a check — the check is the_rendering_reconstructs_the_scenario"]
fn print_rendering() {
    print!("{}", original().to_rust("rendered"));
}
