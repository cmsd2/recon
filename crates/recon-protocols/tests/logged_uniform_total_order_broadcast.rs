//! The fail-recovery member of the pair: the same order, and it survives a restart.
//!
//! The ordering properties are the shared suite's, in `total_order_log.rs`, written against the
//! port. What is here is the half only this member claims.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::logged_uniform_total_order_broadcast::LoggedUniformTotalOrderBroadcast;
use recon_protocols::total_order_log::{LogInd, TotalOrderLog};
use recon_sim::{Config, Sim};

mod common;
use common::{A, ALL, B, BOUND, C, assert_send_rate_flat, timing};

type Lutob = LoggedUniformTotalOrderBroadcast<u32>;

fn sim(seed: u64) -> Sim<Lutob> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, |me| {
        Lutob::new(me, ALL, timing())
    })
}

fn settle(s: &mut Sim<Lutob>) {
    s.run_for(Duration::from_millis(6000));
}

/// The sequence `node` holds, read from its own state rather than its indications — because a
/// restart is the point, and what survives is state.
fn held(s: &Sim<Lutob>, node: NodeId) -> Vec<u32> {
    s.at(node).entries().iter().map(|(_, v)| *v).collect()
}

fn is_prefix(a: &[u32], b: &[u32]) -> bool {
    a.len() <= b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}

#[test]
fn the_ordered_sequence_survives_a_restart() {
    let mut s = sim(1);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(i as u32));
    }
    settle(&mut s);

    let before = held(&s, A);
    assert!(!before.is_empty(), "nothing was ordered, so nothing could survive");

    s.crash(A);
    s.restart(A);
    settle(&mut s);

    let after = held(&s, A);
    assert!(
        is_prefix(&before, &after),
        "what A had ordered did not survive: {before:?} then {after:?}"
    );
    assert_eq!(after.len(), before.len(), "and it did not lose entries: {after:?}");
}

#[test]
fn a_restarted_process_agrees_with_one_that_never_failed() {
    let mut s = sim(2);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(10 + i as u32));
    }
    settle(&mut s);
    s.crash(B);
    s.restart(B);
    settle(&mut s);

    let restarted = held(&s, B);
    let steady = held(&s, C);
    assert!(!restarted.is_empty() && !steady.is_empty());
    assert!(
        is_prefix(&restarted, &steady) || is_prefix(&steady, &restarted),
        "the sequences diverged: {restarted:?} vs {steady:?}"
    );
}

/// The write that matters: a proposal is durable before anyone could observe it, so a process that
/// recovers re-proposes what it recorded rather than something new — which a decided uniform
/// consensus could not accommodate.
#[test]
fn a_process_that_dies_inside_a_write_recovers_consistently() {
    let mut s = sim(3);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(i as u32));
    }
    s.run_for(Duration::from_millis(300));
    s.crash_on_next_write(A);
    settle(&mut s);
    s.crash(A);
    s.restart(A);
    settle(&mut s);

    assert!(s.trace().deaths_in_writes() > 0, "nobody died inside a write, so nothing was tested");
    let recovered = held(&s, A);
    let steady = held(&s, C);
    assert!(
        is_prefix(&recovered, &steady) || is_prefix(&steady, &recovered),
        "a process that died inside a write recovered inconsistently: {recovered:?} vs {steady:?}"
    );
}

/// Non-vacuity for the durable half: the run actually wrote, and appended rather than rewrote.
#[test]
fn the_record_is_appended_rather_than_rewritten() {
    let mut s = sim(4);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(i as u32));
    }
    settle(&mut s);

    assert!(s.trace().appends() > 0, "nothing was appended");
    // The page writes `store(delivered)` and `store(proposals)`, rewriting whole growing
    // structures; this module appends instead, so the appends must dominate. See the departures.
    assert!(
        s.trace().appends() > s.trace().metadata_writes(),
        "{} appends against {} rewrites — the growing halves are being rewritten",
        s.trace().appends(),
        s.trace().metadata_writes()
    );
}

/// Recovery must end. A recovering process re-proposes what it recorded, round by round, until it
/// reaches one it never proposed for — and if that never terminated, the run would keep sending for
/// ever. The shared suite asserts a flat rate for a run with no faults; this asserts it for one
/// after a restart, which is the case only this member has.
#[test]
fn a_recovered_process_settles() {
    let mut s = sim(6);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(i as u32));
    }
    settle(&mut s);
    s.crash(A);
    s.restart(A);
    settle(&mut s);

    assert!(!held(&s, A).is_empty(), "A recovered nothing, so nothing was measured");
    assert_send_rate_flat!(s, Duration::from_millis(400), 4);
}

/// The pair's whole point: what this member claims and the other does not is durability, and both
/// are held to one classification through the port.
#[test]
fn the_port_classifies_this_implementation_too() {
    let mut s = sim(5);
    s.command(A, Lutob::append(7));
    settle(&mut s);

    let ordered: Vec<u32> = s
        .trace()
        .indications_at(A)
        .filter_map(|i| match Lutob::classify(i.clone()) {
            LogInd::Ordered { value, .. } => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(ordered, vec![7]);
}
