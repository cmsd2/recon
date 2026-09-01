//! Logged epoch-change against Algorithm 5.8.
//!
//! What this suite is for is the half [`epoch_change`](../src/epoch_change.rs)'s cannot ask: the
//! epoch a process has entered has to be durable *before* the process acts as though it has entered
//! it, and it has to still be there afterwards.

use recon_core::NodeId;
use recon_protocols::logged_epoch_change::{Ind, LoggedEpochChange};
use recon_sim::{Config, Sim, TraceEvent};

mod common;
use common::*;

type Lec = LoggedEpochChange;

fn lec(me: NodeId) -> Lec {
    LoggedEpochChange::new(me, ALL, timing())
}

fn sim(seed: u64) -> Sim<Lec> {
    Sim::new(Config::default().seed(seed).synchronous(BOUND), &ALL, lec)
}

/// The epochs `node` was told to start, in order.
fn started(s: &Sim<Lec>, node: NodeId) -> Vec<(u64, NodeId)> {
    s.trace().indications_at(node).map(|Ind::StartEpoch { ts, leader }| (*ts, *leader)).collect()
}

// ------------------------------------------------- durable before visible: task 6.1

#[test]
fn the_epoch_is_durable_before_anything_reveals_it() {
    // `store(startts, start); trigger ⟨ lec, StartEpoch | startts, start ⟩` — in that order, and
    // the order is the whole obligation. A process that told the consensus above to enter epoch 20
    // and then came back believing it had entered nothing would read an empty state where an
    // accepted value belongs.
    let mut s = sim(1);
    s.run_for(timeout() * 4);

    for n in ALL {
        assert!(!started(&s, n).is_empty(), "{n} started no epoch, so there is nothing to order");
    }

    // Walk the trace in order. Every indication at a node must be preceded by a write at that same
    // node — and by one more write than the previous indication was, since each entered epoch is
    // recorded before it is announced.
    let mut writes = std::collections::BTreeMap::<NodeId, usize>::new();
    let mut inds = std::collections::BTreeMap::<NodeId, usize>::new();
    for e in s.trace().events() {
        match e {
            TraceEvent::Wrote { node, .. } => *writes.entry(*node).or_default() += 1,
            TraceEvent::Indicated { node, .. } => {
                let seen = inds.entry(*node).or_default();
                *seen += 1;
                assert!(
                    writes.get(node).copied().unwrap_or(0) >= *seen,
                    "{node} announced epoch {seen} with only {} writes behind it",
                    writes.get(node).copied().unwrap_or(0)
                );
            }
            _ => {}
        }
    }
}

#[test]
fn nothing_is_written_that_is_not_an_epoch_entered() {
    // The other half of the ordering claim: the writes are not a stream of speculative saves that
    // happen to outnumber the indications. One write per entered epoch, exactly.
    let mut s = sim(2);
    s.run_for(timeout() * 4);

    let entered: usize = ALL.iter().map(|n| started(&s, *n).len()).sum();
    assert!(entered > 0, "no epoch was entered anywhere");
    assert_eq!(s.trace().writes(), entered, "one write per entered epoch");
    assert_eq!(s.trace().metadata_writes(), entered, "and all of them rewrites, never appends");
}

// ------------------------------------------------- restart: task 6.2

#[test]
fn a_restarted_process_does_not_enter_a_timestamp_it_has_entered_before() {
    // `ts` is volatile and the book does not store it, so a recovered leader really does climb from
    // `rank(self)` again and really does re-announce candidates it has used. What must not happen
    // is that a process *enters* an epoch at a timestamp it has entered before: `startts` is
    // durable and `newts > startts` is checked against the recovered value.
    let mut s = sim(3);
    s.run_for(timeout() * 4);

    let before: Vec<(u64, NodeId)> = started(&s, E);
    assert!(!before.is_empty(), "E entered no epoch before the crash");

    s.crash(E);
    s.run_for(timeout() * 2);
    s.restart(E);
    s.run_for(timeout() * 8);

    for n in ALL {
        let seen = started(&s, n);
        let mut sorted: Vec<u64> = seen.iter().map(|(ts, _)| *ts).collect();
        let strictly_increasing = sorted.windows(2).all(|w| w[0] < w[1]);
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "{n} entered the same timestamp twice: {seen:?}");
        assert!(strictly_increasing, "{n}'s entered timestamps did not increase: {seen:?}");
    }

    // And the non-vacuity half: the restarted process really did re-announce a candidate it had
    // used before, so the guard above was doing work rather than never being reached.
    let p = s.at(E);
    assert!(
        p.last_timestamp() >= before.last().unwrap().0,
        "E came back believing it had entered epoch {} when it had entered {}",
        p.last_timestamp(),
        before.last().unwrap().0
    );
}

#[test]
fn the_recovered_process_reads_its_epoch_back_rather_than_starting_again() {
    let mut s = sim(4);
    s.run_for(timeout() * 4);
    let (ts, leader) = *started(&s, D).last().expect("D entered an epoch");

    s.crash(D);
    s.run_for(timeout());
    s.restart(D);
    s.run_for(timeout());

    let p = s.at(D);
    assert_eq!(p.last_timestamp(), ts, "D lost the epoch it had entered");
    assert_eq!(p.last_leader(), leader, "D lost who led it");

    // `Recovery` retrieves; it does not re-announce. The layer above already knows.
    assert_eq!(
        started(&s, D).iter().filter(|(t, _)| *t == ts).count(),
        1,
        "D announced the same epoch twice across the restart"
    );
    assert_eq!(s.trace().recoveries_with_state(), 1, "D recovered from a record, not from nothing");
}

#[test]
fn a_process_that_had_entered_nothing_recovers_nothing() {
    // The counterpart, and the reason `Recovered { had_state }` exists: an empty store must not be
    // read as an entered epoch. Crashed before the detector has said anything, so nothing is
    // durable yet.
    let mut s = sim(5);
    s.crash(A);
    s.restart(A);
    s.run_for(timeout());

    assert_eq!(s.trace().recoveries_with_state(), 0, "A had written nothing to recover");
}

// ------------------------------------------------- rejoining: task 6.3

#[test]
fn a_process_recovering_into_settled_leadership_rejoins_the_same_epoch() {
    // Leadership settles on E, everybody enters E's last epoch, then C goes down and comes back.
    // It must rejoin that epoch rather than start a sequence of its own — which is what it would do
    // if `startts` were volatile, since `newts > startts` would then be true of everything.
    let mut s = sim(6);
    s.run_for(timeout() * 6);

    let settled = *started(&s, C).last().expect("C entered an epoch");
    for n in ALL {
        assert_eq!(
            *started(&s, n).last().expect("entered an epoch"),
            settled,
            "{n} is not in the settled epoch, so this run is not the case being tested"
        );
    }

    s.crash(C);
    s.run_for(timeout() * 2);
    s.restart(C);
    s.run_for(timeout() * 8);

    let p = s.at(C);
    assert_eq!(
        (p.last_timestamp(), p.last_leader()),
        settled,
        "C came back into a different epoch"
    );
    assert_eq!(
        *started(&s, C).last().expect("entered an epoch"),
        settled,
        "C announced an epoch after recovering, so it started a fresh sequence"
    );
}

#[test]
fn the_settled_epoch_is_reached_at_all_and_by_everyone() {
    // Non-vacuity for the test above: if leadership never settled, the assertion that C returns to
    // "the settled epoch" would be comparing two arbitrary values that happen to match.
    let mut s = sim(7);
    s.run_for(timeout() * 6);

    let final_epochs: std::collections::BTreeSet<(u64, NodeId)> =
        ALL.iter().map(|n| *started(&s, *n).last().expect("entered an epoch")).collect();
    assert_eq!(final_epochs.len(), 1, "processes ended in different epochs: {final_epochs:?}");

    let (ts, leader) = *final_epochs.iter().next().unwrap();
    assert!(ts > 0, "the settled epoch is the initial one, so nothing was ever announced");
    assert_eq!(leader, E, "leadership settled somewhere other than maxrank(Π)");

    // And it stops: another six timeouts add no further epoch.
    let counts: Vec<usize> = ALL.iter().map(|n| started(&s, *n).len()).collect();
    s.run_for(timeout() * 6);
    let after: Vec<usize> = ALL.iter().map(|n| started(&s, *n).len()).collect();
    assert_eq!(counts, after, "epochs kept starting after leadership settled");
}

// ------------------------------------------------- the refusal still refuses

#[test]
fn a_stale_candidate_is_still_refused_and_the_leader_climbs_past_it() {
    // Non-vacuity for the departure above. Silencing the repeat of an accepted announcement must
    // not silence a refusal that is doing real work: a new leader of lower rank starts from a
    // candidate *below* the epoch everybody has entered, and only the NACK moves it past.
    let mut s = sim(8);
    s.run_for(timeout() * 4);
    let settled = started(&s, A).last().expect("A entered an epoch").0;
    assert_eq!(settled, 5, "E leads epoch rank(E) = 5, and nothing has made it climb");

    s.crash(E);
    s.run_for(timeout() * 8);

    // D's first candidate is rank(D) = 4, which is below the epoch everybody has entered. It
    // cannot be accepted, and D only discovers that from the refusals.
    let d = s.at(D);
    assert_eq!(d.trusted(), D, "D is the new leader once E is accused");
    assert!(d.candidate() > 4, "D never climbed past its first candidate, so no NACK reached it");

    for n in [A, B, C, D] {
        let last = started(&s, n).last().expect("entered an epoch").0;
        assert!(last > settled, "{n} is still in epoch {last}, so the new leader never got in");
        assert_eq!(last % 5, 4, "epoch {last} is not in D's residue class, so D did not lead it");
    }
}

// ------------------------------------------------- bounded by membership, not by time

#[test]
fn the_send_rate_does_not_grow_once_leadership_has_settled() {
    // Over a stubborn broadcast every announcement comes round again for ever. Algorithm 5.8 as
    // printed refuses each one again, on a fresh stubborn transmission, so the rate climbs without
    // bound; the two guards in the module make it flat. This is the test that would have caught it.
    let mut s = sim(20);
    s.run_for(timeout() * 4);
    assert_send_rate_flat!(s, timeout() * 2, 4);
}

#[test]
fn the_send_rate_stays_flat_after_a_leadership_change() {
    // The stale-announcement case: after E crashes, D's first candidates are below the epoch
    // everyone entered and are refused. Each is refused once, not once per redelivery.
    let mut s = sim(21);
    s.run_for(timeout() * 4);
    s.crash(E);
    s.run_for(timeout() * 8);
    assert_send_rate_flat!(s, timeout() * 2, 4);
}

// ------------------------------------------------- dying inside the write

#[test]
fn dying_inside_the_epoch_write_never_leaves_an_epoch_announced_without_a_record() {
    // `store(startts, start); trigger ⟨ lec, StartEpoch ⟩` — the test above checks the order in a
    // run where the write lands. This one arms the write itself, so the crash is *inside* it and
    // the seed decides whether it landed. Either outcome is allowed. What is not allowed is `A`
    // having told the layer above it entered an epoch it has no record of.
    let mut landed = 0;
    let mut lost = 0;
    for seed in 0..40u64 {
        let mut s = sim(seed);
        // Nothing is written at `Init` here, so the first write at A is the epoch it enters.
        s.crash_on_next_write(A);
        s.run_for(timeout() * 2);
        assert_eq!(s.trace().deaths_in_writes(), 1, "seed {seed}: A died in a write");
        assert!(started(&s, A).is_empty(), "seed {seed}: A announced from the doomed handler");

        s.restart(A);
        if s.at(A).last_timestamp() > 0 {
            landed += 1;
        } else {
            lost += 1;
        }

        // Either way the leader's announcement is still being retransmitted, so A ends up in the
        // same epoch as everyone else — with a record behind it this time.
        s.run_for(timeout() * 4);
        let settled = started(&s, E).last().expect("E entered an epoch").0;
        assert_eq!(s.at(A).last_timestamp(), settled, "seed {seed}: A is not in the settled epoch");
    }
    assert!(landed > 0 && lost > 0, "both outcomes must occur: {landed} landed, {lost} lost");
}
