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

/// Longer than the delivery bound, so nothing sent before a crash is still in flight afterwards.
///
/// The restart tests need this, and the reason is a trap this suite fell into once: the stubborn
/// children retransmit everything ever sent on every tick, so at any instant the network holds a
/// full replayable copy of the run. A process crashed and restarted in the same instant is rebuilt
/// by that backlog — the network's redundancy, which is the *crash-stop* member's story — and a
/// durability test that allows it asserts nothing about storage.
///
/// **The order is partition, then drain, then restart**, and it is the order that does the work.
/// `Sim::partition` refuses *future* sends; it does not retract what is already scheduled, and one
/// stubborn tick carries the whole history. Draining after the partition is what empties the queue
/// A would otherwise wake up into. Partitioning after the drain leaves a delivery bound's worth in
/// flight, which was enough to rebuild the log — the mutation audit caught exactly that in the
/// write-death test below, one commit after this suite was supposedly fixed.
const DRAIN: Duration = Duration::from_millis(100);

type Lutob = LoggedUniformTotalOrderBroadcast<u32>;

fn sim(seed: u64) -> Sim<Lutob> {
    // A transcription's stubborn children retransmit everything for ever, so a run with three
    // settle windows queues past the default step budget — and `run_for` stops dispatching at the
    // budget without saying so. The crash-and-recover tests here need the room.
    let config = Config::default().seed(seed).synchronous(BOUND).max_steps(10_000_000);
    Sim::new(config, &ALL, |me| Lutob::new(me, ALL, timing()))
}

fn settle(s: &mut Sim<Lutob>) {
    // Any later instant, not a sequencing device: rounds decide within ~300ms and detection takes
    // 120ms, so this is still generous — while the idle churn beneath a settle is the stubborn
    // children resending everything ever sent every tick, which is what makes a long window cost.
    s.run_for(Duration::from_millis(2000));
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

    // Cut A off, then drain what was already scheduled for it against the dead process, and only
    // then bring it back. What it holds came from its own storage and nowhere else — which is the
    // one thing this member claims and the crash-stop member does not.
    s.crash(A);
    s.partition(&[&[A], &ALL[1..]]);
    s.run_for(DRAIN);
    s.restart(A);
    settle(&mut s);

    assert_eq!(
        s.trace().recoveries_with_state(),
        1,
        "the restart did not take the recovery branch"
    );
    let after = held(&s, A);
    assert_eq!(after, before, "what A had ordered did not survive its own restart");
}

#[test]
fn a_restarted_process_agrees_with_one_that_never_failed() {
    let mut s = sim(2);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(10 + i as u32));
    }
    settle(&mut s);
    s.crash(B);
    s.run_for(DRAIN);
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
    // Some rounds first, so records exist for the recovery to read; then the doom is armed and
    // fresh appends force the write it is waiting for — before the restart, which is where the
    // earlier version of this test went wrong: it counted deaths over the whole run, and the one
    // it counted happened after the recovery it thought it was testing.
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(i as u32));
    }
    s.run_for(Duration::from_millis(1500));
    s.crash_on_next_write(A);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(10 + i as u32));
    }
    settle(&mut s);

    assert!(s.trace().deaths_in_writes() > 0, "nobody died inside a write, so nothing was tested");

    // A is down where the doomed write left it. Isolate, then drain, then restart, as the test
    // above does: recovery must come from what landed, not from the backlog.
    s.partition(&[&[A], &ALL[1..]]);
    s.run_for(DRAIN);
    s.restart(A);
    settle(&mut s);

    let recovered = held(&s, A);
    let steady = held(&s, C);
    assert!(!recovered.is_empty(), "A recovered nothing, so consistency was not tested");
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

    // The page writes `store(delivered)` and `store(proposals)`, rewriting whole growing
    // structures; this module appends them instead. The types enforce the split — the growing
    // halves live in `Record`, which `Durable` cannot carry — so what is left to a test is
    // non-vacuity: every ordered entry at every process really went through the appended sequence.
    // An earlier form asserted appends outnumber rewrites, which broke the moment the consensus
    // instances' detectors ran: a live detector rewrites its bounded record at its own cadence, and
    // the count of bounded rewrites says nothing about the O(n²) failure this departure removed.
    let ordered_everywhere = ALL.len() * ALL.len();
    assert!(
        s.trace().appends() >= ordered_everywhere,
        "{} appends cannot cover {} ordered entries — the growing halves went somewhere else",
        s.trace().appends(),
        ordered_everywhere
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
    s.run_for(DRAIN);
    s.restart(A);
    settle(&mut s);

    assert!(!held(&s, A).is_empty(), "A recovered nothing, so nothing was measured");
    assert_send_rate_flat!(s, Duration::from_millis(400), 4);
}

/// Recovery restores what new work needs, not only what old work left. Resuming exercises replay;
/// an append *after* recovering exercises what replay forgot to restore — a round counter left at
/// one, a proposal re-made for a round that already decided.
#[test]
fn a_recovered_process_appends_something_new() {
    let mut s = sim(7);
    for (i, node) in ALL.iter().enumerate() {
        s.command(*node, Lutob::append(i as u32));
    }
    settle(&mut s);
    let before = held(&s, A).len();
    assert!(before > 0, "nothing was ordered before the crash");

    s.crash(A);
    s.run_for(DRAIN);
    s.restart(A);
    settle(&mut s);

    s.command(A, Lutob::append(99));
    settle(&mut s);

    for node in ALL {
        let seq = held(&s, node);
        assert!(
            seq.contains(&99),
            "{node} never ordered the entry appended after recovery: {seq:?}"
        );
    }
    let a = held(&s, A);
    let c = held(&s, C);
    assert!(
        is_prefix(&a, &c) || is_prefix(&c, &a),
        "the recovered process diverged from a steady one: {a:?} vs {c:?}"
    );
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
