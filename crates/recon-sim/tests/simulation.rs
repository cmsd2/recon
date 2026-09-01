//! Verifies the simulation contract using a deliberately trivial protocol, so that anything
//! observed is the simulator's behaviour and not a protocol's.

use core::time::Duration;
use recon_core::{NodeId, Position, ProtoCx, Protocol, Store, Time, TimerId};
use recon_sim::{Config, DropReason, Sim, TraceEvent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);

/// Sends one message when told to, delivers whatever arrives, and can tick on a timer.
struct Parrot {
    me: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Cmd {
    SendTo(NodeId, u32),
    Tick(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Wire(u32);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Got(NodeId, u32);

impl Protocol for Parrot {
    type Cmd = Cmd;
    type Ind = Got;
    type Msg = Wire;
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: Cmd, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            Cmd::SendTo(to, n) => cx.send(to, Wire(n)),
            Cmd::Tick(d) => {
                cx.set_timer(d);
            }
        }
    }

    fn on_msg(&mut self, from: NodeId, Wire(n): Wire, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(Got(from, n));
    }

    fn on_timer(&mut self, _: TimerId, cx: &mut ProtoCx<'_, Self>) {
        let _ = self.me;
        cx.indicate(Got(self.me, u32::MAX));
    }
}

fn sim(config: Config) -> Sim<Parrot> {
    Sim::new(config, &[A, B, C], |me| Parrot { me })
}

// ------------------------------------------------------- Determinism: tasks 3.1, 3.6

/// Drive a busy run and return its trace rendered comparably.
fn busy_run(seed: u64) -> Vec<String> {
    let mut s = sim(Config::default()
        .seed(seed)
        .loss(0.3)
        .duplication(0.2)
        .reorder(0.2)
        .latency(Duration::from_millis(1), Duration::from_millis(20)));
    for i in 0..40u32 {
        s.command_at(A, Duration::from_millis(i as u64), Cmd::SendTo(B, i));
        s.command_at(B, Duration::from_millis(i as u64), Cmd::SendTo(C, i));
        s.command_at(C, Duration::from_millis(i as u64), Cmd::SendTo(A, i));
    }
    s.run_until(Time::from_millis(500));
    s.trace().events().iter().map(|e| format!("{e:?}")).collect()
}

#[test]
fn the_same_seed_reproduces_the_same_trace() {
    assert_eq!(busy_run(7), busy_run(7));
}

#[test]
fn the_same_seed_reproduces_the_same_trace_across_many_seeds() {
    for seed in 0..8 {
        assert_eq!(busy_run(seed), busy_run(seed), "seed {seed} was not reproducible");
    }
}

#[test]
fn different_seeds_explore_different_schedules() {
    let traces: Vec<_> = (0..8).map(busy_run).collect();
    let all_same = traces.iter().all(|t| *t == traces[0]);
    assert!(!all_same, "differing seeds must be able to produce differing schedules");
    // ...and each remains individually reproducible, which the test above establishes.
}

#[test]
fn simultaneous_events_are_ordered_deterministically() {
    // Three sends scheduled at the same instant must always be processed in the same order.
    let order = |seed: u64| {
        let mut s = sim(Config::default().seed(seed));
        s.command(A, Cmd::SendTo(B, 1));
        s.command(A, Cmd::SendTo(B, 2));
        s.command(A, Cmd::SendTo(B, 3));
        s.run_until(Time::from_millis(100));
        s.trace().sends().map(|(_, _, m)| m.0).collect::<Vec<_>>()
    };
    assert_eq!(order(1), vec![1, 2, 3]);
    assert_eq!(order(1), order(1));
    assert_eq!(order(2), vec![1, 2, 3], "ordering at one instant must not depend on the seed");
}

// ------------------------------------------------------------- Virtual clock: task 3.1

#[test]
fn a_distant_timer_costs_no_real_time() {
    let wall = std::time::Instant::now();
    let mut s = sim(Config::default());
    s.command(A, Cmd::Tick(Duration::from_secs(3600)));
    s.run_until(Time::MAX);
    assert_eq!(s.trace().timer_fires(), 1, "the timer must have fired");
    assert!(
        wall.elapsed() < Duration::from_secs(1),
        "an hour of virtual time must not cost real time"
    );
}

#[test]
fn the_clock_advances_to_scheduled_events() {
    let mut s = sim(Config::default().latency(Duration::from_millis(5), Duration::from_millis(5)));
    s.command(A, Cmd::SendTo(B, 1));
    s.run_until(Time::from_millis(100));
    let delivered = s
        .trace()
        .events()
        .iter()
        .find(|e| matches!(e, TraceEvent::Delivered { .. }))
        .expect("a delivery");
    assert_eq!(delivered.at(), Time::from_millis(5), "delivery must land at send + latency");
}

// --------------------------------------------------------------- Harness: task 3.2

#[test]
fn several_processes_run_in_one_test_process() {
    let mut s = sim(Config::default());
    assert_eq!(s.nodes().collect::<Vec<_>>(), vec![A, B, C]);
    s.command(A, Cmd::SendTo(B, 1));
    s.command(B, Cmd::SendTo(C, 2));
    s.command(C, Cmd::SendTo(A, 3));
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().delivery_count(), 3);
    assert_eq!(s.trace().indication_count(), 3);
}

#[test]
fn a_crashed_process_stops_receiving() {
    let mut s = sim(Config::default());
    s.crash(B);
    s.command(A, Cmd::SendTo(B, 1));
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().delivery_count(), 0);
    assert_eq!(s.trace().drops_because(DropReason::RecipientCrashed), 1);
}

#[test]
fn a_restarted_process_receives_again() {
    let mut s = sim(Config::default());
    s.crash(B);
    s.command(A, Cmd::SendTo(B, 1));
    s.run_until(Time::from_millis(50));
    s.restart(B);
    s.command(A, Cmd::SendTo(B, 2));
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().delivery_count(), 1);
}

// -------------------------------------------------------- Fault injection: task 3.3

#[test]
fn loss_occurs_at_about_the_configured_rate() {
    let mut s = sim(Config::default().seed(3).loss(0.5));
    for i in 0..400u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(1000));
    let sent = s.trace().send_count();
    let lost = s.trace().drops_because(DropReason::Lost);
    assert_eq!(sent, 400);
    let rate = lost as f64 / sent as f64;
    assert!((0.40..0.60).contains(&rate), "loss rate was {rate}, expected about 0.5");
}

#[test]
fn no_loss_is_configurable() {
    let mut s = sim(Config::default().seed(3).loss(0.0));
    for i in 0..100u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(1000));
    assert_eq!(s.trace().drops(), 0);
    assert_eq!(s.trace().delivery_count(), 100);
}

#[test]
fn duplication_delivers_a_message_twice() {
    let mut s = sim(Config::default().seed(5).duplication(1.0));
    s.command(A, Cmd::SendTo(B, 9));
    s.run_until(Time::from_millis(1000));
    assert_eq!(s.trace().duplicates(), 1);
    assert_eq!(s.trace().delivery_count(), 2, "both copies must arrive");
}

#[test]
fn latency_jitter_reorders_messages() {
    let mut s = sim(Config::default()
        .seed(11)
        .latency(Duration::from_millis(1), Duration::from_millis(60)));
    for i in 0..30u32 {
        s.command_at(A, Duration::from_millis(i as u64), Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(2000));
    let order: Vec<u32> = s.trace().deliveries().map(|(_, _, m)| m.0).collect();
    assert_eq!(order.len(), 30);
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(sorted, (0..30).collect::<Vec<_>>(), "every message must still arrive");
    assert_ne!(order, sorted, "jitter must actually reorder deliveries");
}

#[test]
fn the_reorder_knob_forces_extreme_delay() {
    let mut s = sim(Config::default().seed(2).reorder(1.0));
    s.command(A, Cmd::SendTo(B, 1));
    s.run_until(Time::from_millis(1000));
    assert_eq!(s.trace().reorderings(), 1);
    let d = s
        .trace()
        .events()
        .iter()
        .find(|e| matches!(e, TraceEvent::Delivered { .. }))
        .expect("a delivery");
    assert!(d.at() >= Time::from_millis(50), "a reordered message must be pushed well back");
}

// ------------------------------------------------------------- Partitions: task 3.4

#[test]
fn a_partition_prevents_delivery() {
    let mut s = sim(Config::default());
    s.partition(&[&[A, B], &[C]]);
    s.command(A, Cmd::SendTo(C, 1));
    s.command(A, Cmd::SendTo(B, 2));
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().drops_because(DropReason::Partitioned), 1);
    assert_eq!(s.trace().delivery_count(), 1, "within a partition delivery still works");
}

#[test]
fn a_healed_partition_permits_delivery_again() {
    let mut s = sim(Config::default());
    s.partition(&[&[A, B], &[C]]);
    s.command(A, Cmd::SendTo(C, 1));
    s.run_until(Time::from_millis(50));
    assert_eq!(s.trace().delivery_count(), 0);

    s.heal();
    s.command(A, Cmd::SendTo(C, 2));
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().delivery_count(), 1);
}

// ------------------------------------------------------------------ Trace: task 3.5

#[test]
fn the_trace_records_every_kind_of_event() {
    let mut s = sim(Config::default().seed(1).duplication(1.0));
    s.command(A, Cmd::SendTo(B, 1));
    s.command(A, Cmd::Tick(Duration::from_millis(10)));
    s.run_until(Time::from_millis(100));

    let ev = s.trace().events();
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::Sent { .. })));
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::Delivered { .. })));
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::Duplicated { .. })));
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::TimerFired { .. })));
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::Indicated { .. })));
}

#[test]
fn the_trace_is_ordered_by_time() {
    let mut s = sim(Config::default()
        .seed(4)
        .loss(0.2)
        .latency(Duration::from_millis(1), Duration::from_millis(30)));
    for i in 0..30u32 {
        s.command_at(A, Duration::from_millis(i as u64), Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(500));
    let times: Vec<Time> = s.trace().events().iter().map(|e| e.at()).collect();
    let mut sorted = times.clone();
    sorted.sort();
    assert_eq!(times, sorted, "trace entries must be non-decreasing in virtual time");
}

#[test]
fn fault_injection_is_distinguishable_in_the_trace() {
    let mut s = sim(Config::default().seed(6).loss(0.5).duplication(0.5));
    for i in 0..200u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(1000));
    assert!(s.trace().drops_because(DropReason::Lost) > 0, "losses must be visible");
    assert!(s.trace().duplicates() > 0, "duplicates must be visible");
    assert!(s.trace().delivery_count() > 0, "normal deliveries must still occur");
}

#[test]
fn properties_are_assertable_without_touching_protocol_internals() {
    let mut s = sim(Config::default().seed(8));
    s.command(A, Cmd::SendTo(B, 42));
    s.run_until(Time::from_millis(100));
    // No creation: every indication corresponds to something actually sent.
    let sent: Vec<u32> = s.trace().sends().map(|(_, _, m)| m.0).collect();
    for (_, Got(_, n)) in s.trace().indications() {
        assert!(sent.contains(n), "indicated a value that was never sent");
    }
}

// ------------------------------------------------------------ Codec check: task 3.7

#[test]
fn the_default_path_performs_no_encoding() {
    // Nothing to observe directly; the guarantee is that a non-serialisable message type would
    // still run. `Cmd` here is not Serialize, and the run works.
    let mut s = sim(Config::default());
    s.command(A, Cmd::SendTo(B, 1));
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().delivery_count(), 1);
}

#[test]
fn codec_checking_passes_for_a_sound_message_type() {
    let mut s = sim(Config::default());
    s.enable_codec_check();
    s.command(A, Cmd::SendTo(B, 1234));
    s.run_until(Time::from_millis(100));
    assert_eq!(s.trace().delivery_count(), 1);
}

#[test]
fn a_message_type_survives_a_round_trip() {
    let w = Wire(7);
    let back: Wire = recon_sim::codec::round_trip(&w).expect("round trip");
    assert_eq!(back, w);
}

#[test]
fn a_broken_round_trip_is_reported() {
    // A type whose Deserialize disagrees with its Serialize: the defect the mode exists to find.
    #[derive(Debug, PartialEq, Serialize)]
    struct Lossy(u32);
    impl<'de> Deserialize<'de> for Lossy {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let n = u32::deserialize(d)?;
            Ok(Lossy(n.wrapping_add(1))) // wrong on purpose
        }
    }
    let err = recon_sim::codec::round_trip(&Lossy(1)).expect_err("must not round-trip");
    assert!(err.to_string().contains("round trip"), "got: {err}");
}

// ------------------------------------------- Crash loses volatile state: task 3.8

/// Counts what it has seen, so state loss is observable.
struct Counter {
    seen: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CountCmd {
    Bump,
    ArmTimer(Duration),
}

impl Protocol for Counter {
    type Cmd = CountCmd;
    type Ind = u32;
    type Msg = ();
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, cmd: CountCmd, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            CountCmd::Bump => {
                self.seen += 1;
                cx.indicate(self.seen);
            }
            CountCmd::ArmTimer(d) => {
                cx.set_timer(d);
            }
        }
    }
    fn on_msg(&mut self, _: NodeId, _: (), _: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(u32::MAX);
    }
}

fn counters() -> Sim<Counter> {
    Sim::new(Config::default(), &[A, B], |_| Counter { seen: 0 })
}

#[test]
fn a_crash_loses_volatile_state() {
    let mut s = counters();
    s.command(A, CountCmd::Bump);
    s.command(A, CountCmd::Bump);
    s.run_until(Time::from_millis(10));
    assert_eq!(s.protocol(A).unwrap().seen, 2);

    s.crash(A);
    s.restart(A);
    assert_eq!(s.protocol(A).unwrap().seen, 0, "a crash must not preserve state");

    s.command(A, CountCmd::Bump);
    s.run_until(Time::from_millis(20));
    assert_eq!(s.protocol(A).unwrap().seen, 1, "the restarted process counts from scratch");
}

#[test]
fn a_suspension_preserves_state() {
    let mut s = counters();
    s.command(A, CountCmd::Bump);
    s.command(A, CountCmd::Bump);
    s.run_until(Time::from_millis(10));

    s.suspend(A);
    assert!(s.is_stopped(A));
    s.resume(A);
    assert_eq!(s.protocol(A).unwrap().seen, 2, "a suspension is a pause, not a failure");
}

#[test]
fn a_suspended_process_handles_nothing_while_stopped() {
    let mut s = counters();
    s.suspend(A);
    s.command(A, CountCmd::Bump);
    s.run_until(Time::from_millis(10));
    assert_eq!(s.protocol(A).unwrap().seen, 0);

    s.resume(A);
    s.command(A, CountCmd::Bump);
    s.run_until(Time::from_millis(20));
    assert_eq!(s.protocol(A).unwrap().seen, 1);
}

#[test]
fn a_crash_discards_pending_timers() {
    // Timers are volatile state and must not outlive the process that set them.
    let mut s = counters();
    s.command(A, CountCmd::ArmTimer(Duration::from_millis(100)));
    s.run_until(Time::from_millis(10));

    s.crash(A);
    s.restart(A);
    s.run_until(Time::from_millis(500));
    assert_eq!(s.trace().timer_fires(), 0, "a crashed process's timer must not fire");
}

#[test]
fn a_suspension_keeps_pending_timers() {
    let mut s = counters();
    s.command(A, CountCmd::ArmTimer(Duration::from_millis(100)));
    s.run_until(Time::from_millis(10));

    s.suspend(A);
    s.run_until(Time::from_millis(50));
    s.resume(A);
    s.run_until(Time::from_millis(500));
    assert_eq!(s.trace().timer_fires(), 1, "a suspension keeps what a crash would lose");
}

#[test]
fn a_crash_is_distinguishable_from_a_suspension_in_the_trace() {
    let mut s = counters();
    s.crash(A);
    s.suspend(B);
    let ev = s.trace().events();
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::Crashed { node, .. } if *node == A)));
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::Suspended { node, .. } if *node == B)));
}

// ------------------------------------------ Synchronous mode: tasks 1.1 to 1.4

fn sync_sim(bound: Duration, seed: u64) -> Sim<Parrot> {
    Sim::new(Config::default().seed(seed).synchronous(bound), &[A, B, C], |me| Parrot { me })
}

/// Pair each Sent with its Delivered by payload, and return the observed delays.
fn delays(s: &Sim<Parrot>) -> Vec<Duration> {
    let mut sent: BTreeMap<u32, Time> = BTreeMap::new();
    let mut out = Vec::new();
    for e in s.trace().events() {
        match e {
            TraceEvent::Sent { at, msg: Wire(n), .. } => {
                sent.insert(*n, *at);
            }
            TraceEvent::Delivered { at, msg: Wire(n), .. } => {
                if let Some(t0) = sent.get(n) {
                    out.push(at.saturating_since(*t0));
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn the_delivery_bound_is_readable_from_the_run() {
    // A protocol depending on the bound must be able to be configured from it, not from a guess.
    let bound = Duration::from_millis(25);
    let s = sync_sim(bound, 1);
    assert_eq!(s.delivery_bound(), Some(bound));

    let async_s = sim(Config::default());
    assert_eq!(async_s.delivery_bound(), None, "the default makes no timing promise");
}

#[test]
fn every_delivery_is_within_the_bound() {
    let bound = Duration::from_millis(20);
    for seed in 0..8u64 {
        let mut s = sync_sim(bound, seed);
        for i in 0..40u32 {
            s.command_at(A, Duration::from_millis(i as u64), Cmd::SendTo(B, i));
        }
        s.run_until(Time::from_millis(2000));
        let d = delays(&s);
        assert_eq!(d.len(), 40, "seed {seed}: every message must be delivered");
        for delay in d {
            assert!(delay <= bound, "seed {seed}: delivery took {delay:?}, bound is {bound:?}");
        }
    }
}

#[test]
fn nothing_is_lost_or_duplicated_in_synchronous_mode() {
    let mut s = sync_sim(Duration::from_millis(15), 2);
    for i in 0..60u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(1000));
    assert_eq!(s.trace().drops(), 0);
    assert_eq!(s.trace().duplicates(), 0);
    assert_eq!(s.trace().delivery_count(), 60);
}

#[test]
fn the_fault_knobs_cannot_subvert_the_synchronous_promise() {
    // Enforcement is at delivery time, so builder order cannot reintroduce loss.
    let bound = Duration::from_millis(10);
    let mut s: Sim<Parrot> = Sim::new(
        Config::default().seed(3).synchronous(bound).loss(0.9).duplication(0.9).reorder(0.9),
        &[A, B, C],
        |me| Parrot { me },
    );
    for i in 0..50u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(500));
    assert_eq!(s.trace().drops(), 0, "loss set after synchronous must not take effect");
    assert_eq!(s.trace().duplicates(), 0);
    assert_eq!(s.trace().reorderings(), 0);
    for delay in delays(&s) {
        assert!(delay <= bound);
    }
}

#[test]
fn crashes_still_stop_delivery_in_synchronous_mode() {
    // The mode constrains timing, not failure — a detector with nothing to detect is untestable.
    let mut s = sync_sim(Duration::from_millis(10), 4);
    s.crash(B);
    s.command(A, Cmd::SendTo(B, 1));
    s.run_until(Time::from_millis(200));
    assert_eq!(s.trace().delivery_count(), 0);
    assert_eq!(s.trace().drops_because(DropReason::RecipientCrashed), 1);
}

#[test]
fn partitions_still_stop_delivery_in_synchronous_mode() {
    let mut s = sync_sim(Duration::from_millis(10), 5);
    s.partition(&[&[A, B], &[C]]);
    s.command(A, Cmd::SendTo(C, 1));
    s.command(A, Cmd::SendTo(B, 2));
    s.run_until(Time::from_millis(200));
    assert_eq!(s.trace().drops_because(DropReason::Partitioned), 1);
    assert_eq!(s.trace().delivery_count(), 1);
}

#[test]
fn the_default_remains_asynchronous() {
    // The existing behaviour must be untouched: loss, duplication and jitter as before.
    let mut s = sim(Config::default()
        .seed(6)
        .loss(0.5)
        .duplication(0.5)
        .latency(Duration::from_millis(1), Duration::from_millis(40)));
    for i in 0..200u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(2000));
    assert!(s.trace().drops() > 0, "the default must still lose");
    assert!(s.trace().duplicates() > 0, "and still duplicate");
    assert!(delays(&s).iter().any(|d| *d > Duration::from_millis(20)), "and still jitter");
}

#[test]
fn a_timer_due_during_a_suspension_fires_on_resume() {
    // A suspension preserves state, and a pending timer is state. Dropping it leaves the process
    // alive but permanently inert — which is how a heartbeat protocol silently dies.
    let mut s = counters();
    s.command(A, CountCmd::ArmTimer(Duration::from_millis(50)));
    s.run_for(Duration::from_millis(10));

    s.suspend(A);
    s.run_for(Duration::from_millis(100)); // the timer comes due here, while suspended
    assert_eq!(s.trace().timer_fires(), 0, "nothing fires while suspended");

    s.resume(A);
    s.run_for(Duration::from_millis(10));
    assert_eq!(s.trace().timer_fires(), 1, "and it fires once the process is back");
}

#[test]
fn a_delivery_due_during_a_suspension_arrives_on_resume() {
    // The invariant the model imposes on every layer, imposed on the model: a message is
    // delivered, or the scope carrying it ends. Dropping it while the process is merely stalled
    // and its session is still up would be neither.
    let mut s = session_sim(21);
    s.run_for(Duration::from_millis(50)); // the session comes up
    assert!(s.has_session(A, B));

    s.suspend(B);
    s.command(A, Cmd::SendTo(B, 7));
    s.run_for(Duration::from_millis(200));
    assert_eq!(arrivals(&s, B), Vec::<u32>::new(), "nothing arrives while stalled");
    assert_eq!(s.trace().drops(), 0, "and nothing is dropped either");
    assert_eq!(s.trace().session_ends(), 0, "the session never ended, so nothing may be lost");

    s.resume(B);
    s.run_for(Duration::from_millis(50));
    assert_eq!(arrivals(&s, B), vec![7], "held, and handed over once the stall is past");
}

#[test]
fn a_suspension_holds_deliveries_outside_a_session_too() {
    // A stall is not loss. Loss is what the `loss` knob is for, and it is drawn per message and
    // recorded as a drop; a process being descheduled is neither.
    let mut s: Sim<Parrot> = Sim::new(Config::default().seed(23), &[A, B, C], |me| Parrot { me });
    s.suspend(A);
    for i in 0..5u32 {
        s.command(B, Cmd::SendTo(A, i));
    }
    s.run_for(Duration::from_millis(200));
    assert_eq!(arrivals(&s, A), Vec::<u32>::new(), "nothing arrives while stalled");
    assert_eq!(s.trace().drops(), 0, "and nothing is dropped on its account");

    s.resume(A);
    s.run_for(Duration::from_millis(50));
    let mut got = arrivals(&s, A);
    got.sort();
    assert_eq!(got, vec![0, 1, 2, 3, 4], "all five, once it is back");
}

#[test]
fn a_crash_discards_deliveries_held_during_a_suspension() {
    // Held for a stall, lost to a failure — and in the session model the crash ends the session,
    // so the loss is announced rather than silent.
    let mut s = session_sim(22);
    s.run_for(Duration::from_millis(50));
    s.suspend(B);
    s.command(A, Cmd::SendTo(B, 7));
    s.run_for(Duration::from_millis(200));

    s.crash(B);
    assert!(!s.has_session(A, B), "the crash ends the session that was carrying it");
    s.restart(B);
    s.run_for(Duration::from_millis(200));
    assert!(!arrivals(&s, B).contains(&7), "a crash loses what a suspension was holding");
}

#[test]
fn resuming_is_not_restarting() {
    // A resumed process never lost anything, so it takes no startup branch: replaying `on_init`
    // or `on_recovery` over intact state would tell it it restarted when it did not.
    let mut s = counters();
    s.command(A, CountCmd::Bump);
    s.run_for(Duration::from_millis(10));
    let before = s.trace().len();

    s.suspend(A);
    s.resume(A);

    let after = &s.trace().events()[before..];
    assert!(after.iter().any(|e| matches!(e, TraceEvent::Suspended { node, .. } if *node == A)));
    assert!(after.iter().any(|e| matches!(e, TraceEvent::Resumed { node, .. } if *node == A)));
    assert!(
        !after
            .iter()
            .any(|e| matches!(e, TraceEvent::Recovered { .. } | TraceEvent::Restarted { .. })),
        "no startup branch: {after:?}"
    );
}

#[test]
#[should_panic(expected = "not crashed")]
fn restarting_a_suspended_process_is_a_mistake_that_says_so() {
    let mut s = counters();
    s.suspend(A);
    s.restart(A);
}

#[test]
#[should_panic(expected = "not suspended")]
fn resuming_a_crashed_process_is_a_mistake_that_says_so() {
    let mut s = counters();
    s.crash(A);
    s.resume(A);
}

#[test]
fn a_crash_discards_timers_deferred_during_a_suspension() {
    // Suspended then crashed: the crash still takes the volatile state, held timers included.
    let mut s = counters();
    s.command(A, CountCmd::ArmTimer(Duration::from_millis(50)));
    s.run_for(Duration::from_millis(10));

    s.suspend(A);
    s.run_for(Duration::from_millis(100));
    s.crash(A);
    s.restart(A);
    s.run_for(Duration::from_millis(500));
    assert_eq!(s.trace().timer_fires(), 0, "a crash loses what a suspension was holding");
}

// ---------------------------------------------- The session model: group 2

fn session_sim(seed: u64) -> Sim<Parrot> {
    Sim::new(
        Config::default()
            .seed(seed)
            .sessions()
            .latency(Duration::from_millis(1), Duration::from_millis(30)),
        &[A, B, C],
        |me| Parrot { me },
    )
}

/// Payloads delivered to `to`, in delivery order.
fn arrivals(s: &Sim<Parrot>, to: NodeId) -> Vec<u32> {
    s.trace()
        .events()
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Delivered { to: t, msg: Wire(n), .. } if *t == to => Some(*n),
            _ => None,
        })
        .collect()
}

#[test]
fn a_session_delivers_reliably_and_in_order() {
    for seed in 0..10u64 {
        let mut s = session_sim(seed);
        for i in 0..50u32 {
            s.command(A, Cmd::SendTo(B, i));
        }
        s.run_until(Time::from_millis(3000));
        assert_eq!(
            arrivals(&s, B),
            (0..50).collect::<Vec<_>>(),
            "seed {seed}: a session must deliver everything, in order, exactly once"
        );
        assert_eq!(s.trace().drops(), 0);
        assert_eq!(s.trace().duplicates(), 0);
    }
}

#[test]
fn a_session_has_one_epoch_shared_by_both_ends() {
    let mut s = session_sim(1);
    s.command(A, Cmd::SendTo(B, 1));
    s.run_for(Duration::from_millis(50));
    assert_eq!(s.session_epoch(A, B), Some(1));
    assert_eq!(s.session_epoch(B, A), Some(1), "one session per pair, not per direction");
}

/// Break a session with traffic in flight, and report what survived.
fn break_with_traffic(seed: u64) -> (usize, Vec<u32>) {
    let mut s = session_sim(seed);
    for i in 0..40u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.step_now(); // the commands run; the messages are in flight
    s.break_session(A, B);
    s.run_until(Time::from_millis(2000));
    assert_eq!(s.trace().session_ends(), 1);
    (s.trace().suffix_losses(), arrivals(&s, B))
}

#[test]
fn nothing_is_delivered_on_a_session_after_it_has_ended() {
    // A scope boundary that arrivals can trail is not a boundary. The wire carries no epoch, so a
    // straggler from the old session is indistinguishable from new traffic — and the layer above
    // resends on `Established`, so it would be reading the answer to a question it has not asked
    // yet. A real transport hands over what it has and *then* surfaces the close.
    for seed in 0..20u64 {
        let mut s = session_sim(seed);
        for i in 0..40u32 {
            s.command(A, Cmd::SendTo(B, i));
        }
        s.step_now(); // the commands run; the messages are in flight
        s.break_session(A, B);
        s.run_until(Time::from_millis(2000));

        let ended = s
            .trace()
            .events()
            .iter()
            .find_map(|e| match e {
                TraceEvent::SessionEnded { at, a, b, .. } if (*a, *b) == (A, B) => Some(*at),
                _ => None,
            })
            .expect("the break is recorded");
        let reopened = s.trace().events().iter().find_map(|e| match e {
            TraceEvent::SessionOpened { at, a, b, epoch } if (*a, *b) == (A, B) && *epoch > 1 => {
                Some(*at)
            }
            _ => None,
        });
        assert!(reopened.is_none_or(|t| t > ended), "seed {seed}: nor may it reopen that instant");

        let trailing = s.trace().events().iter().filter(|e| {
            matches!(e, TraceEvent::Delivered { at, from, to, .. }
                if pair_is(*from, *to) && *at > ended)
        });
        assert_eq!(trailing.count(), 0, "seed {seed}: no arrival may trail the ending");
    }
}

fn pair_is(from: NodeId, to: NodeId) -> bool {
    (from == A && to == B) || (from == B && to == A)
}

#[test]
fn what_survives_a_break_is_a_prefix() {
    // Whatever the cut, the session was FIFO up to it — so the survivors are a prefix, never a
    // gap. This holds for every seed, including those where nothing is lost.
    for seed in 0..20u64 {
        let (lost, got) = break_with_traffic(seed);
        assert_eq!(
            got,
            (0..got.len() as u32).collect::<Vec<_>>(),
            "seed {seed}: survivors must be a prefix, saw {got:?}"
        );
        assert_eq!(got.len() + lost, 40, "seed {seed}: every message either arrived or was lost");
    }
}

#[test]
fn a_break_can_discard_what_was_in_flight() {
    // Not every seed loses something — "nothing" is a legitimate suffix — so find one that does.
    let losing = (0..40u64).find(|s| break_with_traffic(*s).0 > 0);
    let seed = losing.expect("some seed must lose in-flight traffic");
    let (lost, got) = break_with_traffic(seed);
    assert!(lost > 0);
    assert!(got.len() < 40, "seed {seed}: not everything can have arrived");
}

#[test]
fn the_lost_suffix_is_genuinely_unknown() {
    // A model that always dropped everything in flight, or always nothing, would pass a loose
    // test while modelling nothing at all.
    let mut sizes = std::collections::BTreeSet::new();
    for seed in 0..60u64 {
        let mut s = session_sim(seed);
        for i in 0..12u32 {
            s.command(A, Cmd::SendTo(B, i));
        }
        s.step_now(); // the commands run; the messages are in flight
        s.break_session(A, B);
        s.run_until(Time::from_millis(2000));
        sizes.insert(s.trace().suffix_losses());
    }
    assert!(sizes.len() > 2, "the amount lost must vary across seeds, saw {sizes:?}");
    assert!(sizes.contains(&0), "sometimes nothing in flight is lost, saw {sizes:?}");
    assert!(sizes.iter().any(|n| *n >= 10), "and sometimes nearly all of it, saw {sizes:?}");
}

#[test]
fn a_new_session_opens_at_a_higher_epoch() {
    let mut s = session_sim(3);
    s.command(A, Cmd::SendTo(B, 1));
    s.run_for(Duration::from_millis(50));
    let first = s.session_epoch(A, B).expect("a session");

    s.break_session(A, B);
    assert!(!s.has_session(A, B));
    s.command(A, Cmd::SendTo(B, 2));
    s.run_for(Duration::from_millis(50));

    let second = s.session_epoch(A, B).expect("a new session");
    assert!(second > first, "{second} must exceed {first}");
}

#[test]
fn ordering_restarts_with_the_new_session() {
    let mut s = session_sim(4);
    for i in 0..10u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.step_now(); // the commands run; the messages are in flight
    s.break_session(A, B);
    s.run_for(Duration::from_millis(200));
    let before = arrivals(&s, B).len();

    for i in 100..110u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.run_until(Time::from_millis(3000));

    let after: Vec<u32> = arrivals(&s, B).into_iter().skip(before).collect();
    assert_eq!(after, (100..110).collect::<Vec<_>>(), "the new session orders its own traffic");
}

#[test]
fn a_partition_ends_the_sessions_it_severs_and_no_others() {
    let mut s = session_sim(5);
    s.run_for(Duration::from_millis(50));
    assert!(s.has_session(A, C) && s.has_session(B, C) && s.has_session(A, B));

    s.partition(&[&[A, B], &[C]]);
    assert!(!s.has_session(A, C), "a severed pair has no session");
    assert!(!s.has_session(B, C), "nor does the other severed pair");
    assert!(s.has_session(A, B), "but the intact pair keeps its session");
    assert_eq!(s.trace().session_ends(), 2, "exactly the two crossing the split");
}

#[test]
fn a_crash_ends_every_session_of_the_crashed_process() {
    let mut s = session_sim(6);
    s.command(A, Cmd::SendTo(B, 1));
    s.command(A, Cmd::SendTo(C, 1));
    s.run_for(Duration::from_millis(50));
    assert!(s.has_session(A, B) && s.has_session(A, C));

    s.crash(A);
    assert!(!s.has_session(A, B) && !s.has_session(A, C));
    assert_eq!(s.trace().session_ends(), 2);
}

#[test]
fn the_trace_records_session_events_distinguishably() {
    let mut s = session_sim(7);
    for i in 0..20u32 {
        s.command(A, Cmd::SendTo(B, i));
    }
    s.step_now(); // the commands run; the messages are in flight
    s.break_session(A, B);
    s.run_until(Time::from_millis(1000));

    let ev = s.trace().events();
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::SessionOpened { .. })));
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::SessionEnded { .. })));
    assert!(ev.iter().any(|e| matches!(e, TraceEvent::SuffixLost { .. })));
    // And a property is assertable over them without touching protocol state: A and B's session
    // opened at 1, was broken, and opened again at 2.
    let ab: Vec<u64> = s
        .trace()
        .session_epochs()
        .filter(|(a, b, _)| (*a, *b) == (A, B))
        .map(|(_, _, e)| e)
        .collect();
    assert_eq!(ab, vec![1, 2], "opened, broken, opened again at a higher epoch");
}

#[test]
fn session_runs_are_deterministic() {
    // The delivery queue is the source of determinism, and per-pair ordering changed it.
    let run = |seed: u64| {
        let mut s = session_sim(seed);
        for i in 0..30u32 {
            s.command_at(A, Duration::from_millis(i as u64), Cmd::SendTo(B, i));
            s.command_at(B, Duration::from_millis(i as u64), Cmd::SendTo(C, i));
        }
        s.run_for(Duration::from_millis(200));
        s.break_session(A, B);
        s.run_until(Time::from_millis(3000));
        s.trace().events().iter().map(|e| format!("{e:?}")).collect::<Vec<_>>()
    };
    for seed in 0..8u64 {
        assert_eq!(run(seed), run(seed), "seed {seed} was not reproducible");
    }
    let traces: Vec<_> = (0..8).map(run).collect();
    assert!(!traces.iter().all(|t| *t == traces[0]), "differing seeds must differ somewhere");
}

// ------------------- A link that reconnects on its own: group 1

#[test]
fn a_healed_partition_reconnects_with_nothing_sent() {
    // The link keeps trying on its own, so reconnection does not depend on the layers above
    // happening to transmit — which is a state neither end controls.
    let mut s = session_sim(20);
    s.run_for(Duration::from_millis(50));
    assert!(s.has_session(A, B));
    let first = s.session_epoch(A, B).unwrap();

    s.partition(&[&[A], &[B, C]]);
    assert!(!s.has_session(A, B));
    s.run_for(Duration::from_millis(100));
    assert!(!s.has_session(A, B), "still severed");

    s.heal();
    s.run_for(Duration::from_millis(100)); // nothing is sent by anyone
    assert!(s.has_session(A, B), "the link reconnected without being asked");
    assert!(s.session_epoch(A, B).unwrap() > first);
}

#[test]
fn a_restarted_process_reconnects_with_nothing_sent() {
    let mut s = session_sim(21);
    s.run_for(Duration::from_millis(50));
    assert!(s.has_session(A, B));

    s.crash(B);
    assert!(!s.has_session(A, B));
    s.restart(B);
    s.run_for(Duration::from_millis(100));
    assert!(s.has_session(A, B), "reconnected after restart, unprompted");
}

#[test]
fn an_unreachable_peer_is_retried_not_abandoned() {
    let mut s = session_sim(22);
    s.partition(&[&[A], &[B, C]]);
    s.run_for(Duration::from_millis(500)); // many retry intervals pass
    assert!(!s.has_session(A, B));

    s.heal();
    s.run_for(Duration::from_millis(50));
    assert!(s.has_session(A, B), "retrying resumed as soon as it became possible");
}

#[test]
fn sessions_open_without_anything_being_sent() {
    let mut s = session_sim(23);
    assert!(!s.has_session(A, B), "nothing has run yet");
    s.run_for(Duration::from_millis(50));
    for (x, y) in [(A, B), (A, C), (B, C)] {
        assert!(s.has_session(x, y), "{x}-{y} should be up without any traffic");
    }
}

// ================================================================ stable storage
//
// A process that writes things down and can be asked to write and send in one breath. A write is
// durable when it returns, so the only way to observe a write that did not land is to kill the
// process inside one.

/// What survives a crash: a total, and every peer this process promised it to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ledger {
    total: u32,
    told: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LedgerCmd {
    /// Add to the total and write it down. Nothing is sent.
    Record(u32),
    /// Add to the total, write it down, **and** tell `peer`.
    RecordAndTell(u32, NodeId),
    /// Send without writing anything.
    TellOnly(NodeId),
}

#[derive(Debug, Default)]
struct Keeper {
    total: u32,
    told: Vec<NodeId>,
    /// The appended amounts, read back on recovery.
    replayed: Vec<u32>,
    /// How many events this incarnation had handled by the time recovery returned.
    saw_before_recovery: Option<u32>,
    /// Volatile on purpose: it must be gone after a crash, whatever storage kept.
    events_handled: u32,
}

impl Protocol for Keeper {
    type Cmd = LedgerCmd;
    type Ind = Ledger;
    type Msg = u32;
    type Scope = core::convert::Infallible;
    type Meta = Ledger;
    type Entry = u32;

    fn on_cmd(&mut self, cmd: LedgerCmd, cx: &mut ProtoCx<'_, Self>) {
        self.events_handled += 1;
        match cmd {
            LedgerCmd::Record(n) => {
                self.total += n;
                cx.storage().append(n);
                cx.storage().set(Ledger { total: self.total, told: self.told.clone() });
            }
            LedgerCmd::RecordAndTell(n, peer) => {
                self.total += n;
                self.told.push(peer);
                cx.storage().append(n);
                cx.storage().set(Ledger { total: self.total, told: self.told.clone() });
                cx.send(peer, self.total);
            }
            LedgerCmd::TellOnly(peer) => cx.send(peer, self.total),
        }
    }

    fn on_msg(&mut self, _: NodeId, msg: u32, cx: &mut ProtoCx<'_, Self>) {
        self.events_handled += 1;
        cx.indicate(Ledger { total: msg, told: Vec::new() });
    }

    fn on_timer(&mut self, _: TimerId, _: &mut ProtoCx<'_, Self>) {}

    fn on_recovery(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.saw_before_recovery = Some(self.events_handled);
        self.replayed = cx.storage().read_from(Position::START).into_iter().copied().collect();
        let d = cx.storage().get().cloned().unwrap_or(Ledger { total: 0, told: Vec::new() });
        self.total = d.total;
        self.told = d.told.clone();
        cx.indicate(d);
    }
}

fn keepers(seed: u64) -> Sim<Keeper> {
    Sim::new(Config::default().seed(seed), &[A, B], |_| Keeper::default())
}

#[test]
fn what_was_written_is_retrieved_and_what_was_in_memory_is_not() {
    let mut s = keepers(1);
    s.command(A, LedgerCmd::Record(5));
    s.run_for(Duration::from_millis(50));
    assert_eq!(s.protocol(A).unwrap().total, 5);
    assert_eq!(s.protocol(A).unwrap().events_handled, 1);

    s.crash(A);
    s.restart(A);

    let p = s.protocol(A).unwrap();
    assert_eq!(p.total, 5, "the durable total came back");
    assert_eq!(p.events_handled, 0, "the volatile counter did not");
}

#[test]
fn the_appended_sequence_is_read_back_in_order() {
    let mut s = keepers(2);
    for n in [3u32, 1, 4] {
        s.command(A, LedgerCmd::Record(n));
    }
    s.run_for(Duration::from_millis(50));

    s.crash(A);
    s.restart(A);

    let p = s.protocol(A).unwrap();
    assert_eq!(p.replayed, vec![3, 1, 4], "in the order appended, not sorted or deduplicated");
    assert_eq!(p.total, 8);
    assert_eq!(s.trace().appends(), 3);
    assert_eq!(s.trace().metadata_writes(), 3);
}

#[test]
fn read_from_a_position_skips_what_precedes_it() {
    let mut s = keepers(3);
    for n in [1u32, 2, 3] {
        s.command(A, LedgerCmd::Record(n));
    }
    s.run_for(Duration::from_millis(50));

    let store = s.storage(A).expect("A has written something");
    assert_eq!(store.read_from(Position::START), vec![&1, &2, &3]);
    assert_eq!(store.read_from(Position(1)), vec![&2, &3]);
    assert_eq!(store.end(), Position(3), "one past the last entry");
    assert!(store.read_from(store.end()).is_empty());
}

#[test]
fn a_process_that_never_wrote_recovers_nothing() {
    let mut s = keepers(4);
    s.command(A, LedgerCmd::TellOnly(B));
    s.run_for(Duration::from_millis(50));

    s.crash(A);
    s.restart(A);

    assert_eq!(s.trace().recoveries_with_state(), 0, "there was nothing to recover");
    assert_eq!(s.protocol(A).unwrap().total, 0, "it started as if for the first time");
}

#[test]
fn a_write_is_durable_when_it_returns() {
    // No seed can lose it: once the handler that wrote it has returned, a crash cannot take it.
    for seed in 0..20u64 {
        let mut s = keepers(seed);
        s.command(A, LedgerCmd::Record(7));
        s.run_for(Duration::from_millis(50));
        s.crash(A);
        s.restart(A);
        assert_eq!(s.protocol(A).unwrap().total, 7, "seed {seed}");
        assert_eq!(s.trace().deaths_in_writes(), 0, "seed {seed}: nothing died mid-write");
    }
}

#[test]
fn a_write_the_process_died_inside_may_or_may_not_have_landed() {
    // The one fault a durable-on-return interface admits, and the reason recovery must read rather
    // than assume. Across seeds both outcomes occur, and the recovering process cannot tell which.
    let outcomes: Vec<bool> = (0..40u64)
        .map(|seed| {
            let mut s = keepers(seed);
            s.crash_on_next_write(A);
            s.command(A, LedgerCmd::Record(7));
            s.run_for(Duration::from_millis(50));
            s.restart(A);
            // The append may have landed; the metadata write after it never ran.
            s.storage(A).map(|st| !st.is_empty()).unwrap_or(false)
        })
        .collect();

    assert!(outcomes.iter().any(|kept| *kept), "some runs must keep the write");
    assert!(outcomes.iter().any(|kept| !*kept), "and some must lose it");
}

#[test]
fn a_partially_written_value_is_never_retrieved() {
    // The total and the list of who was told move together. A process killed mid-write retrieves
    // some whole value it once wrote, never a mixture of one it wrote and one it did not.
    for seed in 0..40u64 {
        let mut s = keepers(seed);
        s.command(A, LedgerCmd::RecordAndTell(9, B));
        s.run_for(Duration::from_millis(20));

        s.crash_on_next_write(A);
        s.command(A, LedgerCmd::RecordAndTell(1, A));
        s.run_for(Duration::from_millis(20));
        s.restart(A);

        let p = s.protocol(A).unwrap();
        assert_eq!(
            (p.total, p.told.as_slice()),
            (9, &[B][..]),
            "seed {seed}: the earlier whole value, not a mixture: {p:?}"
        );
    }
}

#[test]
fn nothing_decided_on_a_write_that_killed_the_process_escapes_it() {
    // The send follows the write in the same handler. If the process dies in the write, the send
    // must not have happened — otherwise a promise outlives the record it was made from.
    for seed in 0..40u64 {
        let mut s = keepers(seed);
        s.crash_on_next_write(A);
        s.command(A, LedgerCmd::RecordAndTell(4, B));
        s.run_for(Duration::from_millis(100));

        assert_eq!(s.trace().deaths_in_writes(), 1, "seed {seed}: the process died in the write");
        assert_eq!(
            s.trace().deliveries().filter(|(_, to, _)| *to == B).count(),
            0,
            "seed {seed}: and told nobody"
        );
    }
}

#[test]
fn a_send_after_a_write_that_returned_does_go_out() {
    // The contrast: without the fault, writing first costs the send nothing.
    let mut s = keepers(5);
    s.command(A, LedgerCmd::RecordAndTell(3, B));
    s.run_for(Duration::from_millis(50));
    assert_eq!(s.trace().deliveries().filter(|(_, to, _)| *to == B).count(), 1);
    assert_eq!(s.trace().deaths_in_writes(), 0);
}

#[test]
fn a_send_with_no_write_before_it_costs_nothing() {
    let mut s = keepers(6);
    s.command(A, LedgerCmd::TellOnly(B));
    s.run_for(Duration::from_millis(50));
    assert_eq!(s.trace().deliveries().filter(|(_, to, _)| *to == B).count(), 1);
    assert_eq!(s.trace().writes(), 0, "and no write was involved");
}

#[test]
fn durability_is_assertable_from_the_trace_alone() {
    // Without reading any protocol state: the write is recorded before the message is delivered.
    let mut s = keepers(7);
    s.command(A, LedgerCmd::RecordAndTell(2, B));
    s.run_for(Duration::from_millis(50));

    let wrote_at = s
        .trace()
        .events()
        .iter()
        .find_map(|e| match e {
            TraceEvent::Wrote { at, .. } => Some(*at),
            _ => None,
        })
        .expect("the write is in the trace");
    let delivered_at = s
        .trace()
        .events()
        .iter()
        .find_map(|e| match e {
            TraceEvent::Delivered { at, to, .. } if *to == B => Some(*at),
            _ => None,
        })
        .expect("the delivery is in the trace");

    assert!(wrote_at <= delivered_at, "durable at {wrote_at:?}, delivered at {delivered_at:?}");
    assert_eq!(s.trace().writes(), 2, "one append and one metadata write");
}

#[test]
fn nothing_is_dispatched_between_a_restart_and_recovery_returning() {
    // A message already in flight when the process comes back. It must wait: a protocol that has
    // not finished reading what survived would otherwise handle it against state it has not loaded.
    let mut s = keepers(8);
    s.command(A, LedgerCmd::Record(5));
    s.run_for(Duration::from_millis(10));

    s.command(B, LedgerCmd::TellOnly(A));
    s.step_now(); // B has sent; the message has not arrived
    assert_eq!(s.protocol(A).unwrap().events_handled, 1);

    s.crash(A);
    s.restart(A);
    assert_eq!(
        s.protocol(A).unwrap().saw_before_recovery,
        Some(0),
        "recovery ran against a fresh incarnation, having handled nothing"
    );

    s.run_for(Duration::from_millis(50));
    assert_eq!(s.protocol(A).unwrap().events_handled, 1, "and the message was handled after");
    assert_eq!(s.trace().indications_at(A).count(), 2, "recovery announced, then the message did");
}

#[test]
fn recovery_reads_and_acts_within_the_handler() {
    // The read and what it decides are one uninterruptible step, so the announcement immediately
    // follows the recovery in the trace with nothing of this node's in between.
    let mut s = keepers(9);
    s.command(A, LedgerCmd::Record(6));
    s.run_for(Duration::from_millis(10));

    s.crash(A);
    s.restart(A);

    let after: Vec<&TraceEvent<u32, Ledger>> = s
        .trace()
        .events()
        .iter()
        .skip_while(|e| !matches!(e, TraceEvent::Recovered { node, .. } if *node == A))
        .collect();
    assert!(matches!(after[0], TraceEvent::Recovered { had_state: true, .. }));
    assert!(
        matches!(after[1], TraceEvent::Indicated { node, ind, .. }
            if *node == A && ind.total == 6),
        "the announcement is the very next thing, so nothing came between the read and it"
    );
    assert_eq!(s.protocol(A).unwrap().replayed, vec![6], "and it had read the entries too");
}

#[test]
fn a_run_with_writes_crashes_and_recoveries_reproduces_from_its_seed() {
    let run = |seed: u64| {
        let mut s = keepers(seed);
        s.crash_on_next_write(A);
        s.command(A, LedgerCmd::RecordAndTell(1, B));
        s.run_for(Duration::from_millis(20));
        s.restart(A);
        s.command(A, LedgerCmd::Record(2));
        s.run_for(Duration::from_millis(80));
        (s.trace().len(), s.protocol(A).unwrap().total, s.trace().deaths_in_writes())
    };
    for seed in 0..12u64 {
        assert_eq!(run(seed), run(seed), "seed {seed} must reproduce exactly");
    }
}

// ------------------------- that the scope bridge is opt-in, and what forgetting it costs

/// Records every scope event it is given, and nothing else. `Scope` is inhabited, so this one
/// *can* be told — which is what makes forgetting to tell it observable.
struct Listener {
    heard: Vec<recon_core::SessionEvent>,
}

impl Protocol for Listener {
    type Cmd = ();
    type Ind = ();
    type Msg = ();
    type Scope = recon_core::SessionEvent;
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_cmd(&mut self, (): (), _cx: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, (): (), _cx: &mut ProtoCx<'_, Self>) {}
    fn on_timer(&mut self, _: TimerId, _cx: &mut ProtoCx<'_, Self>) {}
    fn on_scope_event(&mut self, e: recon_core::SessionEvent, _cx: &mut ProtoCx<'_, Self>) {
        self.heard.push(e);
    }
}

fn listeners(deliver: bool) -> Sim<Listener> {
    let mut s: Sim<Listener> = Sim::new(Config::default().seed(31).sessions(), &[A, B], |_| {
        Listener { heard: Vec::new() }
    });
    if deliver {
        s.deliver_session_events();
    }
    s
}

#[test]
fn forgetting_deliver_session_events_silently_disables_the_whole_bridge() {
    // `deliver_session_events` is opt-in and nothing compile-checks it: the bound lives on the
    // method, so a protocol that declares no scopes cannot call it, and one that does is free not
    // to. A driver that forgets gets a run where sessions open and close and no layer is ever
    // told — every resend clause dead, every `[session]` tag unearned, and not one test failing
    // to say so. This pins that, so the hazard is at least written down in a place that runs.
    let mut without = listeners(false);
    without.run_for(Duration::from_millis(50));
    without.break_session(A, B);
    without.run_for(Duration::from_millis(500));
    assert!(without.trace().session_ends() > 0, "the sessions really did open and close");
    assert!(
        without.protocol(A).unwrap().heard.is_empty(),
        "and A was told nothing at all: this is the failure that makes no noise"
    );

    let mut with = listeners(true);
    with.run_for(Duration::from_millis(50));
    with.break_session(A, B);
    with.run_for(Duration::from_millis(500));
    let heard = &with.protocol(A).unwrap().heard;
    assert!(
        heard.iter().any(|e| matches!(e, recon_core::SessionEvent::Established { .. }))
            && heard.iter().any(|e| matches!(e, recon_core::SessionEvent::Ended { .. })),
        "with the opt-in, both boundaries arrive: {heard:?}"
    );
}

// ------------------------------ Timers are named by a handle the run owns: tasks 3.3 to 3.5

/// Two layers, each with a timer of its own, in one process. Each remembers the handle it was
/// given, so a test can ask which of them a trace entry belongs to — which is the whole question
/// a handle has to answer once the type no longer says where in the composition it came from.
#[derive(Default)]
struct TwoTimers {
    slow: Option<TimerId>,
    fast: Option<TimerId>,
    slow_fires: u32,
    fast_fires: u32,
}

impl TwoTimers {
    fn slow_period() -> Duration {
        Duration::from_millis(70)
    }
    fn fast_period() -> Duration {
        Duration::from_millis(10)
    }
}

impl Protocol for TwoTimers {
    type Cmd = ();
    type Ind = ();
    type Msg = ();
    type Scope = core::convert::Infallible;
    /// Keeps nothing durably: a crash loses everything this protocol knows.
    type Meta = core::convert::Infallible;
    type Entry = core::convert::Infallible;

    fn on_init(&mut self, cx: &mut ProtoCx<'_, Self>) {
        self.slow = Some(cx.set_timer(Self::slow_period()));
        self.fast = Some(cx.set_timer(Self::fast_period()));
    }

    fn on_cmd(&mut self, (): (), _: &mut ProtoCx<'_, Self>) {}
    fn on_msg(&mut self, _: NodeId, (): (), _: &mut ProtoCx<'_, Self>) {}

    fn on_timer(&mut self, id: TimerId, cx: &mut ProtoCx<'_, Self>) {
        // Each layer acts only on the expiry it registered, and re-arms with a fresh handle.
        if self.slow == Some(id) {
            self.slow_fires += 1;
            self.slow = Some(cx.set_timer(Self::slow_period()));
        } else if self.fast == Some(id) {
            self.fast_fires += 1;
            self.fast = Some(cx.set_timer(Self::fast_period()));
        }
    }
}

fn two_timers(config: Config) -> Sim<TwoTimers> {
    Sim::new(config, &[A, B], |_| TwoTimers::default())
}

#[test]
fn two_layers_of_one_process_are_given_different_handles() {
    // A source owned per protocol, or begun afresh for each event, would hand both the same
    // identity — and each would then accept the other's expiry as its own.
    let mut s = two_timers(Config::default().seed(3));
    s.step_now(); // both layers have armed their timers

    let a = s.protocol(A).expect("A exists");
    let (slow, fast) = (a.slow.expect("slow armed"), a.fast.expect("fast armed"));
    assert_ne!(slow, fast, "two timers registered in one process must not share a handle");
}

#[test]
fn handles_are_distinct_across_every_layer_of_every_process() {
    // Distinctness is a property of the run, not of a layer: A's handles and B's must not
    // collide either, or a trace entry would not say whose timer it names.
    let mut s = two_timers(Config::default().seed(3));
    s.step_now(); // both layers have armed their timers

    let mut seen = std::collections::BTreeSet::new();
    for node in [A, B] {
        let p = s.protocol(node).expect("the process exists");
        for id in [p.slow.expect("slow armed"), p.fast.expect("fast armed")] {
            assert!(seen.insert(id), "handle {id:?} was issued twice within one run");
        }
    }
    assert_eq!(seen.len(), 4);
}

#[test]
fn the_trace_names_which_timer_fired() {
    // The claim the trace has to settle: with two timers outstanding at once, an entry says
    // *which* of them fired — by the handle the registering layer was given, which that layer
    // still holds. Before this it said only that "a timer" fired.
    let mut s = two_timers(Config::default().seed(3));
    s.step_now(); // both layers have armed their timers
    let slow_first = s.protocol(A).expect("A exists").slow.expect("slow armed");

    // Far enough for the fast timer to have fired repeatedly and the slow one exactly once.
    s.run_for(Duration::from_millis(100));

    let fired: Vec<TimerId> = s
        .trace()
        .events()
        .iter()
        .filter_map(|e| match e {
            TraceEvent::TimerFired { node, id, .. } if *node == A => Some(*id),
            _ => None,
        })
        .collect();

    assert!(fired.contains(&slow_first), "the slow timer's own handle appears in the trace");
    assert_eq!(
        fired.iter().filter(|id| **id == slow_first).count(),
        1,
        "and names that firing alone — a handle is spent when the timer it named fires"
    );
    let a = s.protocol(A).expect("A exists");
    assert_eq!(a.slow_fires, 1, "one slow expiry...");
    assert!(a.fast_fires > 5, "...against many fast ones, so the trace had to distinguish them");
}

#[test]
fn a_run_with_timers_reproduces_from_its_seed_including_its_handles() {
    // The trace comparison covers the handles because a timer entry now carries one. An identity
    // drawn from anywhere but the run — a process-global counter, say — would break this while
    // leaving every other trace entry identical.
    let run = |seed: u64| {
        let mut s = two_timers(Config::default().seed(seed));
        s.run_for(Duration::from_millis(200));
        let events: Vec<String> = s.trace().events().iter().map(|e| format!("{e:?}")).collect();
        let timers = s.trace().timer_fires();
        (events, timers)
    };

    let (first, fires) = run(9);
    let (again, _) = run(9);
    assert!(fires > 10, "non-vacuity: the run must actually have fired timers");
    assert_eq!(first, again, "the same seed must reproduce the same trace, handles included");
}

// ------------------------------------------------------- stepping by event, not by duration

#[test]
fn step_now_dispatches_this_instant_and_leaves_the_clock_where_it_is() {
    // A command is scheduled, not run. `step_now` runs it — and everything else due now — without
    // moving the clock, so what it sent is in flight afterwards and a test can act on that state
    // without guessing at a duration shorter than the latency.
    let mut s =
        sim(Config::default().seed(1).latency(Duration::from_millis(5), Duration::from_millis(10)));
    s.command(A, Cmd::SendTo(B, 7));
    assert_eq!(s.trace().send_count(), 0, "scheduled, not yet run");

    s.step_now();
    assert_eq!(s.now(), Time::ZERO, "the clock did not move");
    assert_eq!(s.trace().send_count(), 1, "the command ran and its send is in flight");
    assert_eq!(s.trace().delivery_count(), 0, "nothing due later was touched");

    s.step_now();
    assert_eq!(s.trace().delivery_count(), 0, "a second call at the same instant finds nothing");

    s.run_for(Duration::from_millis(20));
    assert_eq!(s.trace().delivery_count(), 1);
}

#[test]
fn step_dispatches_one_event_and_moves_the_clock_to_it() {
    let mut s =
        sim(Config::default().seed(1).latency(Duration::from_millis(5), Duration::from_millis(5)));
    s.command(A, Cmd::SendTo(B, 7));
    s.command(A, Cmd::SendTo(C, 8));

    assert!(s.step(), "the first command");
    assert_eq!(s.now(), Time::ZERO);
    assert_eq!(s.trace().send_count(), 1, "one event, one send");
    assert!(s.step(), "the second command");
    assert_eq!(s.trace().send_count(), 2);

    assert!(s.step(), "the first delivery");
    assert_eq!(s.now(), Time::ZERO + Duration::from_millis(5), "the clock moved to the event");
    assert_eq!(s.trace().delivery_count(), 1, "one delivery, not both");

    while s.step() {}
    assert_eq!(s.trace().delivery_count(), 2, "stepping to exhaustion finishes the run");
}

#[test]
fn a_send_in_the_instant_a_session_ended_is_dropped_and_says_so_and_the_next_instant_opens_anew() {
    // The answer to "is a message right after a session comes up lost?": no. What is lost is a send
    // in the very instant the session ended — there is no session then and none opens in that
    // instant — and it is recorded as exactly that. The next instant's send opens the successor and
    // is delivered.
    let mut s = sim(Config::default()
        .seed(1)
        .sessions()
        .latency(Duration::from_millis(5), Duration::from_millis(5)));
    s.command(A, Cmd::SendTo(B, 1));
    s.run_for(Duration::from_millis(50));
    assert_eq!(s.trace().delivery_count(), 1, "the first send opened the session and arrived");
    let first = s.session_epoch(A, B).expect("a session is up");

    s.break_session(A, B);
    s.command(A, Cmd::SendTo(B, 2));
    s.step_now(); // the same instant as the ending
    assert_eq!(s.trace().drops_because(DropReason::NoSession), 1, "dropped, and named");
    assert_eq!(s.trace().drops_because(DropReason::Partitioned), 0, "not called a partition");

    // Any later instant will do — this is not a duration guessed shorter than a latency, it is
    // "not the instant of the ending", which is the one thing the sim refuses.
    s.run_for(Duration::from_millis(1));
    s.command(A, Cmd::SendTo(B, 3));
    s.run_for(Duration::from_millis(50));
    assert_eq!(s.trace().delivery_count(), 2, "the next instant's send arrived");
    assert!(s.session_epoch(A, B).expect("a successor is up") > first, "on a new session");
}

// ------------------------------------------------------- Severing pairs, and non-transitivity

/// Everything `node` was delivered, by sender.
fn heard_from(s: &Sim<Parrot>, node: NodeId) -> Vec<NodeId> {
    s.trace().deliveries().filter(|(_, to, _)| *to == node).map(|(from, _, _)| from).collect()
}

/// Send one message each way between every pair, and settle.
fn everyone_greets(s: &mut Sim<Parrot>) {
    for a in [A, B, C] {
        for b in [A, B, C] {
            if a != b {
                s.command(a, Cmd::SendTo(b, 1));
            }
        }
    }
    s.run_for(Duration::from_millis(100));
}

#[test]
fn a_severed_pair_cannot_reach_each_other_in_either_direction() {
    let mut s = sim(Config::default().seed(1));
    s.sever(A, C);
    everyone_greets(&mut s);

    assert!(!heard_from(&s, A).contains(&C), "A heard from C across a severed pair");
    assert!(!heard_from(&s, C).contains(&A), "C heard from A — severing is symmetric");
    // And nothing else was touched: B still hears from both, and both from B.
    assert!(heard_from(&s, B).contains(&A) && heard_from(&s, B).contains(&C), "B hears both");
    assert!(heard_from(&s, A).contains(&B) && heard_from(&s, C).contains(&B), "both hear B");
}

#[test]
fn reachability_need_not_be_transitive() {
    // The case a grouping cannot express, and the whole reason for the change: A reaches B, B
    // reaches C, A does not reach C. There is no group any of the three belongs to.
    let mut s = sim(Config::default().seed(2));
    s.sever(A, C);

    assert!(s.reachable(A, B), "A reaches B");
    assert!(s.reachable(B, C), "B reaches C");
    assert!(!s.reachable(A, C), "and yet A does not reach C");

    everyone_greets(&mut s);
    assert!(heard_from(&s, B).contains(&A) && heard_from(&s, B).contains(&C));
    assert!(!heard_from(&s, A).contains(&C) && !heard_from(&s, C).contains(&A));
}

#[test]
fn reachable_reflects_severing_partitioning_and_healing() {
    let mut s = sim(Config::default().seed(3));
    assert!(s.reachable(A, C), "nothing severed to begin with");

    s.sever(A, C);
    assert!(!s.reachable(A, C));
    s.reconnect(A, C);
    assert!(s.reachable(A, C), "reconnect restores one pair");

    // A grouping is the special case in which the severed pairs span two groups.
    s.partition(&[&[A, B], &[C]]);
    assert!(s.reachable(A, B), "same group");
    assert!(!s.reachable(A, C) && !s.reachable(B, C), "across the boundary");

    s.heal();
    for (a, b) in [(A, B), (B, C), (A, C)] {
        assert!(s.reachable(a, b), "heal restores everything");
    }
}

#[test]
fn heal_clears_a_severing_as_well_as_a_partition() {
    let mut s = sim(Config::default().seed(4));
    s.sever(A, C);
    s.heal();
    assert!(s.reachable(A, C), "heal is not only about partitions");

    everyone_greets(&mut s);
    assert!(heard_from(&s, A).contains(&C) && heard_from(&s, C).contains(&A));
}

#[test]
fn reconnecting_one_pair_leaves_the_others_severed() {
    let mut s = sim(Config::default().seed(5));
    s.sever(A, B);
    s.sever(B, C);
    s.reconnect(A, B);

    assert!(s.reachable(A, B), "the reconnected pair");
    assert!(!s.reachable(B, C), "the other severing stands");
    assert!(s.reachable(A, C), "and an untouched pair is untouched");
}

#[test]
fn partitioning_replaces_a_severing_rather_than_adding_to_it() {
    // `partition` is documented as replacing what was severed before. Stated because the other
    // reading — that it adds — would make a test that partitions after severing quietly wrong.
    let mut s = sim(Config::default().seed(6));
    s.sever(A, B);
    s.partition(&[&[A, B], &[C]]);
    assert!(s.reachable(A, B), "the earlier severing was discarded");
    assert!(!s.reachable(A, C), "and the partition applies");
}

#[test]
fn a_pair_severed_mid_run_stops_delivering_from_that_moment() {
    let mut s =
        sim(Config::default().seed(7).latency(Duration::from_millis(5), Duration::from_millis(5)));
    s.command(A, Cmd::SendTo(C, 1));
    s.run_for(Duration::from_millis(50));
    assert_eq!(heard_from(&s, C), vec![A], "the first message arrived");

    s.sever(A, C);
    s.command(A, Cmd::SendTo(C, 2));
    s.run_for(Duration::from_millis(50));
    assert_eq!(heard_from(&s, C), vec![A], "and nothing since");
    assert!(s.trace().drops_because(DropReason::Partitioned) > 0, "recorded as unreachable");
}

#[test]
fn severing_a_pair_ends_its_session_and_leaves_the_others_up() {
    // The session model follows the connectivity model for free: `end_severed_sessions` asks
    // `connected`, which now answers per pair. What is new is that only *one* session ends.
    let mut s = session_sim(8);
    s.command(A, Cmd::SendTo(B, 1));
    s.command(A, Cmd::SendTo(C, 1));
    s.command(B, Cmd::SendTo(C, 1));
    s.run_for(Duration::from_millis(100));
    for (a, b) in [(A, B), (A, C), (B, C)] {
        assert!(s.has_session(a, b), "{a}–{b} is up before the severing");
    }

    s.sever(A, C);
    assert!(!s.has_session(A, C), "the severed pair's session ended");
    assert!(s.has_session(A, B) && s.has_session(B, C), "and no other session did");

    // It comes back on reconnection, at a higher epoch, exactly as a healed partition does.
    let before = s.session_epoch(A, B).expect("A–B never went away");
    s.reconnect(A, C);
    s.command(A, Cmd::SendTo(C, 2));
    s.run_for(Duration::from_millis(200));
    assert!(s.has_session(A, C), "the session re-established without being prompted");
    assert_eq!(s.session_epoch(A, B), Some(before), "and the untouched one was not disturbed");
}
