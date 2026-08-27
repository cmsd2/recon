//! Verifies the method, not the protocols.
//!
//! The protocol suites assert that guarantees hold. This one asserts that those assertions are
//! worth something: that the faults they claim to run under were actually injected, that they are
//! not passing vacuously, and that a failure can be reproduced from its seed.
//!
//! The checks are written as fallible functions over an extracted summary rather than as
//! assertions, so that each can itself be tested against a case it must reject.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, TimerId};
use recon_protocols::best_effort_broadcast::{BestEffortBroadcast, Cmd, Ind};
use recon_protocols::perfect_link::{self as pl};
use recon_sim::{Config, DropReason, Sim};
use std::collections::{BTreeMap, BTreeSet};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const ALL: [NodeId; 4] = [A, B, C, D];

fn interval() -> Duration {
    Duration::from_millis(10)
}

// ------------------------------------------------------------- what a run shows

/// What a run exposed, extracted from its trace. Plain data, so a check can be exercised
/// against a summary constructed by hand.
#[derive(Debug, Default, Clone)]
struct Observed {
    /// (sender, payload) for every broadcast requested.
    broadcasts: Vec<(NodeId, u32)>,
    /// (deliverer, claimed sender, payload) for every delivery to the layer above.
    delivered: Vec<(NodeId, NodeId, u32)>,
    /// Processes that were crashed at any point.
    crashed: BTreeSet<NodeId>,
    sends: usize,
    losses: usize,
    duplicates: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct Violation(String);

fn v(s: impl Into<String>) -> Violation {
    Violation(s.into())
}

// ------------------------------------------------------------------ the checks

/// No process delivers the same broadcast twice.
fn check_no_duplication(o: &Observed) -> Result<(), Violation> {
    let mut seen: BTreeMap<(NodeId, NodeId, u32), usize> = BTreeMap::new();
    for d in &o.delivered {
        let n = seen.entry(*d).or_default();
        *n += 1;
        if *n > 1 {
            return Err(v(format!("{} delivered {:?} {} times", d.0, (d.1, d.2), n)));
        }
    }
    Ok(())
}

/// Nothing is delivered that was not broadcast, by the process it is attributed to.
fn check_no_creation(o: &Observed) -> Result<(), Violation> {
    for (at, from, msg) in &o.delivered {
        if !o.broadcasts.contains(&(*from, *msg)) {
            return Err(v(format!("{at} delivered {msg} from {from}, never broadcast")));
        }
    }
    Ok(())
}

/// Every process that never crashed delivers every broadcast made by a process that never crashed.
fn check_validity(o: &Observed) -> Result<(), Violation> {
    for (sender, msg) in &o.broadcasts {
        if o.crashed.contains(sender) {
            continue; // best-effort makes no promise for a crashed sender
        }
        for n in ALL {
            if o.crashed.contains(&n) {
                continue;
            }
            if !o.delivered.contains(&(n, *sender, *msg)) {
                return Err(v(format!("{n} never delivered {msg} from correct sender {sender}")));
            }
        }
    }
    Ok(())
}

/// The faults the run was configured for actually happened.
///
/// Without this, a suite that stopped injecting faults would keep passing, and would be
/// asserting its guarantees under conditions it no longer creates.
fn check_faults_occurred(o: &Observed, want_loss: bool, want_dup: bool) -> Result<(), Violation> {
    if want_loss && o.losses == 0 {
        return Err(v(format!("configured for loss but none occurred in {} sends", o.sends)));
    }
    if want_dup && o.duplicates == 0 {
        return Err(v(format!(
            "configured for duplication but none occurred in {} sends",
            o.sends
        )));
    }
    Ok(())
}

/// Something actually happened.
///
/// Every absence-of-violation property above is satisfied trivially by a run that delivers
/// nothing. This is the guard against that.
fn check_not_vacuous(o: &Observed, min_deliveries: usize) -> Result<(), Violation> {
    if o.delivered.len() < min_deliveries {
        return Err(v(format!(
            "only {} deliveries, expected at least {min_deliveries} — the suite may be passing vacuously",
            o.delivered.len()
        )));
    }
    Ok(())
}

fn check_all(o: &Observed, want_loss: bool, want_dup: bool, min: usize) -> Result<(), Violation> {
    check_no_creation(o)?;
    check_no_duplication(o)?;
    check_validity(o)?;
    check_faults_occurred(o, want_loss, want_dup)?;
    check_not_vacuous(o, min)?;
    Ok(())
}

// ------------------------------------------------------- driving the real stack

fn observe(s: &Sim<BestEffortBroadcast<u32>>, broadcasts: &[(NodeId, u32)]) -> Observed {
    Observed {
        broadcasts: broadcasts.to_vec(),
        delivered: s
            .trace()
            .indications()
            .map(|(at, Ind::Deliver { from, msg })| (at, *from, *msg))
            .collect(),
        crashed: BTreeSet::new(),
        sends: s.trace().send_count(),
        losses: s.trace().drops_because(DropReason::Lost),
        duplicates: s.trace().duplicates(),
    }
}

/// A full three-layer run: broadcast over perfect links over stubborn links.
fn full_stack_run(seed: u64) -> Observed {
    let mut s: Sim<BestEffortBroadcast<u32>> = Sim::new(
        Config::default()
            .seed(seed)
            .loss(0.4)
            .duplication(0.3)
            .reorder(0.1)
            .latency(Duration::from_millis(1), Duration::from_millis(15)),
        &ALL,
        |me| BestEffortBroadcast::new(me, ALL, interval()),
    );

    let mut broadcasts = Vec::new();
    for (i, sender) in [A, B, C].into_iter().enumerate() {
        let msg = 100 + i as u32;
        s.command(sender, Cmd::Broadcast(msg));
        broadcasts.push((sender, msg));
    }
    s.run_until(recon_core::Time::from_millis(6000));
    observe(&s, &broadcasts)
}

// ---------------------------------------------------------------------- task 7.1

#[test]
fn the_full_stack_holds_every_property_across_many_seeds() {
    for seed in 0..40u64 {
        let o = full_stack_run(seed);
        if let Err(Violation(why)) = check_all(&o, true, true, 12) {
            panic!("seed {seed}: {why}");
        }
    }
}

#[test]
fn the_full_stack_holds_under_partition_and_healing() {
    for seed in 0..12u64 {
        let mut s: Sim<BestEffortBroadcast<u32>> =
            Sim::new(Config::default().seed(seed).loss(0.2), &ALL, |me| {
                BestEffortBroadcast::new(me, ALL, interval())
            });
        s.partition(&[&[A, B], &[C, D]]);
        s.command(A, Cmd::Broadcast(1));
        s.run_until(recon_core::Time::from_millis(500));
        s.heal();
        s.run_until(recon_core::Time::from_millis(4000));

        let o = observe(&s, &[(A, 1)]);
        if let Err(Violation(why)) = check_all(&o, true, false, 4) {
            panic!("seed {seed}: {why}");
        }
    }
}

// ---------------------------------------------------------------------- task 7.2

#[test]
fn the_fault_check_rejects_a_run_with_no_faults() {
    // If the network silently stopped injecting faults, every guarantee would still hold and
    // the suite would still pass. This is what stops that.
    let quiet = Observed { sends: 500, losses: 0, duplicates: 0, ..Default::default() };
    assert!(check_faults_occurred(&quiet, true, false).is_err(), "loss must be demanded");
    assert!(check_faults_occurred(&quiet, false, true).is_err(), "duplication must be demanded");
    assert!(check_faults_occurred(&quiet, false, false).is_ok(), "and only when configured");
}

#[test]
fn a_loss_free_run_really_would_slip_past_the_other_checks() {
    // Demonstrates that the fault check is load-bearing rather than belt-and-braces: with faults
    // disabled, every property below still passes.
    let mut s: Sim<BestEffortBroadcast<u32>> = Sim::new(Config::default().seed(1), &ALL, |me| {
        BestEffortBroadcast::new(me, ALL, interval())
    });
    s.command(A, Cmd::Broadcast(1));
    s.run_until(recon_core::Time::from_millis(500));
    let o = observe(&s, &[(A, 1)]);

    assert!(check_no_creation(&o).is_ok());
    assert!(check_no_duplication(&o).is_ok());
    assert!(check_validity(&o).is_ok());
    assert_eq!(o.losses, 0);
    assert!(check_faults_occurred(&o, true, false).is_err(), "only the fault check notices");
}

// ---------------------------------------------------------------------- task 7.3

/// Accepts broadcasts and delivers nothing. Satisfies every absence-of-violation property.
struct Silent;

impl Protocol for Silent {
    type Cmd = Cmd<u32>;
    type Ind = Ind<u32>;
    type Msg = pl::Wire<u32>;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, _: Cmd<u32>, _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, _: pl::Wire<u32>, _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}
}

#[test]
fn a_protocol_that_delivers_nothing_passes_every_violation_check() {
    // The failure mode this group exists to catch, demonstrated rather than asserted.
    let mut s: Sim<Silent> = Sim::new(Config::default().seed(1), &ALL, |_| Silent);
    s.command(A, Cmd::Broadcast(1));
    s.run_until(recon_core::Time::from_millis(500));

    let o = Observed { broadcasts: vec![(A, 1)], delivered: Vec::new(), ..Default::default() };
    assert!(check_no_creation(&o).is_ok(), "nothing created, because nothing happened");
    assert!(check_no_duplication(&o).is_ok(), "nothing duplicated, likewise");
    assert_eq!(s.trace().indication_count(), 0);
}

#[test]
fn the_vacuity_guard_rejects_a_protocol_that_delivers_nothing() {
    let o = Observed { broadcasts: vec![(A, 1)], delivered: Vec::new(), ..Default::default() };
    assert!(check_not_vacuous(&o, 4).is_err());
    // And validity catches it too, which is the belt to the vacuity guard's braces.
    assert!(check_validity(&o).is_err());
}

#[test]
fn the_vacuity_guard_accepts_a_run_that_did_something() {
    let o = full_stack_run(0);
    assert!(check_not_vacuous(&o, 12).is_ok(), "a real run must clear the bar comfortably");
}

// ---------------------------------------------------------------------- task 7.4

/// Best-effort broadcast with a deliberate defect: it sometimes omits a process from the
/// fan-out, so validity fails on some seeds and not others.
struct Defective {
    inner: BestEffortBroadcast<u32>,
    skip_probability: f64,
}

impl Protocol for Defective {
    type Cmd = Cmd<u32>;
    type Ind = Ind<u32>;
    type Msg = pl::Wire<u32>;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd<u32>, cx: &mut ProtoCx<'_, Self>) {
        use rand::Rng;
        if cx.rng().random::<f64>() < self.skip_probability {
            return; // drops the broadcast entirely — validity is violated
        }
        self.inner.on_cmd(cmd, cx);
    }
    fn on_msg(&mut self, from: NodeId, msg: pl::Wire<u32>, cx: &mut ProtoCx<'_, Self>) {
        self.inner.on_msg(from, msg, cx);
    }
    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        self.inner.on_timer(id, cx);
    }
}

fn defective_run(seed: u64) -> Result<(), Violation> {
    let mut s: Sim<Defective> = Sim::new(Config::default().seed(seed), &ALL, |me| Defective {
        inner: BestEffortBroadcast::new(me, ALL, interval()),
        skip_probability: 0.35,
    });
    s.command(A, Cmd::Broadcast(7));
    s.run_until(recon_core::Time::from_millis(2000));

    let o = Observed {
        broadcasts: vec![(A, 7)],
        delivered: s
            .trace()
            .indications()
            .map(|(at, Ind::Deliver { from, msg })| (at, *from, *msg))
            .collect(),
        ..Default::default()
    };
    check_validity(&o)
}

#[test]
fn a_failure_is_found_by_seed_and_reproduced_from_it() {
    // The workflow the simulator exists to provide: search, report a seed, replay it.
    let failing = (0..200u64).find(|s| defective_run(*s).is_err());
    let seed = failing.expect("a protocol this broken must fail under some schedule");

    let first = defective_run(seed).expect_err("the seed must fail");
    let again = defective_run(seed).expect_err("and must fail on replay");
    assert_eq!(first, again, "replaying a seed must reproduce the identical failure");

    // A failing seed is a number, not a log file.
    assert!(!first.0.is_empty());
}

#[test]
fn a_passing_seed_keeps_passing() {
    let passing = (0..200u64).find(|s| defective_run(*s).is_ok());
    let seed = passing.expect("some schedules should let the defect through unnoticed");
    assert!(defective_run(seed).is_ok());
    assert!(defective_run(seed).is_ok(), "determinism runs in both directions");
}

#[test]
fn the_correct_stack_fails_under_no_seed() {
    // The same search against the real protocol finds nothing, which is what makes the search
    // above meaningful.
    let failing = (0..60u64).find(|s| {
        let o = full_stack_run(*s);
        check_validity(&o).is_err()
    });
    assert_eq!(failing, None, "the correct stack must survive every schedule searched");
}
