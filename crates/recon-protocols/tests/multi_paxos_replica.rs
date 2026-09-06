//! The Multi-Paxos replica of *Paxos Made Moderately Complex* Figure 1, against its own claims.
//!
//! The shared port suite in `tests/total_order_log.rs` holds this module to the same properties as
//! the other two logs. What is here is what only this implementation shows: slots that decide
//! independently and out of order, a window that stops proposals, a command that loses its slot and
//! comes back, and the fourth liveness violation with its one decision dropped rather than a lossy
//! link switched on.
//!
//! Two sources of schedule, as the Synod suite has. The simulator gives breadth. [`Hand`] gives
//! "lose exactly this message and nothing else", which the simulator cannot: one wire, one link.

use core::convert::Infallible;
use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{
    Effect, Event, MemStore, NodeId, Position, SessionEvent, Time, TimerId, step_noting,
};
use recon_protocols::multi_paxos_replica::{Carried, Cmd, Command, Ind, MultiPaxosReplica};
use recon_protocols::multi_paxos_synod::{Slot, SynodMsg, Wire};
use recon_protocols::session_link::SessionLink;
use recon_protocols::{Note, Timing};
use recon_sim::{Config, Sim};
use std::collections::BTreeMap;

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const E: NodeId = NodeId::new(5);
const FIVE: [NodeId; 5] = [A, B, C, D, E];
const THREE: [NodeId; 3] = [A, B, C];

const BOUND: Duration = Duration::from_millis(20);

fn timing() -> Timing {
    Timing { retransmit: Duration::from_millis(10), heartbeat: BOUND * 2, detect_after: BOUND * 6 }
}

type Replica = MultiPaxosReplica<u32, SessionLink<Carried<u32>>>;
type Msg = Wire<Carried<u32>>;

fn replica_over(members: &'static [NodeId]) -> impl Fn(NodeId) -> Replica {
    move |me| MultiPaxosReplica::new(me, members.iter().copied(), timing())
}

fn sim_of(members: &'static [NodeId], config: Config) -> Sim<Replica> {
    let mut s: Sim<Replica> = Sim::new(config.sessions(), members, replica_over(members));
    // Three of this module's decisions produce no effect at all — a window that stopped a
    // proposal, a command put back, a slot asked for again — so the narration is the only place
    // they land. Recording it for every test costs nothing and is what those tests read.
    s.record_notes();
    s.deliver_session_events();
    s
}

fn synchronous(seed: u64) -> Config {
    Config::default().seed(seed).synchronous(BOUND).max_steps(10_000_000)
}

/// Long enough for a leadership change and a re-proposal, which is what the slowest legitimate
/// path here costs. Not a sequencing device: every test that depends on an *order* of events steps.
fn settle(s: &mut Sim<Replica>) {
    s.run_for(Duration::from_millis(2000));
}

/// The sequence `node` ordered, from its own indications.
fn ordered_at(s: &Sim<Replica>, node: NodeId) -> Vec<u32> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            Ind::Ordered { value, .. } => Some(*value),
            _ => None,
        })
        .collect()
}

/// The positions `node` reported, in the order it reported them.
fn positions_at(s: &Sim<Replica>, node: NodeId) -> Vec<Position> {
    s.trace()
        .indications_at(node)
        .filter_map(|i| match i {
            Ind::Ordered { position, .. } => Some(*position),
            _ => None,
        })
        .collect()
}

fn notes_at(s: &Sim<Replica>, node: NodeId) -> Vec<Note> {
    s.trace().notes().filter(|(n, _)| *n == node).map(|(_, note)| note.clone()).collect()
}

// ---------------------------------------------------------------- task 3: the command

#[test]
fn a_command_names_who_asked_and_which_request_of_theirs_it_is() {
    // `c = ⟨κ, cid, op⟩`. Both identifying halves earn their place: `from` is what the port's
    // `Ordered` reports, and `cid` is what keeps two appends of the same value from collapsing into
    // one entry under `perform`'s already-applied check.
    let one = Command { from: A, cid: 0, value: 7u32 };
    let two = Command { from: A, cid: 1, value: 7u32 };
    let other = Command { from: B, cid: 0, value: 7u32 };
    assert_ne!(one, two, "two appends of the same value at one process must differ");
    assert_ne!(one, other, "the same value at two processes must differ");
}

#[test]
fn two_appends_of_the_same_value_take_two_positions() {
    // The consequence of `cid`, end to end. `perform` skips a command it has already applied, so
    // without a request identifier the second append would silently become nothing.
    let mut s = sim_of(&FIVE, synchronous(1));
    s.command(A, Cmd::Append(7));
    s.command(A, Cmd::Append(7));
    settle(&mut s);

    for node in FIVE {
        assert_eq!(ordered_at(&s, node), vec![7, 7], "{node} lost one of two identical appends");
    }
}

#[test]
fn the_wire_survives_encoding() {
    let mut s = sim_of(&FIVE, synchronous(2));
    s.enable_codec_check();
    s.command(E, Cmd::Append(42));
    settle(&mut s);
    // The codec check panics inside the sim on a round trip that does not match, so reaching here
    // with traffic having flowed is the assertion. The floor keeps it from passing vacuously.
    assert!(s.trace().send_count() > 0, "nothing was encoded, so nothing was checked");
    assert!(!ordered_at(&s, E).is_empty(), "and nothing was ordered, so no command was encoded");
}

// ---------------------------------------------------------------- task 4: the replica

#[test]
fn a_request_appended_at_a_process_that_does_not_lead_is_proposed_by_the_one_that_does() {
    // §4.4's colocation, seen from above: a replica hands its proposal to the leader on its own
    // machine, and that leader forwards it to the one Ω trusts. Ω trusts the highest rank from the
    // first heartbeat, so A is passive for the whole run and its append still has to be ordered —
    // by E, under E's ballot.
    let mut s = sim_of(&FIVE, synchronous(3));
    s.command(A, Cmd::Append(11));
    s.step_now();
    assert!(ordered_at(&s, A).is_empty(), "nothing can have been ordered in the same instant");

    settle(&mut s);
    assert!(!s.at(A).synod().is_trusted(), "A must be passive, or the forward is not what ran");
    assert!(s.at(E).synod().is_trusted(), "and E must be the one that leads");
    for node in FIVE {
        assert_eq!(ordered_at(&s, node), vec![11], "{node} never ordered the passive append");
    }
}

#[test]
fn positions_are_contiguous_and_track_the_slots_applied() {
    // `Position` and `Slot` are both `u64` and are not interchangeable — the mix-up the types
    // cannot catch. Positions count entries actually ordered and start at `Position::START`; slots
    // count consensus instances and start at 1. They diverge by exactly the number of commands
    // decided in more than one slot, and this asserts that count is zero for a run through this
    // port.
    //
    // **A duplicate is not reachable through this port as it stands, and that is why this asserts
    // the equality rather than a divergence.** A command is minted at exactly one replica, carries
    // that replica's `cid`, and occupies at most one slot at a time — displaced, it is re-queued
    // and proposed for a later slot, and R1 keeps the slot it lost from ever deciding it. The
    // source's duplicate comes from a *client* retrying a request to several replicas, which is a
    // vocabulary above this port. `perform`'s already-applied arm is kept because it is the page's
    // and because that deployment reaches it; what is asserted here is the invariant it maintains.
    let mut s = sim_of(&FIVE, synchronous(4));
    for (i, node) in FIVE.iter().enumerate() {
        s.command(*node, Cmd::Append(i as u32 + 1));
    }
    settle(&mut s);

    for node in FIVE {
        let seen = positions_at(&s, node);
        assert_eq!(seen.len(), FIVE.len(), "{node} did not order everything: {seen:?}");
        for (i, p) in seen.iter().enumerate() {
            assert_eq!(*p, Position(i as u64), "{node}'s positions are not contiguous: {seen:?}");
        }
        let r = s.at(node);
        assert_eq!(
            r.slot_out() - 1,
            r.len() as u64,
            "{node} applied {} slots and took {} positions, so a command was decided twice",
            r.slot_out() - 1,
            r.len(),
        );
    }
}

#[test]
fn a_command_that_loses_its_slot_is_proposed_again_and_still_reaches_the_sequence() {
    // Figure 1's `if c'' ≠ c' then requests := requests ∪ {c''}`. Every replica starts at
    // `slot_in = 1`, so appending at all five in the same instant makes five commands compete for
    // slot 1 and four of them lose it. Losing must not lose the append.
    let mut s = sim_of(&FIVE, synchronous(5));
    for (i, node) in FIVE.iter().enumerate() {
        s.command(*node, Cmd::Append(100 + i as u32));
    }
    settle(&mut s);

    for node in FIVE {
        let seq = ordered_at(&s, node);
        for i in 0..FIVE.len() {
            assert!(seq.contains(&(100 + i as u32)), "{node} lost a displaced command: {seq:?}");
        }
    }
    // Non-vacuity, and it is the whole point of the test: somebody really was displaced. Without
    // this the run could have handed each replica its own slot and proved nothing.
    let displaced: usize = FIVE
        .iter()
        .map(|n| {
            notes_at(&s, *n).iter().filter(|x| matches!(x, Note::ProposalDisplaced { .. })).count()
        })
        .sum();
    assert!(displaced > 0, "no command ever lost its slot, so nothing was put back");
}

#[test]
fn the_window_stops_proposals_and_an_advancing_sequence_releases_them() {
    // R5, in the form that actually holds. The page writes `∀ρ : ρ.slot in < ρ.slot out + WINDOW`
    // and the sentence beside it says "a replica proposes commands only for slots for which it
    // knows the configuration"; the loop tests before using the slot and increments after, so it
    // exits at `slot_in = slot_out + WINDOW` exactly whenever requests remain. The sentence is what
    // the code follows and what this asserts — see the module.
    const WINDOW: Slot = 3;
    let config = synchronous(6).sessions();
    let mut s: Sim<Replica> = Sim::new(config, &THREE, |me| {
        MultiPaxosReplica::new(me, THREE, timing()).with_window(WINDOW)
    });
    s.record_notes();
    s.deliver_session_events();

    // Ten appends at one replica, against a window of three.
    for i in 0..10u32 {
        s.command(A, Cmd::Append(i));
    }
    s.step_now();
    assert!(s.at(A).waiting() > 0, "requests really were held back");
    assert!(
        notes_at(&s, A).iter().any(|n| matches!(n, Note::WindowFull { .. })),
        "the window is what held them, and it must say so",
    );

    // Both forms are standing invariants, so they are checked after every event rather than at the
    // end. The second is the substantive one: nothing is ever *proposed for* a slot the window does
    // not reach.
    for _ in 0..40_000 {
        if !s.step() {
            break;
        }
        for node in THREE {
            let r = s.at(node);
            assert!(
                r.slot_in() <= r.slot_out() + WINDOW,
                "{node} ran past the loop's own bound: slot_in {} against slot_out {}",
                r.slot_in(),
                r.slot_out(),
            );
            for slot in r.proposed_slots() {
                assert!(
                    slot < r.slot_out() + WINDOW,
                    "{node} proposed for slot {slot}, outside the window at slot_out {}",
                    r.slot_out(),
                );
            }
        }
    }
    settle(&mut s);

    // And the sequence advancing is what released them.
    assert_eq!(ordered_at(&s, A).len(), 10, "the window must let go as `slot_out` advances");
    assert_eq!(s.at(A).waiting(), 0, "nothing is left waiting for a slot");
    assert!(s.at(A).slot_out() > WINDOW, "and `slot_out` really ran past one window's worth");
}

#[test]
fn decisions_out_of_order_and_twice_extend_the_sequence_only_in_order() {
    // Figure 1's `while ∃c' : ⟨slot_out, c'⟩ ∈ decisions` — the loop that makes `decisions` a
    // *held* set rather than a stream. A decision for a slot above `slot_out` is kept and acted on
    // when the slots below it decide; a decision that arrives twice adds nothing; and the sequence
    // only ever grows.
    //
    // Driven by hand because nothing in this stack produces the case on its own: the session link
    // beneath delivers in order within a session, so a leader announcing slots 1 to 3 has them
    // arrive in that order. `tests/total_order_log.rs` asserts that, so it is a schedule this suite
    // has to construct rather than wait for.
    let mut h = Hand::new(&THREE);
    h.settle_until_active(C);

    // Three appends at the leader, so three slots are commanded under one ballot at once.
    for v in [10u32, 20, 30] {
        h.append(C, v);
    }
    // Everything runs except that A hears no decision at all.
    let hide = |_: NodeId, to: NodeId, msg: &Msg| {
        !(to == A && matches!(msg, Wire::Synod(SynodMsg::Decision { .. })))
    };
    h.pump(3, hide);
    assert_eq!(ordered_at_hand(&h, C), vec![10, 20, 30], "the leader has all three");
    assert!(ordered_at_hand(&h, A).is_empty(), "and A has none — the precondition");

    // Now hand A the three decisions in reverse. Only the last of them can extend anything.
    let mut held: Vec<(NodeId, Msg)> = h
        .dropped
        .drain(..)
        .filter(|(_, to, m)| *to == A && matches!(m, Wire::Synod(SynodMsg::Decision { .. })))
        .map(|(from, _, m)| (from, m))
        .collect();
    // Deduplicate by slot: the leader answers a re-proposal as well as announcing, so the same slot
    // can be in here more than once, and this test wants to control the repetition itself.
    let mut seen = std::collections::BTreeSet::new();
    held.retain(|(_, m)| match m {
        Wire::Synod(SynodMsg::Decision { slot, .. }) => seen.insert(*slot),
        _ => false,
    });
    assert_eq!(held.len(), 3, "three slots were decided and withheld: {held:?}");
    held.reverse();

    let mut lengths = Vec::new();
    for msg in held.clone() {
        h.deliver(A, msg);
        lengths.push(ordered_at_hand(&h, A).len());
    }
    assert_eq!(lengths, vec![0, 0, 3], "held until the slot below decided, then all three at once");
    assert_eq!(ordered_at_hand(&h, A), vec![10, 20, 30], "and in slot order, not arrival order");

    // Every one of them again, and the sequence must neither grow nor shrink.
    for msg in held {
        h.deliver(A, msg);
        assert_eq!(ordered_at_hand(&h, A), vec![10, 20, 30], "a repeated decision changed the log");
    }
    assert_eq!(h.at(A).slot_out(), 4, "and `slot_out` did not move either");
}

// ---------------------------------------------------------------- task 5: liveness

#[test]
fn a_replica_does_not_act_on_the_childs_timers() {
    // A timer is named by an opaque handle the driver issues and an expiry is offered to *every*
    // layer, so a layer that registered one must compare before acting. Nothing in the type system
    // enforces that; this does. The child registers its retry sweep at `⟨ Init ⟩`, the detector
    // beneath it registers its own, the replica registers a third, and the replica must act on
    // exactly the one it registered.
    //
    // Fired one handle at a time, deliberately. The first draft fired them all at once and counted
    // one re-proposal, and a replica that swept on *every* expiry passed it: the first sweep
    // re-proposed and reset the clock, so the second found nothing due. Breaking the comparison
    // and requiring the red is what found that.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    h.append(A, 1);
    h.settle(|_, _, _| false); // every message lost, so the slot stays open and stays due
    assert_eq!(h.at(A).outstanding(), 1, "a proposal is outstanding");

    // Round one: fire each handle on its own and see which one the replica acts on. Whatever that
    // handler registers in response is the replica's *next* handle, and the only one it may act
    // on next time.
    h.advance(timing().detect_after * 4);
    let mut owned_next = None;
    for id in h.take_timers(A) {
        let before = h.notes_at(A).count();
        let registered = h.fire(A, id);
        let swept = h.notes_at(A).skip(before).any(|n| matches!(n, Note::SlotReproposed { .. }));
        if swept {
            assert!(owned_next.is_none(), "two different handles made the replica sweep");
            assert_eq!(registered.len(), 1, "the sweep re-arms exactly one timer");
            owned_next = Some(registered[0]);
        }
    }
    let mine =
        owned_next.expect("one handle must have made the replica sweep, or the fix is absent");

    // Round two: the slot is due again. Every handle that is not the replica's fires first, and
    // none of them may move it; then the replica's own does, once.
    h.advance(timing().detect_after * 4);
    let ids = h.take_timers(A);
    assert!(ids.contains(&mine), "the replica's re-armed handle is among those registered");
    let before = h.notes_at(A).count();
    for id in ids.iter().copied().filter(|id| *id != mine) {
        h.fire(A, id);
    }
    assert!(
        !h.notes_at(A).skip(before).any(|n| matches!(n, Note::SlotReproposed { .. })),
        "the replica acted on a handle it did not register",
    );
    h.fire(A, mine);
    let swept =
        h.notes_at(A).skip(before).filter(|n| matches!(n, Note::SlotReproposed { .. })).count();
    assert_eq!(swept, 1, "and on its own handle, once");
}

#[test]
fn a_lost_decision_wedges_the_replica_and_the_re_proposal_unwedges_it() {
    // Liu et al.'s fourth liveness violation, with one decision dropped rather than a lossy link
    // switched on. C leads — it is the highest rank, so Ω trusts it from the start and nothing is
    // starved. B is the replica that never hears, and a decision addressed to B is the only thing
    // this run refuses to deliver.
    let drop_decisions_to_b = |_: NodeId, to: NodeId, msg: &Msg| {
        !(to == B && matches!(msg, Wire::Synod(SynodMsg::Decision { .. })))
    };

    let mut h = Hand::new(&THREE);
    h.settle_until_active(C);

    // B appends. §4.4's forward is what puts it on the wire at all: B does not lead, so its own
    // leader hands the proposal to C's.
    h.append(B, 77);
    assert!(
        h.synod_from(B, C).iter().any(|m| matches!(m, SynodMsg::Propose { .. })),
        "the append must reach the leader as a forwarded proposal: {:?}",
        h.synod_from(B, C),
    );
    // C commands it, and everyone is told except B.
    h.pump(2, drop_decisions_to_b);
    assert_eq!(ordered_at_hand(&h, C), vec![77], "the leader applied it");
    assert_eq!(ordered_at_hand(&h, A), vec![77], "and so did the other process that was told");
    assert!(ordered_at_hand(&h, B).is_empty(), "and B was never told — the precondition");
    assert!(
        h.dropped
            .iter()
            .any(|(_, to, m)| *to == B && matches!(m, Wire::Synod(SynodMsg::Decision { .. }))),
        "a decision for B must really have been sent and dropped, or nothing was withheld",
    );

    // Wedged. `slot_out` cannot pass an undecided slot, so the window fills behind it and B stops
    // proposing at all — while the sweep keeps asking and the leader keeps answering into the void.
    //
    // Forty rounds of a heartbeat each, against a default threshold of `detect_after * 3`. The
    // threshold is not a free number: it must exceed the slowest legitimate wait for a decision,
    // whose worst case is a leadership change — `detect_after` for Ω to move, then phase one, then
    // phase two — and it must be far below what filling a `WINDOW` of eight slots costs, or the
    // wedge outlives the fix. `detect_after * 3` clears the first with room and is nowhere near the
    // second, and being wrong in either direction is safe rather than merely tolerable: see the
    // module on why the replica does not try to tell why a decision has not arrived.
    let stuck = h.at(B).slot_out();
    for i in 0..12u32 {
        h.append(B, 200 + i);
    }
    h.pump(40, drop_decisions_to_b);
    assert_eq!(h.at(B).slot_out(), stuck, "`slot_out` cannot pass an undecided slot");
    assert!(h.at(B).waiting() > 0, "and the window filled behind it");
    assert!(
        h.notes_at(B).any(|n| matches!(n, Note::WindowFull { .. })),
        "which is the wedge, and it says so",
    );
    assert!(
        h.notes_at(B).any(|n| matches!(n, Note::SlotReproposed { .. })),
        "the sweep must have re-proposed the stalled slot rather than giving up",
    );
    assert!(ordered_at_hand(&h, B).is_empty(), "and B has still ordered nothing");

    // The answers were there all along: the leader's commander for that slot exited the instant it
    // decided, so nothing re-announces, and every one of these is a reply to B's asking.
    let answers = h
        .dropped
        .iter()
        .filter(|(from, to, m)| {
            *from == C && *to == B && matches!(m, Wire::Synod(SynodMsg::Decision { .. }))
        })
        .count();
    assert!(
        answers > 1,
        "only {answers} answers, so the leader was not answering the re-proposals"
    );

    // Deliver them, and that is the whole of the recovery.
    h.pump(40, |_, _, _| true);
    assert!(h.at(B).slot_out() > stuck, "`slot_out` moved");
    assert_eq!(
        ordered_at_hand(&h, B).len(),
        13,
        "the whole backlog must drain once the answer gets through: {:?}",
        ordered_at_hand(&h, B),
    );
    assert_eq!(ordered_at_hand(&h, B)[0], 77, "starting with the entry that was stuck");
}

#[test]
fn re_proposing_against_a_healthy_run_orders_nothing_twice() {
    // The threshold can be generous and wrong without being unsafe, which is the reason the replica
    // does not try to tell why a decision has not arrived. Here it is deliberately far too short —
    // shorter than a round trip — so every slot is re-proposed while its consensus is still
    // running, and the leader's `∄c'` guard is what makes that harmless.
    let config = synchronous(7).sessions();
    let mut s: Sim<Replica> = Sim::new(config, &FIVE, |me| {
        MultiPaxosReplica::new(me, FIVE, timing()).with_repropose_after(Duration::from_millis(1))
    });
    s.record_notes();
    s.deliver_session_events();
    for (i, node) in FIVE.iter().enumerate() {
        s.command(*node, Cmd::Append(i as u32 + 1));
    }
    settle(&mut s);

    let repeats: usize = FIVE
        .iter()
        .map(|n| {
            notes_at(&s, *n).iter().filter(|x| matches!(x, Note::SlotReproposed { .. })).count()
        })
        .sum();
    assert!(repeats > 5, "only {repeats} re-proposals, so the harmlessness is untested");

    let first = ordered_at(&s, A);
    assert_eq!(first.len(), FIVE.len(), "everything must still be ordered exactly once: {first:?}");
    for node in FIVE {
        assert_eq!(ordered_at(&s, node), first, "{node} disagreed under re-proposal");
    }
}

// ---------------------------------------------------------------- task 6: the port

#[test]
fn a_read_at_a_process_whose_slot_is_undecided_returns_the_shorter_prefix() {
    // The port's own claim: a read is served from the reading process's own copy and says so rather
    // than waiting. Here B is cut off from the decision for a slot A has already applied.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    h.append(A, 5);
    h.settle(|_, to, msg| {
        !(to == B && matches!(msg, Wire::Synod(SynodMsg::Decision { .. })))
            && !(to == A && matches!(msg, Wire::Detector(_)))
    });

    h.event(A, Event::Cmd(Cmd::Read { from: Position::START }));
    h.event(B, Event::Cmd(Cmd::Read { from: Position::START }));
    assert_eq!(reads_at_hand(&h, A), vec![vec![5]], "A serves what it applied");
    assert_eq!(reads_at_hand(&h, B), vec![Vec::<u32>::new()], "B serves the shorter prefix");
    assert!(!ordered_at_hand(&h, A).is_empty(), "and A really had something to be ahead by");
}

#[test]
fn the_survivors_keep_ordering_after_a_minority_crashes_for_good() {
    // Crash-stop, which is what this module claims: a crashed replica is not restarted, because a
    // returning one would report a shortened sequence and a total order that shortens is not one.
    // Two of five go, and the survivors must order something appended *after* they went.
    let mut s = sim_of(&FIVE, synchronous(8));
    s.command(A, Cmd::Append(1));
    settle(&mut s);
    let before = ordered_at(&s, A);
    assert_eq!(before, vec![1], "something must be ordered before the crash");

    s.crash(D);
    s.crash(E);
    assert!(
        s.is_stopped(D) && s.is_stopped(E),
        "the crashes really happened, and before the append that depends on them",
    );

    s.command(A, Cmd::Append(2));
    settle(&mut s);
    let survivors = [A, B, C];
    for node in survivors {
        let seq = ordered_at(&s, node);
        assert!(seq.contains(&2), "{node} never ordered the append after the crash: {seq:?}");
    }
    let seqs: Vec<Vec<u32>> = survivors.iter().map(|n| ordered_at(&s, *n)).collect();
    for seq in &seqs {
        assert_eq!(*seq, seqs[0], "the survivors diverged: {seqs:?}");
    }
}

// ---------------------------------------------------------------- task 7: scope and space

#[test]
fn a_session_ending_reaches_the_layer_above() {
    // This layer bridges nothing: its redundancy is the child's, and the child's is the other
    // processes rather than anything that outlives a session. So the ending is propagated.
    let mut s = sim_of(&FIVE, synchronous(9));
    s.command(E, Cmd::Append(1));
    s.run_for(Duration::from_millis(300));
    s.break_session(E, A);
    s.deliver_session_events();
    s.run_for(Duration::from_millis(300));

    let ended =
        s.trace().indications().filter(|(_, i)| matches!(i, Ind::SessionEnded { .. })).count();
    assert!(ended > 0, "a session ended and nothing above was told — the cardinal sin");
    let established = s
        .trace()
        .indications()
        .filter(|(_, i)| matches!(i, Ind::SessionEstablished { .. }))
        .count();
    assert!(established > 0, "an establishment is the moment a resend is possible; it must arrive");
}

#[test]
fn the_send_rate_is_flat_once_the_work_is_done() {
    // The transcription's space statement, checked where the trace can check it. The collections do
    // grow with commands handled — that is the page, and the module says so — but nothing re-sends
    // more as time passes. The re-proposal timer is the thing that could make it not: it sweeps for
    // ever, and it must find nothing once every slot is decided.
    let mut s = sim_of(&FIVE, synchronous(10));
    for (i, node) in FIVE.iter().enumerate() {
        s.command(*node, Cmd::Append(i as u32));
    }
    settle(&mut s);
    assert_eq!(ordered_at(&s, A).len(), FIVE.len(), "decided first, so the rate measured is idle");
    assert_eq!(s.at(A).outstanding(), 0, "and nothing is outstanding for the sweep to find");
    common::assert_send_rate_flat!(s, Duration::from_millis(400), 4);
}

#[test]
fn the_state_grows_with_commands_and_the_module_says_so() {
    // A transcription measures what it does not bound, so that converting it later has a number to
    // beat. `decisions` is one entry per slot decided, and nothing collects it: §4.2's watermark is
    // the change that does.
    let mut s = sim_of(&FIVE, synchronous(11));
    for i in 0..4u32 {
        s.command(A, Cmd::Append(i));
    }
    settle(&mut s);
    let small = s.at(A).decisions_held();

    for i in 0..12u32 {
        s.command(A, Cmd::Append(100 + i));
    }
    settle(&mut s);
    let large = s.at(A).decisions_held();

    assert!(
        large > small,
        "decisions must grow with commands handled — it is unbounded, and the module says so",
    );
    assert_eq!(large, 16, "one entry per slot decided, and sixteen commands were appended");
    assert_eq!(s.at(A).slot_out() as usize - 1, large, "and every one of them was applied");
}

// ---------------------------------------------------------------- task 4.2: garbage collection

/// What every process is holding, so a bound can be asserted over all of them rather than one.
fn held(s: &Sim<Replica>) -> Vec<(usize, usize, usize)> {
    FIVE.iter()
        .filter(|n| !s.is_stopped(**n))
        .map(|n| {
            let r = s.at(*n);
            (r.synod().accepted_count(), r.synod().decided_count(), r.decisions_held())
        })
        .collect()
}

#[test]
fn state_does_not_grow_with_the_slots_handled() {
    // The claim `docs/bounded-space.md` marked ❌ and §4.2 fixes, asserted rather than described.
    // Two runs, one four times the other: what each process holds must be bounded by the same
    // figure in both, rather than by the slots decided.
    let measure = |entries: u32, seed: u64| {
        let mut s = sim_of(&FIVE, synchronous(seed));
        for i in 0..entries {
            s.command(A, Cmd::Append(i));
        }
        settle(&mut s);
        settle(&mut s);
        assert_eq!(ordered_at(&s, A).len(), entries as usize, "everything must be ordered first");
        // Non-vacuity, and it is the one that matters: a bound holds trivially over a run that
        // never collected. Assert the watermark moved before asserting what it bounded.
        assert!(
            s.at(A).synod().collected_below() > 0,
            "nothing was collected, so the bound below is satisfied by a run that did not collect",
        );
        held(&s)
    };
    let small = measure(10, 111);
    let large = measure(40, 111);

    let worst = |v: &[(usize, usize, usize)]| {
        v.iter().map(|(a, d, _)| *a.max(d)).max().expect("five processes")
    };
    assert!(
        worst(&large) <= worst(&small).max(FIVE.len()),
        "the consensus state grew with the slots handled: {} at ten entries, {} at forty — \
         {small:?} against {large:?}",
        worst(&small),
        worst(&large),
    );
}

#[test]
fn collection_stalls_when_too_few_members_remain_to_report() {
    // §4.2's own caveat: "if there are fewer than 2f + 1 replicas, the crash of f replicas would
    // leave fewer than f + 1 replicas to send periodic updates and no garbage collection could be
    // done". A run that stops collecting after `f` crashes is behaving as specified. This test
    // exists so that nobody later reads the stall as a defect and 'fixes' it.
    //
    // Five members, so `f` is two and `f + 1` is three. Crash three, leaving two — one short.
    let mut s = sim_of(&FIVE, synchronous(112));
    for i in 0..6u32 {
        s.command(A, Cmd::Append(i));
    }
    settle(&mut s);
    let collected_before = s.at(A).synod().collected_below();
    assert!(collected_before > 0, "collection must be working before it is taken away");

    s.crash(C);
    s.crash(D);
    s.crash(E);
    assert!(
        s.is_stopped(C) && s.is_stopped(D) && s.is_stopped(E),
        "the crashes really happened, and before the assertion that depends on them",
    );
    settle(&mut s);
    settle(&mut s);

    assert_eq!(
        s.at(A).synod().collected_below(),
        collected_before,
        "with two of five reporting, the watermark cannot move — and must not",
    );
}

#[test]
fn the_duplicate_filter_is_bounded_by_the_retention_window() {
    // §4.2's *other* half, and a different mechanism from the watermark beneath: the decisions a
    // replica keeps to filter duplicates are bounded by time — here by slots — because no watermark
    // bounds them. `f + 1` replicas having applied up to a slot says nothing about whether a
    // command decided below it may be decided again above it.
    const RETAIN: Slot = 4;
    let config = synchronous(113).sessions();
    let mut s: Sim<Replica> = Sim::new(config, &FIVE, |me| {
        MultiPaxosReplica::new(me, FIVE, timing()).with_retain(RETAIN)
    });
    s.record_notes();
    s.deliver_session_events();

    for i in 0..20u32 {
        s.command(A, Cmd::Append(i));
    }
    settle(&mut s);
    settle(&mut s);
    assert_eq!(ordered_at(&s, A).len(), 20, "everything must be ordered");

    for node in FIVE {
        let r = s.at(node);
        assert!(
            r.decisions_held() as u64 <= RETAIN + 1,
            "{node} kept {} decisions against a window of {RETAIN}",
            r.decisions_held(),
        );
    }
    // And the sequence is **not** collected — it is the data, not the bookkeeping.
    assert_eq!(
        s.at(A).len(),
        20,
        "the ordered sequence must survive the window: it is the log, and a log that discarded \
         its entries would not be one",
    );
}

#[test]
fn the_ordered_sequence_is_read_back_in_full_after_collection() {
    // The exemption, end to end and through the port rather than through an accessor: a reader asks
    // for the sequence from the start long after everything beneath it has been collected.
    let mut s = sim_of(&FIVE, synchronous(114));
    for i in 0..30u32 {
        s.command(A, Cmd::Append(i));
    }
    settle(&mut s);
    settle(&mut s);
    assert!(s.at(A).synod().collected_below() > 0, "the run must have collected");

    s.command(A, Cmd::Read { from: Position::START });
    s.step_now();
    let read = s
        .trace()
        .indications_at(A)
        .filter_map(|i| match i {
            Ind::Contents { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .last()
        .expect("the read must be answered");
    assert_eq!(read.len(), 30, "a read after collection must still serve the whole sequence");
    assert_eq!(read, (0..30u32).collect::<Vec<_>>(), "and in order");
}

#[test]
fn a_replica_stranded_by_a_collection_catches_up_from_a_peer() {
    // **The case that made replica-to-replica transfer part of this change rather than a later
    // one.** Collecting at `f + 1` means `f` correct replicas may be behind, and everything that
    // could have helped them is exactly what was collected: the consensus layer no longer holds the
    // decision, and the leader's answer to a re-proposal needs that record.
    //
    // §4.2's own justification is the remedy: "replicas can learn decisions, and the application
    // state that results from those decisions, from one another". Without it a correct process that
    // missed one decision is stranded for ever, which is what this drove before the transfer
    // existed — measured at `B slot_out=1` with the leader holding zero decisions.
    let drop_decisions_to_b = |_: NodeId, to: NodeId, msg: &Msg| {
        !(to == B && matches!(msg, Wire::Synod(SynodMsg::Decision { .. })))
    };
    let mut h = Hand::new(&THREE);
    h.settle_until_active(C);

    // B appends, and is the one process never told the answer.
    h.append(B, 77);
    h.pump(2, drop_decisions_to_b);
    assert!(ordered_at_hand(&h, B).is_empty(), "B was never told — the precondition");

    // A and C run on and apply past it, so two of three is `f + 1` and everything for B's slot is
    // collected everywhere — including at the leader that would otherwise have answered B.
    for v in 0..8u32 {
        h.append(C, 200 + v);
    }
    h.pump(30, drop_decisions_to_b);
    assert!(
        h.at(C).synod().collected_below() > 1,
        "the leader must have collected past B's slot, or the strand does not arise",
    );
    assert_eq!(
        h.at(C).synod().decided_count(),
        0,
        "and must hold no decision record — which is what used to leave B with nowhere to ask",
    );
    assert!(ordered_at_hand(&h, B).is_empty(), "B is still stranded at this point");

    // Now let everything through. B's re-proposal is refused as collected, it asks a peer, and the
    // peer teaches it from the decisions the *replica* still holds.
    h.pump(40, |_, _, _| true);
    assert_eq!(
        ordered_at_hand(&h, B).first(),
        Some(&77),
        "B must catch up from a peer: {:?}",
        ordered_at_hand(&h, B),
    );
    assert!(
        h.notes_at(B).any(|n| matches!(n, Note::CaughtUpFrom { .. })),
        "and it must be the catch-up that did it, not luck",
    );
    // And it catches up in full, not just past the one slot it missed — converging on what the
    // process that never fell behind holds.
    h.pump(40, |_, _, _| true);
    assert_eq!(
        ordered_at_hand(&h, B),
        ordered_at_hand(&h, C),
        "B must converge on the leader's sequence, not merely unstick",
    );
    assert!(ordered_at_hand(&h, B).len() >= 9, "and that sequence must be the whole run's");
}

// ---------------------------------------------------------------- the hand-driven harness

/// Several processes, one timer-identity source, and a wire the test decides what to do with.
///
/// The same instrument as the Synod suite's, pointed at the layer above: the simulator delivers or
/// loses by its own rules, which is right for breadth and wrong for "lose exactly this decision and
/// nothing else".
struct Hand {
    nodes: BTreeMap<NodeId, Replica>,
    stores: BTreeMap<NodeId, MemStore<Infallible, Infallible>>,
    timers: BTreeMap<NodeId, Vec<TimerId>>,
    wire: Vec<(NodeId, NodeId, Msg)>,
    /// What `settle` refused to deliver. A test that drops one message needs to know the message
    /// was really there to drop, or the precondition it set up is that nothing was ever sent.
    dropped: Vec<(NodeId, NodeId, Msg)>,
    inds: Vec<(NodeId, Ind<u32>)>,
    notes: Vec<(NodeId, Note)>,
    rng: ChaCha8Rng,
    ids: u64,
    now: Time,
}

impl Hand {
    fn new(members: &'static [NodeId]) -> Self {
        let mut h = Hand {
            nodes: members.iter().map(|&n| (n, replica_over(members)(n))).collect(),
            stores: members.iter().map(|&n| (n, MemStore::default())).collect(),
            timers: members.iter().map(|&n| (n, Vec::new())).collect(),
            wire: Vec::new(),
            dropped: Vec::new(),
            inds: Vec::new(),
            notes: Vec::new(),
            rng: ChaCha8Rng::seed_from_u64(11),
            ids: 0,
            now: Time::ZERO,
        };
        for &n in members {
            h.event(n, Event::Init);
        }
        h
    }

    fn event(&mut self, node: NodeId, event: Event<Cmd<u32>, Msg, SessionEvent>) {
        let proto = self.nodes.get_mut(&node).expect("a member");
        let store = self.stores.get_mut(&node).expect("a member");
        // One identity source for the whole harness: two layers each starting at zero would each
        // accept the other's expiry, which is exactly what one of these tests is about.
        let mut narrated: Vec<Note> = Vec::new();
        let effects =
            step_noting(proto, event, self.now, &mut self.rng, store, &mut self.ids, &mut narrated);
        for e in effects {
            match e {
                Effect::Send { to, msg } => self.wire.push((node, to, msg)),
                Effect::Indicate(ind) => self.inds.push((node, ind)),
                Effect::SetTimer { id, .. } => {
                    self.timers.get_mut(&node).expect("a member").push(id)
                }
            }
        }
        for n in narrated {
            self.notes.push((node, n));
        }
    }

    fn append(&mut self, node: NodeId, value: u32) {
        self.event(node, Event::Cmd(Cmd::Append(value)));
    }

    fn notes_at(&self, node: NodeId) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(move |(n, _)| *n == node).map(|(_, note)| note)
    }

    /// Every handle this node currently holds, taken: firing one is the caller's business.
    fn take_timers(&mut self, node: NodeId) -> Vec<TimerId> {
        core::mem::take(self.timers.get_mut(&node).expect("a member"))
    }

    /// Fire one handle, and hand back whatever the process registered while handling it — which
    /// is the same layer's next handle, since a layer re-arms in its own expiry.
    fn fire(&mut self, node: NodeId, id: TimerId) -> Vec<TimerId> {
        let before = self.timers.get(&node).map_or(0, Vec::len);
        self.event(node, Event::Timer(id));
        self.timers.get(&node).expect("a member")[before..].to_vec()
    }

    /// Fire every timer this node holds. The protocol compares before acting, so handing it all of
    /// them is what a driver does.
    fn tick(&mut self, node: NodeId) {
        let ids = self.timers.get(&node).cloned().unwrap_or_default();
        self.timers.get_mut(&node).expect("a member").clear();
        for id in ids {
            self.event(node, Event::Timer(id));
        }
    }

    fn tick_all(&mut self) {
        let nodes: Vec<NodeId> = self.nodes.keys().copied().collect();
        for n in nodes {
            self.tick(n);
        }
    }

    /// Deliver everything in flight that `keep` accepts, and discard the rest. Runs to a fixed
    /// point, because a delivery emits sends of its own.
    fn settle(&mut self, keep: impl Fn(NodeId, NodeId, &Msg) -> bool) {
        for _ in 0..64 {
            if self.wire.is_empty() {
                return;
            }
            let batch = core::mem::take(&mut self.wire);
            for (from, to, msg) in batch {
                if keep(from, to, &msg) {
                    self.event(to, Event::Msg { from, msg });
                } else {
                    self.dropped.push((from, to, msg));
                }
            }
        }
    }

    /// Deliver the algorithm's traffic but starve one process of heartbeats, so it keeps trusting
    /// itself. The simulator cannot do this — one wire, one link.
    fn settle_without_heartbeats_to(&mut self, starved: NodeId) {
        self.settle(|_, to, msg| !(to == starved && matches!(msg, Wire::Detector(_))));
    }

    fn advance(&mut self, d: Duration) {
        self.now += d;
    }

    fn at(&self, node: NodeId) -> &Replica {
        self.nodes.get(&node).expect("a member")
    }

    /// The Synod messages in flight from `from` to `to`. Heartbeats share the wire and are never
    /// what a test about the algorithm means.
    fn synod_from(&self, from: NodeId, to: NodeId) -> Vec<SynodMsg<Command<u32>>> {
        self.wire
            .iter()
            .filter(|(f, t, _)| *f == from && *t == to)
            .filter_map(|(_, _, m)| match m {
                Wire::Synod(s) => Some(s.clone()),
                Wire::Detector(_) => None,
            })
            .collect()
    }

    /// Deliver one message a test took off the wire, so it can choose the order — or deliver the
    /// same one twice.
    fn deliver(&mut self, to: NodeId, (from, msg): (NodeId, Msg)) {
        self.event(to, Event::Msg { from, msg });
    }

    /// Advance, fire every timer and deliver what `keep` accepts, `rounds` times. Heartbeats keep
    /// flowing, which is what stops a long advance turning every process into a self-trusting
    /// leader — the failure a single big `advance` produces and this exists to avoid.
    fn pump(&mut self, rounds: usize, keep: impl Fn(NodeId, NodeId, &Msg) -> bool + Copy) {
        for _ in 0..rounds {
            self.advance(timing().heartbeat);
            self.tick_all();
            self.settle(keep);
        }
    }

    /// Drive the run until `node` — the highest rank, so the one Ω trusts from the start — has
    /// completed phase one. Nothing is starved, so every process agrees who leads.
    fn settle_until_active(&mut self, node: NodeId) {
        for _ in 0..20 {
            self.pump(1, |_, _, _| true);
            if self.at(node).synod().is_active() {
                return;
            }
        }
        panic!("{node} never became an active leader");
    }

    /// Drive `node` to the point where Ω trusts it and phase one has completed. Starving it of
    /// heartbeats is what makes it trust itself.
    fn make_active_leader(&mut self, node: NodeId) {
        for _ in 0..8 {
            self.advance(timing().detect_after);
            self.tick_all();
            self.settle_without_heartbeats_to(node);
            if self.at(node).synod().is_trusted() && self.at(node).synod().is_active() {
                return;
            }
        }
        panic!("{node} never became an active leader");
    }
}

fn ordered_at_hand(h: &Hand, node: NodeId) -> Vec<u32> {
    h.inds
        .iter()
        .filter(|(n, _)| *n == node)
        .filter_map(|(_, i)| match i {
            Ind::Ordered { value, .. } => Some(*value),
            _ => None,
        })
        .collect()
}

fn reads_at_hand(h: &Hand, node: NodeId) -> Vec<Vec<u32>> {
    h.inds
        .iter()
        .filter(|(n, _)| *n == node)
        .filter_map(|(_, i)| match i {
            Ind::Contents { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .collect()
}

mod common;
