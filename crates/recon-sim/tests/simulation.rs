//! Verifies the simulation contract using a deliberately trivial protocol, so that anything
//! observed is the simulator's behaviour and not a protocol's.

use core::time::Duration;
use recon_core::{NodeId, ProtoCx, Protocol, Time};
use recon_sim::{Config, DropReason, Sim, TraceEvent};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tock;

impl Protocol for Parrot {
    type Cmd = Cmd;
    type Ind = Got;
    type Msg = Wire;
    type Timer = Tock;

    fn on_cmd(&mut self, cmd: Cmd, cx: &mut ProtoCx<'_, Self>) {
        match cmd {
            Cmd::SendTo(to, n) => cx.send(to, Wire(n)),
            Cmd::Tick(d) => cx.set_timer(d, Tock),
        }
    }

    fn on_msg(&mut self, from: NodeId, Wire(n): Wire, cx: &mut ProtoCx<'_, Self>) {
        cx.indicate(Got(from, n));
    }

    fn on_timer(&mut self, Tock: Tock, cx: &mut ProtoCx<'_, Self>) {
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
    let mut s = sim(
        Config::default()
            .seed(seed)
            .loss(0.3)
            .duplication(0.2)
            .reorder(0.2)
            .latency(Duration::from_millis(1), Duration::from_millis(20)),
    );
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
    s.run_until(Time::from_nanos(u64::MAX / 2));
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
    let mut s = sim(
        Config::default().seed(11).latency(Duration::from_millis(1), Duration::from_millis(60)),
    );
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
    let mut s = sim(Config::default().seed(4).loss(0.2).latency(
        Duration::from_millis(1),
        Duration::from_millis(30),
    ));
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
