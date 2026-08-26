//! Stubborn broadcast: the repeats are the interface, and a process that was down when a message
//! was sent still receives it after recovering.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::stubborn_broadcast::{Cmd, Ind, StubbornBroadcast};
use recon_sim::{Config, Sim};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const ALL: [NodeId; 3] = [A, B, C];

fn interval() -> Duration {
    Duration::from_millis(10)
}

type Sb = StubbornBroadcast<u32>;

fn sim(seed: u64) -> Sim<Sb> {
    Sim::new(Config::default().seed(seed), &ALL, |me| StubbornBroadcast::new(me, ALL, interval()))
}

fn deliveries(s: &Sim<Sb>, node: NodeId) -> Vec<(NodeId, u32)> {
    s.trace().indications_at(node).map(|Ind::Deliver { from, msg }| (*from, *msg)).collect()
}

#[test]
fn a_broadcast_is_delivered_infinitely_often() {
    let mut s = sim(1);
    s.command(A, Cmd::Broadcast(5));
    s.run_for(Duration::from_millis(200));
    let early: Vec<usize> = ALL.iter().map(|n| deliveries(&s, *n).len()).collect();
    assert!(early.iter().all(|c| *c > 1), "already repeating: {early:?}");

    s.run_for(Duration::from_millis(500));
    let late: Vec<usize> = ALL.iter().map(|n| deliveries(&s, *n).len()).collect();
    for (e, l) in early.iter().zip(&late) {
        assert!(l > e, "and still going: {early:?} then {late:?}");
    }
}

#[test]
fn a_process_that_was_down_receives_it_after_recovering() {
    // The reason this rung exists. C is crashed when the broadcast happens and has no record of
    // it and no way to ask; only a sender that never stopped trying reaches it.
    let mut s = sim(2);
    s.crash(C);
    s.command(A, Cmd::Broadcast(9));
    s.run_for(Duration::from_millis(200));
    assert!(deliveries(&s, C).is_empty(), "C was down and missed it entirely");

    s.restart(C);
    s.run_for(Duration::from_millis(300));
    assert!(
        deliveries(&s, C).iter().any(|(_, m)| *m == 9),
        "and receives it once it is back, because nothing ever stopped transmitting"
    );
}

#[test]
fn repeats_are_delivered_rather_than_suppressed() {
    // Deduplicating here would defeat the point: the layer above is idempotent, this layer is not
    // responsible for making it so.
    let mut s = sim(3);
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(300));
    let got = deliveries(&s, B);
    assert!(got.len() > 3, "many arrivals, all delivered: {}", got.len());
    assert!(got.iter().all(|(from, msg)| (*from, *msg) == (A, 1)));
}

#[test]
fn nothing_is_delivered_that_was_not_broadcast() {
    for seed in 0..8u64 {
        let mut s = sim(seed);
        s.command(A, Cmd::Broadcast(11));
        s.command(B, Cmd::Broadcast(22));
        s.run_for(Duration::from_millis(200));
        for n in ALL {
            for (from, msg) in deliveries(&s, n) {
                assert!(
                    matches!((from, msg), (A, 11) | (B, 22)),
                    "seed {seed}: {n} delivered ({from}, {msg}), never broadcast"
                );
            }
        }
    }
}

#[test]
fn receiving_does_not_grow_this_layers_state() {
    // Bounded by membership and by what is outstanding — receiving adds nothing.
    let mut s = sim(4);
    s.command(A, Cmd::Broadcast(1));
    s.run_for(Duration::from_millis(600));

    let arrivals = deliveries(&s, C).len();
    assert!(arrivals > 10, "plenty arrived at C: {arrivals}");
    assert_eq!(
        s.protocol(C).unwrap().peers().count(),
        ALL.len(),
        "and C's own state is still just the process set"
    );
}

#[test]
fn nothing_is_delivered_when_nothing_is_broadcast() {
    let mut s = sim(5);
    s.run_for(Duration::from_millis(300));
    for n in ALL {
        assert!(deliveries(&s, n).is_empty(), "{n}");
    }
    assert_eq!(s.trace().send_count(), 0);
}
