//! Verifies that an operation's beginning is recorded, and that one which never began is recorded
//! as such rather than vanishing.
//!
//! The trace has always held what a process *concluded* and never what it was *asked*. These are
//! the left-hand ends of the intervals a checker reads; pairing them to the completions is
//! deliberately not done here, and `design.md` in this change says why.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, Time, TimerId};
use recon_sim::{Config, NotBegun, Sim, TraceEvent};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const ABSENT: NodeId = NodeId::new(99);

/// Takes a while over an operation, and says when it is done. Enough to give an operation a
/// beginning and an end that are not the same instant.
struct Worker;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cmd {
    Work(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Done;

impl Protocol for Worker {
    type Cmd = Cmd;
    type Ind = Done;
    type Msg = ();
    type Scope = core::convert::Infallible;
    type Note = core::convert::Infallible;
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, Cmd::Work(d): Cmd, cx: &mut ProtoCx<'_, Self>) {
        cx.set_timer(d);
    }
    fn on_msg(&mut self, _: NodeId, (): (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(Done);
    }
}

fn sim() -> Sim<Worker> {
    Sim::new(Config::default().seed(1), &[A, B], |_| Worker)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// When `node` first said it was done.
fn done_at(s: &Sim<Worker>, node: NodeId) -> Option<Time> {
    s.trace().events().iter().find_map(|e| match e {
        TraceEvent::Indicated { at, node: n, .. } if *n == node => Some(*at),
        _ => None,
    })
}

// ------------------------------------------------------------------ an identity

#[test]
fn no_two_operations_share_an_identity() {
    let mut s = sim();
    let mut ids = Vec::new();
    for i in 0..20 {
        ids.push(s.command(A, Cmd::Work(ms(i))));
        ids.push(s.command_at(B, ms(i), Cmd::Work(ms(1))));
    }
    let distinct: std::collections::BTreeSet<_> = ids.iter().copied().collect();
    assert_eq!(distinct.len(), ids.len(), "identities collided: {ids:?}");
}

#[test]
fn a_caller_can_find_what_it_asked_for() {
    let mut s = sim();
    let op = s.command(A, Cmd::Work(ms(5)));
    s.run_for(ms(50));

    assert_eq!(s.trace().invoked_at(op), Some(Time::ZERO));
    let (node, _, cmd) =
        s.trace().invocations().find(|(_, o, _)| *o == op).expect("the operation is in the trace");
    assert_eq!(node, A);
    assert_eq!(*cmd, Cmd::Work(ms(5)));
}

// ------------------------------------------------------------------ when it began

/// The instant recorded is when the process handled it, not when the caller asked.
#[test]
fn the_instant_recorded_is_when_it_was_handled() {
    let mut s = sim();
    let op = s.command_at(A, ms(30), Cmd::Work(ms(5)));
    s.run_for(ms(100));

    assert_eq!(s.trace().invoked_at(op), Some(Time::from_millis(30)));
}

/// The reason dispatch is the instant recorded rather than the moment the caller asked. All three
/// are issued at once here; recording *that* would show them overlapping each other completely,
/// and a checker fed such a history could rule out almost nothing.
#[test]
fn operations_issued_together_do_not_appear_to_overlap() {
    let mut s = sim();
    let ops: Vec<_> =
        [10u64, 20, 30].iter().map(|d| s.command_at(A, ms(*d), Cmd::Work(ms(1)))).collect();
    s.run_for(ms(100));

    let began: Vec<_> = ops.iter().map(|o| s.trace().invoked_at(*o)).collect();
    assert_eq!(
        began,
        vec![Some(Time::from_millis(10)), Some(Time::from_millis(20)), Some(Time::from_millis(30))]
    );
}

/// The three questions this change exists to make askable, none of which a test here could ask
/// before: when an operation began, how long it took, and whether two of them overlapped.
#[test]
fn a_test_can_ask_when_an_operation_began_how_long_it_took_and_whether_two_overlapped() {
    let mut s = sim();
    let first = s.command(A, Cmd::Work(ms(50)));
    s.run_for(ms(10));
    let second = s.command(B, Cmd::Work(ms(50)));
    s.run_for(ms(200));

    let a_began = s.trace().invoked_at(first).expect("A began");
    let b_began = s.trace().invoked_at(second).expect("B began");
    let a_ended = done_at(&s, A).expect("A finished");
    let b_ended = done_at(&s, B).expect("B finished");

    // When it began.
    assert_eq!(a_began, Time::ZERO);
    assert_eq!(b_began, Time::from_millis(10));

    // How long it took. The completion is paired to its invocation by the test, which knows this
    // protocol; the trace does not guess, and `design.md` says why.
    assert_eq!(a_ended.saturating_since(a_began), ms(50));

    // Whether the two overlapped.
    assert!(
        a_began < b_ended && b_began < a_ended,
        "[{a_began:?},{a_ended:?}] vs [{b_began:?},{b_ended:?}]"
    );
}

// ------------------------------------------------------------------ when it never began

#[test]
fn an_operation_given_to_a_crashed_process_is_recorded() {
    let mut s = sim();
    s.crash(A);
    let op = s.command(A, Cmd::Work(ms(5)));
    s.run_for(ms(50));

    assert_eq!(s.trace().invoked_at(op), None, "it never began");
    assert_eq!(s.trace().why_not_begun(op), Some(NotBegun::Crashed));
}

/// A stalled process's commands are discarded, not held — and are recorded, which is what was
/// missing. A `Deliver` crosses into a stalled process from outside and waits in a buffer that
/// really exists; a `Cmd` comes from the layer above *on that process*, which is stalled with it.
#[test]
fn an_operation_given_to_a_stalled_process_is_recorded_and_not_handled_on_resume() {
    let mut s = sim();
    s.suspend(A);
    let op = s.command(A, Cmd::Work(ms(5)));
    s.run_for(ms(50));

    assert_eq!(s.trace().why_not_begun(op), Some(NotBegun::Stalled));

    s.resume(A);
    s.run_for(ms(200));
    assert_eq!(s.trace().invoked_at(op), None, "a stall does not deliver it late");
    assert_eq!(done_at(&s, A), None, "and nothing came of it");
}

/// The reasons are told apart, which is what the next roadmap item is built on: a stalled process
/// certainly did not begin the operation, where one lost to a crash may have been half-done by the
/// incarnation that died.
#[test]
fn why_an_operation_did_not_begin_is_distinguishable() {
    let mut s = sim();
    s.crash(A);
    s.suspend(B);
    let crashed = s.command(A, Cmd::Work(ms(1)));
    let stalled = s.command(B, Cmd::Work(ms(1)));
    let absent = s.command(ABSENT, Cmd::Work(ms(1)));
    s.run_for(ms(50));

    assert_eq!(s.trace().why_not_begun(crashed), Some(NotBegun::Crashed));
    assert_eq!(s.trace().why_not_begun(stalled), Some(NotBegun::Stalled));
    assert_eq!(s.trace().why_not_begun(absent), Some(NotBegun::NotAProcess));
    assert_eq!(s.trace().not_begun().count(), 3);
}

/// Asked for and never begun is not the same as never asked for. Before this change the two were
/// indistinguishable, because neither left anything behind.
#[test]
fn asked_for_and_never_begun_differs_from_never_asked_for() {
    let mut asked = sim();
    asked.crash(A);
    asked.command(A, Cmd::Work(ms(1)));
    asked.run_for(ms(50));

    let mut never = sim();
    never.crash(A);
    never.run_for(ms(50));

    assert_eq!(asked.trace().not_begun().count(), 1);
    assert_eq!(never.trace().not_begun().count(), 0);
}

// ------------------------------------------------------------------ nothing else changed

/// What the simulator *does* with a command is untouched; only what it records is new. An earlier
/// draft of this change held commands for a stalled process, which would have made this two.
#[test]
fn recording_an_operation_did_not_change_what_happens_to_it() {
    let mut s = sim();
    s.suspend(A);
    s.command(A, Cmd::Work(ms(1)));
    s.run_for(ms(50));
    s.resume(A);
    s.command(A, Cmd::Work(ms(1)));
    s.run_for(ms(50));

    let handled = s.trace().invocations().count();
    assert_eq!(handled, 1, "the stalled one is gone, exactly as before this change");
}
