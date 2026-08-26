//! Logged perfect links against Module 2.4 — and, because the durable record is the whole
//! purchase, that the perfect link beside it does deliver twice on the schedule this survives.

use core::time::Duration;
use recon_core::NodeId;
use recon_protocols::logged_link::{Cmd, Ind, LoggedLink};
use recon_protocols::perfect_link::{self as pl, PerfectLink};
use recon_sim::{Config, Sim, TraceEvent};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const ALL: [NodeId; 2] = [A, B];

fn retransmit() -> Duration {
    Duration::from_millis(10)
}

type Link = LoggedLink<u32>;

fn sim(seed: u64) -> Sim<Link> {
    Sim::new(Config::default().seed(seed), &ALL, |me| LoggedLink::new(me, retransmit()))
}

/// A lossy run, so that retransmission is doing real work.
fn lossy(seed: u64) -> Sim<Link> {
    Sim::new(Config::default().seed(seed).loss(0.5), &ALL, |me| LoggedLink::new(me, retransmit()))
}

/// Every log this node was handed, in order. The layer above reads the set, not a message.
fn logs(s: &Sim<Link>, node: NodeId) -> Vec<Vec<u32>> {
    s.trace()
        .indications_at(node)
        .map(|Ind::Delivered(log)| log.entries().map(|(_, p)| *p).collect())
        .collect()
}

/// What `node` has log-delivered, from its durable state rather than from the notifications.
fn log_of(s: &Sim<Link>, node: NodeId) -> Vec<u32> {
    s.protocol(node).unwrap().log().entries().map(|(_, p)| *p).collect()
}

fn settle(s: &mut Sim<Link>) {
    s.run_for(Duration::from_millis(600));
}

// ------------------------------------------------- log-delivery

#[test]
fn a_first_receipt_is_logged_and_then_announced() {
    let mut s = sim(1);
    s.command(A, Cmd::Send { to: B, msg: 7 });
    settle(&mut s);

    assert_eq!(log_of(&s, B), vec![7]);
    assert_eq!(logs(&s, B), vec![vec![7]], "announced once, carrying the set");

    // The write precedes the announcement: nothing is told to the layer above that is not
    // already durable.
    let stored_at = s.trace().events().iter().find_map(|e| match e {
        TraceEvent::Stored { at, node } if *node == B => Some(*at),
        _ => None,
    });
    let told_at = s.trace().events().iter().find_map(|e| match e {
        TraceEvent::Indicated { at, node, .. } if *node == B => Some(*at),
        _ => None,
    });
    assert!(stored_at.unwrap() <= told_at.unwrap(), "durable before announced");
}

#[test]
fn the_layer_above_reads_the_set_rather_than_receiving_a_message() {
    let mut s = sim(2);
    s.command(A, Cmd::Send { to: B, msg: 1 });
    s.run_for(Duration::from_millis(100));
    s.command(A, Cmd::Send { to: B, msg: 2 });
    settle(&mut s);

    // Each announcement carries everything log-delivered so far, not the one that arrived.
    assert_eq!(logs(&s, B), vec![vec![1], vec![1, 2]]);
}

#[test]
fn a_message_from_a_surviving_sender_arrives_despite_loss() {
    for seed in 0..10u64 {
        let mut s = lossy(seed);
        s.command(A, Cmd::Send { to: B, msg: 5 });
        s.run_for(Duration::from_secs(3));
        assert_eq!(log_of(&s, B), vec![5], "seed {seed}: retransmission gets it through");
    }
}

#[test]
fn the_log_is_durable_before_the_announcement_even_across_a_crash() {
    // Crash B between logging and being told. The message must be in the retrieved set — it
    // cannot survive only in a notification, which is the whole reason the indication is a set.
    let mut kept = 0;
    for seed in 0..40u64 {
        let mut s = sim(seed);
        s.command(A, Cmd::Send { to: B, msg: 9 });
        s.run_for(Duration::from_millis(2)); // arrived; the write may or may not have completed
        s.crash(B);
        s.restart(B);
        settle(&mut s);
        // Whatever happened, B ends up holding it exactly once: either the write survived, or the
        // sender's retransmission delivered it again afterwards.
        assert_eq!(log_of(&s, B), vec![9], "seed {seed}");
        if s.trace().recoveries_with_state() > 0 {
            kept += 1;
        }
    }
    assert!(kept > 0, "and in some runs the write had already completed");
}

// ------------------------------------------------- across a restart

#[test]
fn no_duplication_holds_across_a_restart() {
    // The property this protocol exists for. B log-delivers, crashes, restarts, and the sender is
    // still retransmitting — as a stubborn link always is. The durable record suppresses it.
    let mut s = sim(3);
    s.command(A, Cmd::Send { to: B, msg: 4 });
    s.run_for(Duration::from_millis(200)); // logged, and the write long since durable
    assert_eq!(log_of(&s, B), vec![4]);

    s.crash(B);
    s.restart(B);
    settle(&mut s); // ample time for many retransmissions to arrive

    assert_eq!(log_of(&s, B), vec![4], "still exactly once, an incarnation later");
    assert!(
        s.trace().deliveries().filter(|(_, to, _)| *to == B).count() > 1,
        "and it did arrive again"
    );
}

#[test]
fn the_perfect_link_does_deliver_twice_under_the_same_schedule() {
    // The contrast, and the point. Same schedule, a link whose record is volatile.
    type Vol = PerfectLink<u32>;
    let mut s: Sim<Vol> =
        Sim::new(Config::default().seed(3), &ALL, |me| PerfectLink::new(me, retransmit()));
    s.command(A, pl::Cmd::Send { to: B, msg: 4 });
    s.run_for(Duration::from_millis(200));

    let before = s.trace().indications_at(B).count();
    assert_eq!(before, 1, "delivered once so far");

    s.crash(B);
    s.restart(B);
    s.run_for(Duration::from_millis(600));

    assert!(
        s.trace().indications_at(B).count() > before,
        "the volatile record was lost, so the retransmission is delivered a second time — which \
         is what the durable one buys"
    );
}

#[test]
fn recovery_re_announces_the_log_with_no_message_having_arrived() {
    let mut s = sim(4);
    s.command(A, Cmd::Send { to: B, msg: 6 });
    s.run_for(Duration::from_millis(200));
    let announced = logs(&s, B).len();

    let at = s.now();
    s.crash(B);
    s.restart(B);

    let after: Vec<Vec<u32>> = s
        .trace()
        .events()
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Indicated { at: t, node, ind: Ind::Delivered(log) }
                if *node == B && *t >= at =>
            {
                Some(log.entries().map(|(_, p)| *p).collect())
            }
            _ => None,
        })
        .collect();

    assert_eq!(after, vec![vec![6]], "told again on recovering");
    assert!(logs(&s, B).len() > announced);
}

// ------------------------------------------------- the stated limits

#[test]
fn a_sender_crashing_before_the_message_reaches_anyone_promises_nothing() {
    // LPL1 is conditioned on the sender never crashing, and this is why: nothing in the system
    // has a record of the send, so nothing can retransmit it.
    let mut s = sim(5);
    s.command(A, Cmd::Send { to: B, msg: 8 });
    s.crash(A); // before the first transmission could be delivered
    settle(&mut s);

    assert!(log_of(&s, B).is_empty(), "no delivery is required, and none happens");
    assert!(!s.trace().is_empty());
}

#[test]
fn nothing_is_log_delivered_that_was_not_sent() {
    for seed in 0..8u64 {
        let mut s = lossy(seed);
        s.command(A, Cmd::Send { to: B, msg: 11 });
        s.command(B, Cmd::Send { to: A, msg: 22 });
        s.run_for(Duration::from_secs(2));
        assert_eq!(log_of(&s, B), vec![11], "seed {seed}");
        assert_eq!(log_of(&s, A), vec![22], "seed {seed}");
    }
}

#[test]
fn the_durable_set_grows_with_distinct_messages_log_delivered() {
    // The stated bound, asserted rather than assumed: nothing retires an entry.
    let mut s = sim(6);
    for n in 0..12u32 {
        s.command(A, Cmd::Send { to: B, msg: n });
    }
    settle(&mut s);

    assert_eq!(s.protocol(B).unwrap().log().len(), 12, "one entry per distinct message, for ever");
    assert!(s.trace().writes_completed() > 0, "and it is on disk, not merely in memory");

    // Writes coalesce: a store issued while an earlier one is still outstanding replaces it, so
    // there are fewer completions than arrivals. That is safe only because the durable value is
    // the whole log rather than a delta — a later value always contains an earlier one.
    assert!(
        s.trace().writes_completed() <= 12,
        "coalescing can only reduce the count, never raise it"
    );
}

#[test]
fn identical_content_sent_twice_is_log_delivered_twice() {
    // Deduplication is by identifier, not content: two sends are two messages.
    let mut s = sim(7);
    s.command(A, Cmd::Send { to: B, msg: 3 });
    s.command(A, Cmd::Send { to: B, msg: 3 });
    settle(&mut s);
    assert_eq!(log_of(&s, B), vec![3, 3]);
}

#[test]
fn the_wire_survives_encoding() {
    let mut s = sim(8);
    s.enable_codec_check();
    s.command(A, Cmd::Send { to: B, msg: 2 });
    settle(&mut s);
    assert_eq!(log_of(&s, B), vec![2]);
}
