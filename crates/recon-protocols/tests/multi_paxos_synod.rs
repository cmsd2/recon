//! The Synod protocol of *Paxos Made Moderately Complex* §2, against its own guarantees.
//!
//! Safety here is a property over a whole run rather than an assertion at a point: what has to
//! hold is that no *pair* of observers disagrees about a slot, which no single process's state can
//! say. So the suite carries a checker fed from the trace ([`check`]) and runs it after **every**
//! event, so that a violation names the first event that broke it and the seed replays it.
//!
//! Two sources of schedule, and both are needed. [`sweep_over_seeds`] runs the properties across a
//! batch of seeds with loss, duplication and reordering on, which finds the interleavings nobody
//! thought of. The hand-driven schedules below cover the edges randomness rarely lands on —
//! adoption at exactly the majority and not one fewer, a leader learning what a lost preemption
//! would have told it, an acceptor that never saw phase one answering phase two. Neither
//! substitutes for the other.
//!
//! The safety tests are also registered in `scripts/check-safety-tests.sh`, which compiles two
//! mutations of the module and requires every one of them to go red. Agreement admits the same
//! silent substitution durability did — a run with one settled leader satisfies it whatever the
//! code does — which is the case that guard exists for.

use core::convert::Infallible;
use core::time::Duration;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use recon_core::{Effect, Event, MemStore, NodeId, Time, TimerId, step_noting};
use recon_protocols::multi_paxos_synod::{
    Ballot, Cmd, Ind, MultiPaxosSynod, Pvalue, Slot, SynodMsg, Wire,
};
use recon_protocols::session_link::SessionLink;
use recon_protocols::{Note, Timing};
use recon_sim::{Config, Sim};
use std::collections::{BTreeMap, BTreeSet};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const C: NodeId = NodeId::new(3);
const D: NodeId = NodeId::new(4);
const E: NodeId = NodeId::new(5);
const FIVE: [NodeId; 5] = [A, B, C, D, E];
const THREE: [NodeId; 3] = [A, B, C];

/// The network's promise; everything else is derived from it.
const BOUND: Duration = Duration::from_millis(20);

fn timing() -> Timing {
    Timing { retransmit: Duration::from_millis(10), heartbeat: BOUND * 2, detect_after: BOUND * 6 }
}

/// How long an attempt goes unanswered before its requests are sent again — half the escalation, as
/// the module derives it. Not `Timing::retransmit`, which is only how often the question is asked.
fn resend_after() -> Duration {
    timing().detect_after / 2
}

type Synod = MultiPaxosSynod<u32, SessionLink<SynodMsg<u32>>>;
type Msg = Wire<SynodMsg<u32>>;

fn synod_over(members: &'static [NodeId]) -> impl Fn(NodeId) -> Synod {
    move |me| MultiPaxosSynod::new(me, members.iter().copied(), timing())
}

fn sim_of(members: &'static [NodeId], config: Config) -> Sim<Synod> {
    let mut s: Sim<Synod> = Sim::new(config.sessions(), members, synod_over(members));
    s.deliver_session_events();
    s
}

fn synchronous(seed: u64) -> Config {
    Config::default().seed(seed).synchronous(BOUND).max_steps(2_000_000)
}

/// A network that varies in delay, and whose **sessions break**.
///
/// `Config::loss` is deliberately not set: in session mode the simulator applies no loss,
/// duplication or reordering at all, because that is what a session is — reliable and ordered
/// while it holds. Setting those knobs here would produce a perfectly clean run wearing an
/// adversarial name, which is the vacuity this repository keeps finding. The way to lose a message
/// under a session link is to end the session, so [`churn`] is what makes these runs hostile, and
/// every test using it asserts that sessions really did end.
fn unreliable(seed: u64) -> Config {
    Config::default().seed(seed).latency(Duration::from_millis(1), BOUND).max_steps(2_000_000)
}

/// Break sessions while the run proceeds, checking after every event.
///
/// A session ending is the only way this stack loses a message, so this is the fault injection the
/// suite spends. The pairs are walked deterministically from `seed` so a failure replays.
fn churn(sim: &mut Sim<Synod>, seed: u64, breaks: usize, between: Duration) -> Checked {
    let pairs: Vec<(NodeId, NodeId)> = FIVE
        .iter()
        .flat_map(|a| FIVE.iter().map(move |b| (*a, *b)))
        .filter(|(a, b)| a < b)
        .collect();
    let mut checked = check(sim);
    for i in 0..breaks {
        let (a, b) = pairs[(seed as usize + i * 7) % pairs.len()];
        if sim.has_session(a, b) {
            sim.break_session(a, b);
            sim.deliver_session_events();
        }
        checked = run_checking(sim, between);
    }
    checked
}

// ---------------------------------------------------------------- the checker (task 7.5)

/// What the trace says happened, reduced to the four things safety is made of.
///
/// Fed from the trace rather than from protocol state, because the property is that no *pair* of
/// observers disagrees — which is not visible from inside any one of them.
#[derive(Debug, Default)]
struct Checked {
    /// Per acceptor, the highest ballot it has been seen to hold. A1: this only rises.
    promise: BTreeMap<NodeId, Ballot>,
    /// Per `⟨ballot, slot⟩`, the command proposed under it. C1 and A4: there is at most one.
    commanded: BTreeMap<(Ballot, Slot), u32>,
    /// Per slot, what each process learned was chosen. S1: they agree.
    chosen: BTreeMap<Slot, BTreeMap<NodeId, u32>>,
    /// Every `⟨slot, command⟩` any process was asked to propose. S2 is checked against this.
    proposed: BTreeSet<(Slot, u32)>,
}

/// Reduce the whole trace, then assert S1, S2, A1 and C1 over it.
///
/// Cheap enough to run after every single event, which is the point: a violation then names the
/// first event that broke it rather than the end of the run.
///
/// **A2 is not among these, and cannot be.** "An acceptor accepts only under the ballot it holds"
/// is a claim about internal state at the instant of the acceptance; the trace holds the `p2b` that
/// followed, whose ballot the acceptor's own reply construction makes `≥` the request regardless.
/// So a violation of A2 leaves no signature here. Its evidence is the mutation
/// `synod-accept-below-promise` in `scripts/check-safety-tests.sh`, and the schedules registered
/// against it — the same shape as the total-order log asserting a non-vacuity floor where the
/// direct property is invisible to the trace.
///
/// **Reading the trace rather than acceptor state is also what makes this checker survive §4.1.**
/// That reduction has an acceptor keep only the highest-ballot pvalue per slot, so a majority can
/// stop holding a proposal that was nevertheless chosen — the paper's own worrisome effect, quoted
/// in the module. A checker that reduced acceptor state would read that as a slot losing its
/// choice and fail a correct run. This one reads what was sent and what was indicated, neither of
/// which the reduction touches. `agreement_survives_the_record_of_it_being_overwritten` is where
/// the case itself is driven, and it asserts over acceptor state deliberately, to pin that the
/// evidence really is gone.
fn check(sim: &Sim<Synod>) -> Checked {
    let mut c = Checked::default();
    for (_, _, cmd) in sim.trace().invocations() {
        let Cmd::Propose { slot, command } = cmd;
        c.proposed.insert((*slot, *command));
    }
    // Sends rather than deliveries: what an acceptor put on the wire is what it held at the time,
    // whether or not anything received it. **Exchanges** rather than network sends, because an
    // acceptor that is also the leader answers itself, and its promise is as much a fact then as
    // when it answers a peer.
    for (from, _, msg) in sim.trace().exchanges() {
        match msg {
            Wire::Synod(SynodMsg::P1b { ballot, .. })
            | Wire::Synod(SynodMsg::P2b { ballot, .. }) => {
                // A1, restated as the acceptor's own promise: an acceptor takes up strictly
                // increasing ballots, so what it reports never goes backwards.
                let held = c.promise.entry(from).or_insert(*ballot);
                assert!(
                    *ballot >= *held,
                    "an acceptor's promise went backwards: {from} held {held} and then reported \
                     {ballot}",
                );
                *held = *ballot;
            }
            // A4 and C1 together: at most one command is selected per ballot and slot, and it is
            // the leader that enforces it. Two different commands under one ⟨b, s⟩ would let two
            // majorities accept different values at the same ballot.
            Wire::Synod(SynodMsg::P2a { pvalue }) => {
                let Pvalue { ballot, slot, command } = pvalue;
                let already = c.commanded.entry((*ballot, *slot)).or_insert(*command);
                assert_eq!(
                    already, command,
                    "two commands proposed under one ballot and slot ({ballot}, {slot}): \
                     {already} and {command} — Invariant C1",
                );
            }
            _ => {}
        }
    }
    for (node, ind) in sim.trace().indications() {
        if let Ind::Decision { slot, command } = ind {
            // S2: a chosen proposal is one some process proposed.
            assert!(
                c.proposed.contains(&(*slot, *command)),
                "{node} learned {command} chosen for slot {slot}, which nobody proposed",
            );
            let learners = c.chosen.entry(*slot).or_default();
            for (other, theirs) in learners.iter() {
                // S1: at most one proposal is ever chosen for a slot.
                assert_eq!(
                    theirs, command,
                    "slot {slot} was split: {other} learned {theirs} and {node} learned {command}",
                );
            }
            learners.insert(node, *command);
        }
    }
    c
}

/// Run the sim one event at a time, checking after each. Returns the final reduction.
fn run_checking(sim: &mut Sim<Synod>, until: Duration) -> Checked {
    let deadline = sim.now() + until;
    let mut last = check(sim);
    while sim.now() < deadline {
        if !sim.step() {
            break;
        }
        last = check(sim);
    }
    last
}

fn decisions(sim: &Sim<Synod>) -> BTreeMap<Slot, u32> {
    let mut out = BTreeMap::new();
    for (_, ind) in sim.trace().indications() {
        if let Ind::Decision { slot, command } = ind {
            out.insert(*slot, *command);
        }
    }
    out
}

/// How many times a slot was announced as decided, across every process. More than once is the
/// normal case once a later ballot re-commands it, and what A5 makes harmless.
fn announcements(sim: &Sim<Synod>, want: Slot) -> usize {
    sim.trace()
        .indications()
        .filter(|(_, ind)| matches!(ind, Ind::Decision { slot, .. } if *slot == want))
        .count()
}

/// Did the run really contain a preemption? An acceptor answering with a ballot above the one it
/// was asked about is the only thing that produces one, so it is what the trace can be asked.
fn preemptions(sim: &Sim<Synod>) -> usize {
    let mut asked: BTreeMap<(NodeId, NodeId), Ballot> = BTreeMap::new();
    let mut n = 0;
    // `exchanges`, not `sends`: a leader is one of its own acceptors, so it refuses its own ballot
    // as readily as a peer's, and that hand-off is not a network message. Reading only the network
    // makes this return zero for runs that contained several competing ballots — measured, when
    // the hand-off was first written inside the protocol where the trace could not see it.
    for (from, to, msg) in sim.trace().exchanges() {
        match msg {
            Wire::Synod(SynodMsg::P1a { ballot }) => {
                asked.insert((from, to), *ballot);
            }
            Wire::Synod(SynodMsg::P2a { pvalue }) => {
                asked.insert((from, to), pvalue.ballot);
            }
            Wire::Synod(SynodMsg::P1b { ballot, .. })
            | Wire::Synod(SynodMsg::P2b { ballot, .. })
                if asked.get(&(to, from)).is_some_and(|requested| ballot > requested) =>
            {
                n += 1;
            }
            _ => {}
        }
    }
    n
}

/// How many distinct ballots were put on the wire. More than one means the run really contained
/// competing ballots rather than one leader having its way.
fn ballots_seen(sim: &Sim<Synod>) -> BTreeSet<Ballot> {
    sim.trace()
        .exchanges()
        .filter_map(|(_, _, msg)| match msg {
            Wire::Synod(SynodMsg::P1a { ballot }) => Some(*ballot),
            Wire::Synod(SynodMsg::P2a { pvalue }) => Some(pvalue.ballot),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------- what a run costs

/// Every message a run put on the wire, counted by kind.
///
/// Fed from the trace, and *not* from a total: what a run costs is a different number per kind, and
/// an average over all of them hides the one thing worth showing — that phase one is paid per
/// leadership change and phase two per entry.
#[derive(Debug, Default, PartialEq, Eq)]
struct Cost {
    p1a: usize,
    p1b: usize,
    p2a: usize,
    p2b: usize,
    decision: usize,
    propose: usize,
    /// The detector's, which are per tick rather than per entry — see [`Cost::of`].
    heartbeat: usize,
    /// Messages a process addressed to itself. A deployment has none: the roles are co-located, so
    /// a leader reaching its own acceptor is a function call.
    self_addressed: usize,
}

impl Cost {
    /// What the run has spent so far.
    ///
    /// **The heartbeats are counted and never asserted against the work done**, and that is worth
    /// saying rather than leaving a reader to wonder why the totals do not add up. Ω is a failure
    /// detector: it sends on a timer whether or not anything is happening, so its cost is a
    /// function of how long a run lasts and not of what the run achieved. This capability therefore
    /// cannot make the claim `an_idle_gossip_sends_nothing` makes for the gossip pair, and the
    /// reason is the detector rather than the consensus.
    fn of(sim: &Sim<Synod>) -> Cost {
        let mut c = Cost::default();
        for (from, to, msg) in sim.trace().sends() {
            if from == to {
                c.self_addressed += 1;
            }
            match msg {
                Wire::Detector(_) => c.heartbeat += 1,
                Wire::Synod(SynodMsg::P1a { .. }) => c.p1a += 1,
                Wire::Synod(SynodMsg::P1b { .. }) => c.p1b += 1,
                Wire::Synod(SynodMsg::P2a { .. }) => c.p2a += 1,
                Wire::Synod(SynodMsg::P2b { .. }) => c.p2b += 1,
                Wire::Synod(SynodMsg::Decision { .. }) => c.decision += 1,
                Wire::Synod(SynodMsg::Propose { .. }) => c.propose += 1,
            }
        }
        c
    }

    /// What this run has spent since `earlier`.
    fn since(&self, earlier: &Cost) -> Cost {
        Cost {
            p1a: self.p1a - earlier.p1a,
            p1b: self.p1b - earlier.p1b,
            p2a: self.p2a - earlier.p2a,
            p2b: self.p2b - earlier.p2b,
            decision: self.decision - earlier.decision,
            propose: self.propose - earlier.propose,
            heartbeat: self.heartbeat - earlier.heartbeat,
            self_addressed: self.self_addressed - earlier.self_addressed,
        }
    }
}

/// How many phase-one exchanges the run completed — a leadership change, counted from the trace
/// rather than assumed, since the per-entry figure means nothing until phase one is attributed
/// separately.
fn adoptions(sim: &Sim<Synod>) -> usize {
    sim.trace()
        .sends()
        .filter(|(_, _, m)| matches!(m, Wire::Synod(SynodMsg::P1a { .. })))
        .map(|(_, _, m)| match m {
            Wire::Synod(SynodMsg::P1a { ballot }) => *ballot,
            _ => unreachable!(),
        })
        .collect::<BTreeSet<Ballot>>()
        .len()
}

/// Decide `entries` slots at `leader` over a settled run, and report what phase two cost.
///
/// Leadership is settled *first* and the cost taken from that point, so phase one's own messages
/// are not charged to the entries.
fn phase_two_cost(members: &'static [NodeId], entries: u64, seed: u64) -> (Cost, usize) {
    let mut s = sim_of(members, synchronous(seed));
    let leader = members[members.len() - 1];
    s.run_for(Duration::from_millis(600));
    assert!(s.at(leader).is_active(), "leadership must settle before the entries are counted");
    let settled = Cost::of(&s);
    let settled_adoptions = adoptions(&s);

    for slot in 1..=entries {
        s.command(leader, Cmd::Propose { slot, command: slot as u32 });
    }
    s.run_for(Duration::from_millis(1500));
    assert_eq!(decisions(&s).len(), entries as usize, "every slot must be decided");
    assert_eq!(
        adoptions(&s),
        settled_adoptions,
        "leadership must not change while the entries are counted, or phase one is charged to them",
    );
    (Cost::of(&s).since(&settled), members.len())
}

#[test]
fn phase_two_costs_one_exchange_per_acceptor_per_entry_and_one_decision_to_everyone_else() {
    // The identity, asserted **exactly** rather than as a bound. A bound of `≤ 4n` would pass a run
    // spending three times what it needs, which is what this capability was doing before the
    // retransmission threshold existed.
    for (members, entries) in [(&THREE[..], 6u64), (&FIVE[..], 6)] {
        let members: &'static [NodeId] = if members.len() == 3 { &THREE } else { &FIVE };
        let (cost, n) = phase_two_cost(members, entries, 51);
        let e = entries as usize;
        assert_eq!(
            (cost.p2a, cost.p2b, cost.decision),
            ((n - 1) * e, (n - 1) * e, (n - 1) * e),
            "n={n} entries={e}: phase two is one request and one reply per *other* acceptor per \
             entry, and one decision to every other process — got {cost:?}",
        );
        assert_eq!(cost.p1a, 0, "n={n}: no leadership change, so no phase one — got {cost:?}");
        assert_eq!(cost.p1b, 0, "n={n}: and no answers to one");
        assert_eq!(cost.self_addressed, 0, "n={n}: a process must send itself nothing");
    }
}

#[test]
fn phase_one_is_paid_per_leadership_change_and_not_per_entry() {
    // The whole difference between this capability and one consensus instance per entry. Six
    // entries under one leader must cost the same phase one as one entry does: none at all, beyond
    // the single adoption that made it leader.
    let few = phase_two_cost(&FIVE, 1, 61).0;
    let many = phase_two_cost(&FIVE, 6, 61).0;

    assert_eq!((few.p1a, few.p1b), (0, 0), "one entry pays no phase one of its own");
    assert_eq!((many.p1a, many.p1b), (0, 0), "and neither do six — got {many:?}");
    // Non-vacuity: six entries really were decided, and really cost six times the phase two, or the
    // amortisation above is a statement about a run that did nothing.
    assert_eq!(many.p2a, few.p2a * 6, "six entries must cost six entries' worth of phase two");
    assert!(few.p2a > 0, "and one entry must cost something");
}

#[test]
fn the_whole_of_a_leadership_change_is_one_exchange_per_acceptor() {
    // The other half of the identity: what a leadership change itself costs, which is what the
    // per-entry figure above is amortised against.
    let mut s = sim_of(&FIVE, synchronous(71));
    s.run_for(Duration::from_millis(600));
    assert!(s.at(E).is_active(), "E leads first");
    let cost = Cost::of(&s);
    let n = FIVE.len();

    assert_eq!(adoptions(&s), 1, "exactly one ballot ran, so the count below is one change's");
    assert_eq!(
        (cost.p1a, cost.p1b),
        ((n - 1), (n - 1)),
        "a leadership change is one request and one reply per *other* acceptor — got {cost:?}",
    );
    assert_eq!(cost.self_addressed, 0, "and nothing addressed to the leader itself");
}

#[test]
fn the_resend_threshold_sits_between_a_round_trip_and_the_escalation() {
    // The two bounds the module states, pinned rather than left to whoever configures `Timing`.
    // Below a round trip and a resend goes out for a message still in flight; at or above the
    // escalation and a phase restarts before a retransmission was ever tried.
    let round_trip = BOUND * 2;
    assert!(
        resend_after() > round_trip,
        "the threshold {:?} must exceed one round trip {round_trip:?}, or it resends what is \
         merely in flight",
        resend_after(),
    );
    assert!(
        resend_after() < timing().detect_after,
        "the threshold {:?} must stay below the escalation {:?}, or an attempt escalates before \
         it has been retried",
        resend_after(),
        timing().detect_after,
    );
    // And the sweep is finer than the threshold, or the threshold is not what decides the rate.
    assert!(
        timing().retransmit < resend_after(),
        "the sweep must be finer than the threshold it checks",
    );
}

#[test]
fn the_escalations_still_fire_at_their_own_threshold() {
    // This change is about cost, and the cross-check's liveness fixes are not cost. A lost `p1a`
    // must still restart phase one at the escalation, unchanged by the resend threshold sitting
    // below it.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    h.preempt(A, Ballot { round: 30, leader: C });
    assert!(h.at(A).is_scouting(), "A is in phase one");
    let ballot = h.at(A).leader_ballot();
    h.wire.clear();

    // Every `p1b` lost, so the scout is neither adopted nor preempted. Past the escalation it
    // restarts, which is a fresh fan-out to *every* acceptor rather than to those that owe.
    for _ in 0..20 {
        h.advance(timing().retransmit);
        h.tick(A);
        h.settle(|_, to, msg| {
            !(to == A && matches!(msg, Wire::Synod(SynodMsg::P1b { .. })))
                && !matches!(msg, Wire::Detector(_))
        });
    }
    assert!(h.at(A).is_scouting(), "it must still be scouting rather than having given up");
    assert_eq!(h.at(A).leader_ballot(), ballot, "and at the same ballot — a restart, not a climb");

    // One more escalation's worth, with nothing settled afterwards, so the restart's own fan-out is
    // still in flight to be read. `start_scout` resets the clock, so each escalation earns its own.
    h.wire.clear();
    for _ in 0..14 {
        h.advance(timing().retransmit);
        h.tick(A);
    }
    let asked: Vec<NodeId> = h
        .in_flight()
        .iter()
        .filter(|(from, _, m)| *from == A && matches!(m, Wire::Synod(SynodMsg::P1a { .. })))
        .map(|(_, to, _)| *to)
        .collect();
    assert!(
        asked.len() >= THREE.len(),
        "the escalation restarts phase one against *every* acceptor, not only those that owe — \
         got {asked:?}",
    );
}

// ------------------------------------------------- a message to oneself is not a network message

#[test]
fn nothing_a_process_addresses_to_itself_reaches_the_network() {
    // The roles are co-located — one process holds both the acceptor and the leader — so a leader
    // reaching its own acceptor is a hand-off. The simulator delivers it without the network, and
    // this asserts the consequence over a run that covers **both** phases: entries decided under
    // one leader, and then a leadership change after it crashes.
    let mut s = sim_of(&FIVE, synchronous(81));
    s.run_for(Duration::from_millis(600));
    for slot in 1..=3u64 {
        s.command(E, Cmd::Propose { slot, command: slot as u32 });
    }
    s.run_for(Duration::from_millis(600));
    s.crash(E);
    // Wait for Ω to move before proposing: a process that is not yet trusted forwards to the one
    // that is, which here is the crashed E, and nothing at this layer recovers that — the cost of
    // colocation, which `a_proposal_forwarded_to_a_crashed_process_is_lost` covers.
    for _ in 0..40 {
        s.run_for(Duration::from_millis(100));
        if s.at(D).is_trusted() {
            break;
        }
    }
    assert!(s.at(D).is_trusted(), "the detector must move to D before it can lead");
    for slot in 4..=6u64 {
        s.command(D, Cmd::Propose { slot, command: slot as u32 });
    }
    s.run_for(Duration::from_secs(2));

    assert!(
        s.trace().sends().all(|(from, to, _)| from != to),
        "a process addressed the network to itself",
    );
    // Non-vacuity, and both halves of it: the run really did both phases, and hand-offs really did
    // happen — an assertion that nothing self-addressed is on the network is satisfied by a run in
    // which no process ever addressed itself at all.
    assert!(decisions(&s).len() >= 6, "the run must decide under both leaders");
    assert!(ballots_seen(&s).len() > 1, "and leadership must really have changed");
    assert!(
        s.trace().handed_to_self().count() > 0,
        "and the run must contain hand-offs, or this asserts nothing",
    );
}

#[test]
fn a_leaders_own_acceptor_still_counts_toward_the_majority() {
    // The regression the hand-off most easily causes: a leader that no longer counts itself needs
    // one more remote answer than it should, and on three processes that is the difference between
    // deciding and not. Driven so that the leader's own answer is the one completing the quorum —
    // one peer answers, which is not a majority of three, and the leader's own makes it two.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    h.propose(A, 1, 111);

    // Cut C out entirely, so the only answers are B's and A's own — and keep starving A of
    // heartbeats, or it stops trusting itself and yields mid-schedule.
    h.settle(|from, to, msg| {
        from != C && to != C && !(to == A && matches!(msg, Wire::Detector(_)))
    });
    assert!(
        h.decisions_at(A).contains(&(1, 111)),
        "one peer's answer plus the leader's own is a majority of three, got {:?}",
        h.decisions_at(A),
    );
    // Non-vacuity: C really was silent, so the leader really did count itself.
    assert!(
        h.at(A).accepted_for(1).is_some(),
        "the leader's own acceptor must have accepted, or it was not part of the quorum",
    );
}

#[test]
fn a_hand_off_takes_no_time_where_a_network_message_takes_a_delivery_bound() {
    // The half that is not bookkeeping. A leader used to wait a full delivery bound for its own
    // acceptor's answer, which lengthened both phases and therefore how much the sweep resent.
    // Asserted from the trace rather than from a duration: the hand-off and the delivery it causes
    // are at the same instant, and the network's are not.
    let mut s = sim_of(&THREE, synchronous(83));
    s.run_for(Duration::from_millis(600));

    let handed: Vec<(NodeId, Time)> = s
        .trace()
        .events()
        .iter()
        .filter_map(|e| match e {
            recon_sim::TraceEvent::HandedToSelf { at, node, .. } => Some((*node, *at)),
            _ => None,
        })
        .collect();
    assert!(!handed.is_empty(), "the run must contain hand-offs");

    // Every hand-off is followed by its delivery at the same instant.
    for (node, at) in &handed {
        let delivered_then = s.trace().events().iter().any(|e| match e {
            recon_sim::TraceEvent::Delivered { at: d, from, to, .. } => {
                from == node && to == node && d == at
            }
            _ => false,
        });
        assert!(delivered_then, "{node}'s hand-off at {at:?} was not delivered at that instant");
    }
    // And the contrast: a network message is not, under a synchronous bound.
    let networked = s.trace().events().iter().any(|e| match e {
        recon_sim::TraceEvent::Sent { at, from, to, .. } => {
            from != to
                && s.trace().events().iter().any(|d| match d {
                    recon_sim::TraceEvent::Delivered { at: da, from: df, to: dt, .. } => {
                        df == from && dt == to && da > at
                    }
                    _ => false,
                })
        }
        _ => false,
    });
    assert!(
        networked,
        "a message that crossed the network must take time, or there is no contrast"
    );
}

#[test]
fn a_refusal_a_leader_hears_from_its_own_acceptor_is_visible_in_the_trace() {
    // **The floor an earlier draft emptied.** Eliding the hand-off inside the protocol made a
    // leader's own acceptor refusing its ballot invisible, and `preemptions` — the non-vacuity half
    // under two registered safety tests — returned exactly zero for runs containing several
    // competing ballots. Recording the hand-off is what keeps it readable, and this asserts that
    // directly rather than trusting `exchanges` to cover it.
    let mut h = Hand::new(&THREE);
    // B takes up a high ballot, so A's own acceptor will refuse A's lower one.
    let high = Ballot { round: 40, leader: C };
    h.event(A, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P1a { ballot: high }) });
    assert_eq!(h.at(A).adopted_ballot(), Some(high), "A's acceptor holds the high ballot");
    h.wire.clear();

    // A now scouts at its own, lower ballot. Its own acceptor refuses, and that refusal is the
    // whole of what this test is about.
    h.make_active_leader(A);
    assert!(
        h.at(A).leader_ballot() > high,
        "A must have climbed above the ballot its own acceptor refused, which is the refusal \
         having been acted on",
    );

    // And in the simulator the same refusal is a trace event rather than nothing at all.
    let mut s = sim_of(&FIVE, unreliable(84));
    for slot in 1..=3u64 {
        s.command(A, Cmd::Propose { slot, command: slot as u32 });
        s.command(E, Cmd::Propose { slot, command: 50 + slot as u32 });
    }
    s.run_for(Duration::from_millis(300));
    s.partition(&[&[A, B], &[C, D, E]]);
    churn(&mut s, 84, 4, Duration::from_millis(200));
    s.heal();
    s.deliver_session_events();
    run_checking(&mut s, Duration::from_secs(3));

    assert!(ballots_seen(&s).len() > 1, "the run must contain competing ballots");
    assert!(
        preemptions(&s) > 0,
        "a refusal must be readable from the trace — this returned zero when the hand-off was \
         elided inside the protocol, and two registered safety tests lost their floor",
    );
    assert!(
        s.trace().handed_to_self().count() > 0,
        "and the run must contain hand-offs, or the floor above was never at risk",
    );
}

// ---------------------------------------------------------------- the hand-driven harness

/// Several processes, one timer-identity source, and a wire the test decides what to do with.
///
/// The simulator delivers or loses by its own rules, which is right for a sweep and wrong for
/// "lose exactly this message and nothing else". Here every send lands in [`Hand::wire`] and the
/// test says which of them arrive.
struct Hand {
    nodes: BTreeMap<NodeId, Synod>,
    stores: BTreeMap<NodeId, MemStore<Infallible, Infallible>>,
    timers: BTreeMap<NodeId, Vec<TimerId>>,
    /// In flight: who sent it, who it is for, what it says.
    wire: Vec<(NodeId, NodeId, Msg)>,
    /// Taken out of flight by [`Hand::hold`] and put back by [`Hand::release`] — a message delayed
    /// past an event rather than lost, which is what a stale leader needs to exist at all.
    held: Vec<(NodeId, NodeId, Msg)>,
    inds: Vec<(NodeId, Ind<u32>)>,
    notes: Vec<(NodeId, Note)>,
    rng: ChaCha8Rng,
    ids: u64,
    now: Time,
}

impl Hand {
    fn new(members: &'static [NodeId]) -> Self {
        let mut h = Hand {
            nodes: members.iter().map(|&n| (n, synod_over(members)(n))).collect(),
            stores: members.iter().map(|&n| (n, MemStore::default())).collect(),
            timers: members.iter().map(|&n| (n, Vec::new())).collect(),
            wire: Vec::new(),
            held: Vec::new(),
            inds: Vec::new(),
            notes: Vec::new(),
            rng: ChaCha8Rng::seed_from_u64(7),
            ids: 0,
            now: Time::ZERO,
        };
        for &n in members {
            h.event(n, Event::Init);
        }
        h
    }

    /// Deliver one event and file everything it emitted.
    fn event(&mut self, node: NodeId, event: Event<Cmd<u32>, Msg, recon_core::SessionEvent>) {
        let proto = self.nodes.get_mut(&node).expect("a member");
        let store = self.stores.get_mut(&node).expect("a member");
        // One identity source for the whole harness: two layers each starting at zero would each
        // accept the other's expiry.
        // `step_noting` rather than `step_with`: a decision that produced no effect is exactly
        // what some of these tests are about, and only the narration holds it.
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

    /// What a node narrated — the decisions that left no effect behind.
    fn notes_at(&self, node: NodeId) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(move |(n, _)| *n == node).map(|(_, note)| note)
    }

    fn propose(&mut self, node: NodeId, slot: Slot, command: u32) {
        self.event(node, Event::Cmd(Cmd::Propose { slot, command }));
    }

    /// Fire every timer this node has registered. The protocol compares before acting, so handing
    /// it all of them is what a driver does.
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

    /// Deliver everything in flight that `keep` accepts, and discard the rest.
    ///
    /// Runs to a fixed point: a delivery emits sends of its own, and a phase does not complete in
    /// one pass.
    fn settle(&mut self, keep: impl Fn(NodeId, NodeId, &Msg) -> bool) {
        for _ in 0..64 {
            if self.wire.is_empty() {
                return;
            }
            let batch = core::mem::take(&mut self.wire);
            for (from, to, msg) in batch {
                if keep(from, to, &msg) {
                    self.event(to, Event::Msg { from, msg });
                }
            }
        }
    }

    /// Deliver everything.
    fn settle_all(&mut self) {
        self.settle(|_, _, _| true);
    }

    /// Deliver only the Synod protocol's own traffic, so a node can be starved of heartbeats
    /// without being cut off from the algorithm. The simulator cannot do this — one wire, one
    /// link — which is why the duel below is driven by hand.
    fn settle_without_heartbeats_to(&mut self, starved: NodeId) {
        self.settle(|_, to, msg| !(to == starved && matches!(msg, Wire::Detector(_))));
    }

    fn advance(&mut self, d: Duration) {
        self.now += d;
    }

    fn at(&self, node: NodeId) -> &Synod {
        self.nodes.get(&node).expect("a member")
    }

    fn decisions_at(&self, node: NodeId) -> Vec<(Slot, u32)> {
        self.inds
            .iter()
            .filter(|(n, _)| *n == node)
            .filter_map(|(_, i)| match i {
                Ind::Decision { slot, command } => Some((*slot, *command)),
                _ => None,
            })
            .collect()
    }

    fn all_decisions(&self) -> Vec<(NodeId, Slot, u32)> {
        self.inds
            .iter()
            .filter_map(|(n, i)| match i {
                Ind::Decision { slot, command } => Some((*n, *slot, *command)),
                _ => None,
            })
            .collect()
    }

    /// What is in flight, for a test that wants to inspect or intercept it.
    fn in_flight(&self) -> &[(NodeId, NodeId, Msg)] {
        &self.wire
    }

    /// The Synod messages in flight from `from` to `to`. The detector's heartbeats share the wire
    /// and are never what a test about the algorithm means.
    fn synod_from(&self, from: NodeId, to: NodeId) -> Vec<SynodMsg<u32>> {
        self.wire
            .iter()
            .filter(|(f, t, _)| *f == from && *t == to)
            .filter_map(|(_, _, m)| match m {
                Wire::Synod(s) => Some(s.clone()),
                Wire::Detector(_) => None,
            })
            .collect()
    }

    /// Every Synod message in flight from `from`, whoever it is for.
    fn synod_sent_by(&self, from: NodeId) -> Vec<SynodMsg<u32>> {
        self.wire
            .iter()
            .filter(|(f, _, _)| *f == from)
            .filter_map(|(_, _, m)| match m {
                Wire::Synod(s) => Some(s.clone()),
                Wire::Detector(_) => None,
            })
            .collect()
    }

    /// Take everything `which` accepts out of flight and keep it, to be put back by
    /// [`Hand::release`]. A *delay*, where [`Hand::settle`] gives a loss.
    fn hold(&mut self, which: impl Fn(NodeId, NodeId, &Msg) -> bool) {
        let batch = core::mem::take(&mut self.wire);
        for (from, to, msg) in batch {
            if which(from, to, &msg) {
                self.held.push((from, to, msg));
            } else {
                self.wire.push((from, to, msg));
            }
        }
    }

    /// Put everything held back in flight.
    fn release(&mut self) {
        let held = core::mem::take(&mut self.held);
        self.wire.extend(held);
    }

    /// Preempt `node` with `high`, whichever phase it is in.
    ///
    /// A leader learns of a higher ballot only through an attempt it has in flight — Figure 7's
    /// `preempted` arrives from a scout or a commander, and a leader running neither is told
    /// nothing. So a scouting leader is answered with a `p1b` and an active one with a `p2b`, for
    /// which it must first have a commander.
    fn preempt(&mut self, node: NodeId, high: Ballot) {
        if self.at(node).is_scouting() {
            let from = *self.nodes.keys().find(|n| **n != node).expect("another member");
            self.event(
                node,
                Event::Msg {
                    from,
                    msg: Wire::Synod(SynodMsg::P1b { ballot: high, accepted: Vec::new() }),
                },
            );
            return;
        }
        assert!(self.at(node).is_active(), "{node} has no attempt in flight to be preempted");
        let existing = self.at(node).commanded_slots().next();
        let slot = match existing {
            Some(slot) => slot,
            None => {
                self.propose(node, 999, 9_999);
                999
            }
        };
        let from = *self.nodes.keys().find(|n| **n != node).expect("another member");
        self.event(
            node,
            Event::Msg { from, msg: Wire::Synod(SynodMsg::P2b { ballot: high, slot }) },
        );
    }

    /// Drive `node` to the point where Ω trusts it and phase one has completed.
    ///
    /// Starving it of heartbeats is what makes it trust itself: it suspects everyone else, so
    /// `maxrank(Π \ suspected)` is itself. The simulator cannot do this — one wire, one link — which
    /// is why the leader-side tests are driven by hand.
    fn make_active_leader(&mut self, node: NodeId) {
        for _ in 0..8 {
            self.advance(timing().detect_after);
            self.tick_all();
            self.settle_without_heartbeats_to(node);
            if self.at(node).is_trusted() && self.at(node).is_active() {
                return;
            }
        }
        panic!(
            "{node} never became an active leader: trusted={} scouting={} active={}",
            self.at(node).is_trusted(),
            self.at(node).is_scouting(),
            self.at(node).is_active(),
        );
    }
}

// ---------------------------------------------------------------- task 2.1: ballots

#[test]
fn any_two_ballots_are_comparable_and_a_ballot_names_its_leader() {
    let a0 = Ballot::initial(A);
    let e0 = Ballot::initial(E);
    let a1 = Ballot { round: 1, leader: A };

    // Lexicographic on ⟨round, leader⟩: the round dominates, and the leader breaks the tie, so two
    // processes' ballots are never equal however far their rounds drift.
    assert!(a0 < e0, "same round, so the leader orders them");
    assert!(e0 < a1, "a higher round wins whoever leads it");
    assert_ne!(a0, e0);
    assert_eq!(a0.leader, A, "a ballot names its leader");
    assert_eq!(a1.leader, A);

    // ⊥ is ordered before any normal ballot number, which `Option`'s own ordering already gives.
    assert!(None < Some(a0), "⊥ is below every ballot");

    // `(r' + 1, self())` — the ballot to take up after being preempted.
    assert_eq!(Ballot::above(e0, A), Ballot { round: 1, leader: A });
    assert!(Ballot::above(e0, A) > e0, "climbing past what beat it");
}

// ---------------------------------------------------------------- task 2.2: the wire

#[test]
fn the_wire_survives_encoding() {
    let mut s = sim_of(&FIVE, synchronous(1));
    s.enable_codec_check();
    s.command(E, Cmd::Propose { slot: 1, command: 42 });
    s.run_for(Duration::from_secs(2));
    // The codec check panics inside the sim on a round trip that does not match, so reaching here
    // with traffic having flowed is the assertion. The floor keeps it from passing vacuously.
    assert!(s.trace().send_count() > 0, "nothing was encoded, so nothing was checked");
}

// ---------------------------------------------------------------- task 3: the acceptor

#[test]
fn a_stale_p1a_is_refused_and_the_refusal_names_the_ballot_that_beat_it() {
    let mut h = Hand::new(&THREE);
    let high = Ballot { round: 9, leader: C };
    let low = Ballot { round: 2, leader: A };

    // B adopts the high ballot first.
    h.event(B, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P1a { ballot: high }) });
    assert_eq!(h.at(B).adopted_ballot(), Some(high));
    h.wire.clear();

    // Then a lower one arrives. `if b > ballot_num` fails, so nothing is taken up.
    h.event(B, Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P1a { ballot: low }) });
    assert_eq!(h.at(B).adopted_ballot(), Some(high), "a stale p1a takes nothing up");

    let replies = h.synod_from(B, A);
    match replies.as_slice() {
        [SynodMsg::P1b { ballot, .. }] => {
            assert_eq!(
                *ballot, high,
                "the refusal names the ballot that beat it, not the one asked"
            );
        }
        other => panic!("expected exactly one p1b, got {other:?}"),
    }
}

#[test]
fn an_acceptors_promise_is_monotonic_over_a_run_that_offers_it_lower_ballots() {
    let mut h = Hand::new(&THREE);
    let offered = [
        Ballot { round: 5, leader: C },
        Ballot { round: 2, leader: A },
        Ballot { round: 7, leader: B },
        Ballot { round: 1, leader: A },
        Ballot { round: 7, leader: A },
    ];
    let mut held = None;
    let mut lower_really_offered = 0;
    for ballot in offered {
        if Some(ballot) < held {
            lower_really_offered += 1;
        }
        h.event(B, Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P1a { ballot }) });
        let now = h.at(B).adopted_ballot();
        assert!(now >= held, "the promise went backwards: {held:?} then {now:?}");
        held = now;
    }
    assert_eq!(held, Some(Ballot { round: 7, leader: B }), "the highest offered, and no other");
    // Non-vacuity: the run really did offer it lower ballots, which is the only way the assertion
    // above could have failed.
    assert!(lower_really_offered >= 2, "only {lower_really_offered} lower ballots were offered");
}

#[test]
fn an_acceptor_that_missed_phase_one_still_counts_in_phase_two() {
    // The departure this module makes, tested directly: Figure 4's `b = ballot_num` would have B
    // refuse here, answer with its own lower ballot, and kill the commander that asked.
    let mut h = Hand::new(&THREE);
    let ballot = Ballot { round: 3, leader: A };

    // B has adopted nothing at all — no p1a for this ballot ever reached it.
    assert_eq!(h.at(B).adopted_ballot(), None, "the phase-one request really did not arrive");

    let pvalue = Pvalue { ballot, slot: 1, command: 77 };
    h.event(B, Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P2a { pvalue }) });

    assert_eq!(h.at(B).adopted_ballot(), Some(ballot), "it adopts what it accepts");
    let replies = h.synod_from(B, A);
    match replies.as_slice() {
        [SynodMsg::P2b { ballot: answered, slot }] => {
            assert_eq!(*answered, ballot, "the answer counts toward the commander's majority");
            assert_eq!(*slot, 1, "and names the slot it answers for");
        }
        other => panic!("expected exactly one p2b, got {other:?}"),
    }
}

// ---------------------------------------------------------------- task 4: the leader

#[test]
fn a_proposal_arriving_before_the_ballot_is_adopted_is_remembered_and_sent_on_adoption() {
    // Figure 7's `if active then` guard doing nothing. "Passive" here means **trusted but not yet
    // adopted** — a process Ω does not trust forwards instead, which is the test below.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    // Put A back into phase one, where it is trusted and scouting but not active.
    h.preempt(A, Ballot { round: 40, leader: C });
    assert!(h.at(A).is_trusted() && h.at(A).is_scouting() && !h.at(A).is_active());
    // The wire is left alone: it holds the scout's own `p1a`, and clearing it would leave the
    // attempt unanswerable and this test asserting adoption that could never happen.
    h.propose(A, 4, 400);
    assert!(
        h.synod_sent_by(A).iter().all(|m| !matches!(
            m,
            SynodMsg::P2a { pvalue } if pvalue.slot == 4
        )),
        "a leader that has not adopted commands nothing: {:?}",
        h.synod_sent_by(A),
    );
    assert!(
        h.synod_sent_by(A).iter().all(|m| !matches!(m, SynodMsg::Propose { .. })),
        "and a trusted leader forwards nothing — it will lead itself",
    );

    // Adoption spawns a commander for every proposal it was holding.
    h.settle_without_heartbeats_to(A);
    assert!(
        h.decisions_at(A).contains(&(4, 400)),
        "the remembered proposal must be commanded once the ballot is adopted, got {:?}",
        h.decisions_at(A),
    );
}

#[test]
fn a_proposal_at_a_process_that_will_not_lead_is_forwarded_to_the_one_that_will() {
    // §4.4: "If λ is passive, monitoring another leader λ′, it forwards the proposal to λ′."
    // Without this a proposal made at a process Ω never trusts is never acted on at all.
    let mut h = Hand::new(&THREE);
    // C is the highest, so Ω trusts C everywhere. A is passive and will stay so.
    h.advance(timing().detect_after);
    h.tick_all();
    h.settle_all();
    assert!(!h.at(A).is_trusted(), "A is not trusted, which is the premise");
    assert_eq!(h.at(A).trusted_leader(), Some(C), "and it knows who is");
    h.wire.clear();

    h.propose(A, 4, 400);
    let forwarded: Vec<_> = h.synod_from(A, C);
    assert!(
        forwarded.iter().any(|m| matches!(m, SynodMsg::Propose { slot: 4, command: 400 })),
        "the proposal must reach the trusted leader, got {forwarded:?}",
    );
    assert!(
        h.synod_from(A, B).is_empty(),
        "and only that one — a forward is directed, not a fan-out",
    );

    // It is commanded under the leader's own ballot, and decided.
    h.settle_all();
    assert!(
        h.decisions_at(C).contains(&(4, 400)),
        "the forwarded proposal must be commanded by the leader, got {:?}",
        h.decisions_at(C),
    );
    assert!(
        h.decisions_at(A).contains(&(4, 400)),
        "and the decision must reach the process that asked",
    );
}

#[test]
fn a_forwarded_proposal_is_not_forwarded_again() {
    // Two processes whose detectors disagree would pass one back and forth for as long as they
    // disagree. A `Propose` that arrived is handled locally or dropped, so the path is two hops.
    let mut h = Hand::new(&THREE);
    h.advance(timing().detect_after);
    h.tick_all();
    h.settle_all();
    assert!(!h.at(B).is_trusted(), "B is passive and trusts someone else");
    h.wire.clear();

    // A forwarded proposal arrives at B, which also cannot act on it.
    h.event(
        B,
        Event::Msg { from: A, msg: Wire::Synod(SynodMsg::Propose { slot: 5, command: 500 }) },
    );
    assert!(
        h.synod_sent_by(B).iter().all(|m| !matches!(m, SynodMsg::Propose { .. })),
        "an arrived proposal must not be forwarded on: {:?}",
        h.synod_sent_by(B),
    );
    // The decision to drop produces no effect at all, so the narration is the only record.
    assert!(
        h.notes_at(B).any(|n| matches!(n, Note::ProposalIgnored { slot: 5 })),
        "dropping it must be narrated, since nothing else can say it happened",
    );
}

#[test]
fn an_active_leader_commands_rather_than_forwarding_even_where_omega_has_moved_on() {
    // The ordering of the two conditions. An adopted ballot stands until something preempts it, so
    // commanding is a round trip where forwarding is a round trip plus a phase one. "Stops
    // competing" means starting no new ballots, not abandoning one a majority already adopted.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    // Let the heartbeats back so Ω moves to C while A stays active at its adopted ballot.
    for _ in 0..8 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle_all();
        if !h.at(A).is_trusted() {
            break;
        }
    }
    assert!(!h.at(A).is_trusted() && h.at(A).is_active(), "A is active but no longer trusted");
    h.wire.clear();

    h.propose(A, 6, 600);
    assert!(
        h.synod_sent_by(A).iter().any(|m| matches!(
            m,
            SynodMsg::P2a { pvalue } if pvalue.slot == 6
        )),
        "an active leader commands: {:?}",
        h.synod_sent_by(A),
    );
    assert!(
        h.synod_sent_by(A).iter().all(|m| !matches!(m, SynodMsg::Propose { .. })),
        "and forwards nothing",
    );
}

#[test]
fn every_process_learns_a_decision_not_only_the_one_that_counted_it() {
    // Figure 6(a)'s last line, restored: `∀ρ ∈ replicas : send(ρ, ⟨decision, s, c⟩)`.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    h.propose(A, 2, 222);
    h.settle_without_heartbeats_to(A);

    for node in THREE {
        assert!(
            h.decisions_at(node).contains(&(2, 222)),
            "{node} did not learn the decision; only the counting process did",
        );
    }
}

#[test]
fn a_leader_answers_a_re_proposal_for_a_decided_slot_with_the_decision() {
    // The leader-side half of Liu et al.'s fix. A decision is announced once and its commander then
    // exits, so a process the announcement never reached has no other way back; asking is the way.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);

    // A decides slot 3, but the announcement never reaches B.
    h.propose(A, 3, 333);
    h.settle(|_, to, msg| {
        !(to == B && matches!(msg, Wire::Synod(SynodMsg::Decision { .. })))
            && !(to == A && matches!(msg, Wire::Detector(_)))
    });
    assert!(h.decisions_at(A).contains(&(3, 333)), "A decided it");
    assert!(h.decisions_at(B).is_empty(), "and B was never told — the precondition");
    h.wire.clear();

    // B asks, by proposing for the same slot. The answer goes to B alone.
    h.event(
        A,
        Event::Msg { from: B, msg: Wire::Synod(SynodMsg::Propose { slot: 3, command: 999 }) },
    );
    let answered = h.synod_from(A, B);
    assert!(
        answered.iter().any(|m| matches!(m, SynodMsg::Decision { slot: 3, command: 333 })),
        "the answer must carry the decided command, not the one asked about: {answered:?}",
    );
    assert!(
        h.synod_from(A, C).is_empty(),
        "and go to the asker alone, not a fan-out — the announcement was the fan-out",
    );
    assert!(
        h.synod_sent_by(A).iter().all(|m| !matches!(m, SynodMsg::P2a { .. })),
        "and start no commander for a slot already decided",
    );

    // Which is what unwedges B.
    h.settle_without_heartbeats_to(A);
    assert!(
        h.decisions_at(B).contains(&(3, 333)),
        "asking must be what gets the decision to B, got {:?}",
        h.decisions_at(B),
    );
}

#[test]
fn a_proposal_for_a_slot_proposed_but_not_yet_decided_draws_no_answer() {
    // The other half of the guard: the attempt already in flight is what fills the slot, and a
    // second commander for one ⟨ballot, slot⟩ would break Invariant C1.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);

    // A commands slot 4 but no answer arrives, so it stays undecided.
    h.propose(A, 4, 444);
    h.settle(|_, to, msg| {
        !(to == A && matches!(msg, Wire::Synod(SynodMsg::P2b { .. })))
            && !(to == A && matches!(msg, Wire::Detector(_)))
    });
    assert!(h.decisions_at(A).is_empty(), "nothing is decided yet");
    assert!(h.at(A).commanded_slots().any(|s| s == 4), "and the commander is still outstanding");
    h.wire.clear();

    h.event(
        A,
        Event::Msg { from: B, msg: Wire::Synod(SynodMsg::Propose { slot: 4, command: 999 }) },
    );
    assert!(
        h.synod_from(A, B).iter().all(|m| !matches!(m, SynodMsg::Decision { .. })),
        "an undecided slot must draw no decision: {:?}",
        h.synod_from(A, B),
    );
    assert_eq!(
        h.at(A).commanded_slots().collect::<Vec<_>>(),
        vec![4],
        "and no second commander for the slot",
    );
}

#[test]
fn the_answer_names_the_decided_command_not_the_answerers_own_stale_proposal() {
    // A safety hole in the answer path, found by reading. The answer takes its command from
    // `proposals[slot]`, and the module argued that for a decided slot that is the decided command
    // — true of the leader that decided it and of any leader that adopted afterwards, because
    // `pmax` rewrites it. It is **false** of a leader that commanded something else, was preempted,
    // and never adopted again: nothing rewrites its `proposals`, the announcement of the real
    // decision still marks the slot decided, and a forwarded re-proposal is then answered with the
    // wrong command. A replica whose detector names that process would apply it.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);

    // A commands 333 for slot 3, and nothing A sends reaches anyone: no acceptor ever holds it.
    h.propose(A, 3, 333);
    let nothing_from_a = |from: NodeId, to: NodeId, msg: &Msg| {
        from != A && !(to == C && matches!(msg, Wire::Detector(_)))
    };
    h.settle(nothing_from_a);
    assert!(h.at(A).commanded_slots().any(|s| s == 3), "A holds a commander for slot 3");

    // C takes over with a higher ballot, finds nothing accepted for slot 3, and decides 777 there.
    for _ in 0..8 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle(nothing_from_a);
        if h.at(C).is_active() {
            break;
        }
    }
    assert!(h.at(C).is_active(), "C must lead for the slot to be decided under it");
    h.propose(C, 3, 777);
    h.settle(nothing_from_a);
    assert!(h.decisions_at(C).contains(&(3, 777)), "C decided 777");
    assert!(h.decisions_at(A).contains(&(3, 777)), "and A was told — so A knows slot 3 is decided");
    h.wire.clear();

    // B asks A. Whatever A answers must be what was decided.
    h.event(
        A,
        Event::Msg { from: B, msg: Wire::Synod(SynodMsg::Propose { slot: 3, command: 999 }) },
    );
    let answered = h.synod_from(A, B);
    let decisions: Vec<u32> = answered
        .iter()
        .filter_map(|m| match m {
            SynodMsg::Decision { slot: 3, command } => Some(*command),
            _ => None,
        })
        .collect();
    assert!(!decisions.is_empty(), "A knows the slot is decided and must answer: {answered:?}");
    assert_eq!(decisions, vec![777], "A answered with a command that was never chosen");
}

#[test]
fn a_proposal_remembered_by_a_leader_that_then_yields_is_forwarded_when_asked_again() {
    // A liveness hole beside the safety one, and with the same root: `proposals` outliving the
    // leadership that filled it. A trusted-but-not-yet-adopted process remembers a proposal for
    // `adopted` to command. If it is preempted before adopting and Ω has moved on, it yields, and
    // the entry stays. Every later proposal for that slot from its own replica then meets the
    // `∄c'` guard and is dropped — never forwarded, never commanded by anyone. The slot is never
    // filled, `slot_out` never passes it, and the replica's re-proposal — the one fix for exactly
    // this — is defeated by the process's own memory.
    let mut h = Hand::new(&THREE);

    // A trusts itself and scouts, but its `p1b` answers never arrive, so it never adopts.
    let starve_a = |_: NodeId, to: NodeId, msg: &Msg| {
        !(to == A && matches!(msg, Wire::Detector(_) | Wire::Synod(SynodMsg::P1b { .. })))
    };
    for _ in 0..3 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle(starve_a);
    }
    assert!(h.at(A).is_trusted() && h.at(A).is_scouting() && !h.at(A).is_active());

    // Its replica proposes. Remembered, for an adoption that will never come.
    h.propose(A, 5, 555);
    assert!(
        !h.synod_sent_by(A).iter().any(|m| matches!(m, SynodMsg::Propose { .. })),
        "trusted, so nothing is forwarded: it is remembered for `adopted`",
    );

    // C rises while nothing of A's reaches anyone, so every acceptor ends up above A's ballot.
    let nothing_from_a = |from: NodeId, to: NodeId, msg: &Msg| {
        from != A && !(to == C && matches!(msg, Wire::Detector(_)))
    };
    for _ in 0..8 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle(nothing_from_a);
        if h.at(C).is_active() {
            break;
        }
    }
    assert!(h.at(C).is_active());

    // Now A hears everyone: its detector moves to C, and its scout is answered with C's ballot
    // and preempted. Not trusted, it yields.
    for _ in 0..4 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle_without_heartbeats_to(C);
    }
    assert_eq!(h.at(A).trusted_leader(), Some(C), "Ω at A must have moved to C");
    assert!(!h.at(A).is_active() && !h.at(A).is_scouting(), "and A must have yielded");
    assert!(h.decisions_at(A).is_empty(), "slot 5 is undecided — the precondition");
    h.wire.clear();

    // The replica asks again. The only thing that can fill slot 5 now is C, so this must reach C.
    h.propose(A, 5, 555);
    let to_c = h.synod_from(A, C);
    assert!(
        to_c.iter().any(|m| matches!(m, SynodMsg::Propose { slot: 5, command: 555 })),
        "a process that will not lead must forward, not sit on its own stale proposal: {to_c:?}",
    );
}

#[test]
fn an_acceptor_keeps_one_pvalue_per_slot_however_many_ballots_command_it() {
    // §4.1: "acceptors only maintain the most recently accepted pvalue for each slot". Three
    // ballots command slot 1, and the acceptor must end with one entry for it, at the highest.
    //
    // Driven by hand: a seeded run has no reason to put three ballots against one slot, and the
    // property is about what is *not* kept, which a run that never produces the case cannot show.
    let mut h = Hand::new(&THREE);
    let ballots = [
        Ballot { round: 1, leader: A },
        Ballot { round: 4, leader: B },
        Ballot { round: 9, leader: C },
    ];
    for (i, ballot) in ballots.iter().enumerate() {
        let pvalue = Pvalue { ballot: *ballot, slot: 1, command: 100 + i as u32 };
        h.event(B, Event::Msg { from: ballot.leader, msg: Wire::Synod(SynodMsg::P2a { pvalue }) });
    }

    assert_eq!(
        h.at(B).accepted_for(1).map(|(b, c)| (b, *c)),
        Some((ballots[2], 102)),
        "the highest ballot's command is what is kept",
    );
    assert_eq!(h.at(B).accepted_count(), 1, "and one entry, not three");

    // A fourth slot is a fourth entry: the reduction is per slot, not a cap.
    let pvalue = Pvalue { ballot: ballots[2], slot: 2, command: 200 };
    h.event(B, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P2a { pvalue }) });
    assert_eq!(
        h.at(B).accepted_count(),
        2,
        "state grows with slots, which is why this is still a \
                                              transcription"
    );
}

#[test]
fn an_acceptors_record_for_a_slot_only_ever_moves_up() {
    // §4.1 has the acceptor keep "the most recently accepted pvalue", with no comparison, and the
    // module argues that the promise already makes the latest the highest. This is that argument
    // driven: whatever order pvalues arrive in, the record for a slot never goes backwards.
    let mut h = Hand::new(&THREE);
    let low = Ballot { round: 2, leader: A };
    let high = Ballot { round: 7, leader: C };

    h.event(B, Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P1a { ballot: low }) });
    let pvalue = Pvalue { ballot: high, slot: 3, command: 777 };
    h.event(B, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P2a { pvalue }) });
    assert_eq!(h.at(B).accepted_for(3).map(|(b, c)| (b, *c)), Some((high, 777)));
    h.wire.clear();

    // The low ballot's own `p2a`, arriving late — a retransmission across a session ending is how.
    // The promise refuses it, which is the `b ≥ ballot_num` arm, so it never reaches the record.
    // That is the whole of why the record needs no comparison of its own.
    let stale = Pvalue { ballot: low, slot: 3, command: 111 };
    h.event(B, Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P2a { pvalue: stale }) });
    assert_eq!(
        h.at(B).accepted_for(3).map(|(b, c)| (b, *c)),
        Some((high, 777)),
        "a ballot the acceptor has superseded must not reach the slot's record",
    );
    match h.synod_from(B, A).as_slice() {
        [SynodMsg::P2b { ballot, slot: 3 }] => {
            assert_eq!(*ballot, high, "and the reply names the ballot the acceptor holds");
        }
        other => panic!("expected one p2b for slot 3, got {other:?}"),
    }
    h.wire.clear();

    // The only pvalue the promise admits that is not strictly above the record is one at exactly
    // the held ballot, and Invariant A4 makes that the same command. So the write is idempotent
    // rather than a case needing a guard.
    let same = Pvalue { ballot: high, slot: 3, command: 777 };
    h.event(B, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P2a { pvalue: same }) });
    assert_eq!(h.at(B).accepted_for(3).map(|(b, c)| (b, *c)), Some((high, 777)));
    assert_eq!(h.at(B).accepted_count(), 1, "and still one entry");
}

#[test]
fn a_scout_keeps_the_highest_ballot_reported_for_a_slot_not_the_last_one_to_arrive() {
    // Where §4.1 puts the maximum: at the leader, across the majority that answers its phase one.
    // Two acceptors report *different* ballots for one slot — reachable whenever a later ballot
    // overwrote one acceptor's record and not another's — and nothing orders their answers. A scout
    // that kept the last arrival would hand `pmax` a command a lower ballot proposed, and the slot
    // would split.
    //
    // This test exists because a mutation found nothing: reducing `keep_max` to a plain insert left
    // the whole suite green and `check-safety-tests.sh` passing. The property used to be carried by
    // the old `⟨ballot, slot⟩` key's iteration order — structural, and so never named by a test.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    // Preempt A so it scouts again, and note the ballot its scout now runs.
    h.preempt(A, Ballot { round: 20, leader: C });
    assert!(h.at(A).is_scouting(), "A must be in phase one for its scout to collect");
    let scouting = h.at(A).leader_ballot();
    h.wire.clear();

    let higher = Ballot { round: 9, leader: C };
    let lower = Ballot { round: 4, leader: B };

    // B answers first with the higher ballot for slot 1, C second with the lower. Last arrival and
    // maximum therefore disagree, which is the whole point of the schedule.
    let answer = |ballot: Ballot, command: u32| SynodMsg::P1b {
        ballot: scouting,
        accepted: vec![Pvalue { ballot, slot: 1, command }],
    };
    h.event(A, Event::Msg { from: B, msg: Wire::Synod(answer(higher, 999)) });
    h.event(A, Event::Msg { from: C, msg: Wire::Synod(answer(lower, 111)) });

    // Two of three is a majority, so the scout has adopted and the leader has commanded.
    assert!(h.at(A).is_active(), "the scout must have adopted on the second answer");
    let commanded: Vec<u32> = h
        .synod_sent_by(A)
        .iter()
        .filter_map(|m| match m {
            SynodMsg::P2a { pvalue } if pvalue.slot == 1 => Some(pvalue.command),
            _ => None,
        })
        .collect();
    assert!(!commanded.is_empty(), "the leader must command slot 1 after adopting");
    for command in &commanded {
        assert_eq!(
            *command, 999,
            "the leader commanded {command}, which the lower ballot proposed — `pmax` must read \
             the maximum, not the last answer to arrive",
        );
    }
    // Non-vacuity: the two answers really did carry different ballots, and the lower one really did
    // arrive last.
    assert!(lower < higher, "the schedule must actually put the maximum first");
}

#[test]
fn a_phase_one_answer_carries_one_pvalue_per_slot() {
    // The message §4.1 exists for. Its size must grow with the slots an acceptor has accepted for
    // and not with the ballots the run has seen.
    let mut h = Hand::new(&THREE);
    let ballots = [
        Ballot { round: 1, leader: A },
        Ballot { round: 3, leader: A },
        Ballot { round: 5, leader: A },
    ];
    // Three ballots against two slots, so ballots outnumber slots and the two growths are
    // distinguishable.
    for (i, ballot) in ballots.iter().enumerate() {
        for slot in 1..=2u64 {
            let pvalue =
                Pvalue { ballot: *ballot, slot, command: (i as u32 + 1) * 10 + slot as u32 };
            h.event(B, Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P2a { pvalue }) });
        }
    }
    h.wire.clear();

    let high = Ballot { round: 8, leader: C };
    h.event(B, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P1a { ballot: high }) });
    match h.synod_from(B, C).as_slice() {
        [SynodMsg::P1b { ballot, accepted }] => {
            assert_eq!(*ballot, high, "the acceptor answers with the ballot it now holds");
            let mut slots: Vec<Slot> = accepted.iter().map(|p| p.slot).collect();
            slots.sort_unstable();
            assert_eq!(slots, vec![1, 2], "one entry per slot, not one per ⟨ballot, slot⟩");
            for p in accepted {
                assert_eq!(p.ballot, ballots[2], "and each is the highest ballot for its slot");
            }
        }
        other => panic!("expected one p1b, got {other:?}"),
    }
    // Non-vacuity: the run really did contain more ballots than slots, which is the whole
    // distinction being drawn.
    assert!(ballots.len() > 2, "three ballots against two slots");
}

#[test]
fn agreement_survives_the_record_of_it_being_overwritten() {
    // §4.1's "worrisome effect", driven exactly as the paper sets it out. Three acceptors; α₁ and
    // α₂ accept ⟨⟨0, λ⟩, 1, c⟩, so c is chosen for slot 1. λ crashes before learning it. λ′ gets
    // α₂ and α₃ to adopt a higher ballot, must select c by pmax, and then α₂ accepts it under the
    // new ballot — overwriting the only other copy of the evidence.
    //
    // At that point no majority stores the pvalue that was chosen, and the paper says so: "in fact
    // no proof that ballot ⟨0, λ⟩ even chose proposal c, as that part of the history has been
    // overwritten". What must still hold is that no other command is ever chosen for slot 1.
    let mut h = Hand::new(&THREE);
    let lo = Ballot { round: 0, leader: A }; // λ
    let hi = Ballot { round: 0, leader: C }; // λ′, higher because the leader breaks the tie

    // α₁ = A and α₂ = B accept ⟨lo, 1, 555⟩. That is a majority of three, so 555 is chosen.
    for acceptor in [A, B] {
        let pvalue = Pvalue { ballot: lo, slot: 1, command: 555 };
        h.event(acceptor, Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P2a { pvalue }) });
    }
    assert_eq!(h.at(A).accepted_for(1).map(|(_, c)| *c), Some(555));
    assert_eq!(h.at(B).accepted_for(1).map(|(_, c)| *c), Some(555));
    h.wire.clear();

    // λ′ runs phase one against α₂ and α₃ — a different majority, which must intersect the first.
    let mut reported: Vec<Pvalue<u32>> = Vec::new();
    for acceptor in [B, C] {
        h.event(acceptor, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P1a { ballot: hi }) });
        for msg in h.synod_from(acceptor, C) {
            if let SynodMsg::P1b { accepted, .. } = msg {
                reported.extend(accepted);
            }
        }
        h.wire.clear();
    }
    // The intersection is what carries the choice: α₂ is in both majorities and still holds it.
    let for_slot_one: Vec<u32> =
        reported.iter().filter(|p| p.slot == 1).map(|p| p.command).collect();
    assert_eq!(for_slot_one, vec![555], "the maximum λ′ must select is the chosen command");

    // So λ′ commands 555 under `hi`, and α₂ accepts it — overwriting its record of `lo`.
    let pvalue = Pvalue { ballot: hi, slot: 1, command: 555 };
    h.event(B, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P2a { pvalue }) });

    // The paper's own observation, asserted: no majority now stores the pvalue that was chosen.
    let still_hold_lo = [A, B, C]
        .iter()
        .filter(|n| h.at(**n).accepted_for(1).is_some_and(|(b, _)| b == lo))
        .count();
    assert_eq!(still_hold_lo, 1, "only α₁ still holds the record — not a majority of three");
    assert!(
        !is_majority_of(still_hold_lo, 3),
        "the evidence is gone, which is the effect §4.1 warns about",
    );

    // And the fact stands: every acceptor that holds anything for slot 1 holds 555.
    for node in THREE {
        if let Some((_, command)) = h.at(node).accepted_for(1) {
            assert_eq!(*command, 555, "{node} holds a command for slot 1 that was never chosen");
        }
    }

    // A third ballot repeats the argument rather than escaping it: it reads a majority, and every
    // majority of three contains an acceptor holding 555.
    let third = Ballot { round: 1, leader: A };
    h.wire.clear();
    let mut seen: Vec<u32> = Vec::new();
    for acceptor in [A, C] {
        h.event(
            acceptor,
            Event::Msg { from: A, msg: Wire::Synod(SynodMsg::P1a { ballot: third }) },
        );
        for msg in h.synod_from(acceptor, A) {
            if let SynodMsg::P1b { accepted, .. } = msg {
                seen.extend(accepted.iter().filter(|p| p.slot == 1).map(|p| p.command));
            }
        }
        h.wire.clear();
    }
    assert_eq!(seen, vec![555], "a later ballot can only select what was already chosen");
}

/// The figures' majority test, for a test that needs to say a count is *not* one.
fn is_majority_of(held: usize, acceptors: usize) -> bool {
    held * 2 > acceptors
}

#[test]
fn a_proposal_forwarded_to_a_crashed_process_is_lost() {
    // The cost of colocation, tested rather than assumed: a proposal now goes to one process, so a
    // detector naming one that has died loses it. Nothing at this layer recovers it — the replica's
    // re-proposal timeout is what does, and that is a later change.
    let mut s = sim_of(&FIVE, synchronous(53));
    s.run_for(Duration::from_millis(200));
    // E is the highest, so Ω trusts it everywhere.
    assert_eq!(s.at(A).trusted_leader(), Some(E), "the detector names E");
    s.crash(E);
    assert!(s.is_stopped(E), "and E really crashed, before the proposal is made");

    // A is passive, still trusts E, and forwards there.
    assert!(!s.at(A).is_trusted(), "A will not lead, so it forwards");
    s.command(A, Cmd::Propose { slot: 1, command: 111 });
    s.run_for(Duration::from_millis(50));
    let forwarded = s
        .trace()
        .sends()
        .filter(|(from, to, m)| {
            *from == A && *to == E && matches!(m, Wire::Synod(SynodMsg::Propose { .. }))
        })
        .count();
    assert!(forwarded > 0, "the forward really happened — the non-vacuity half");

    run_checking(&mut s, Duration::from_secs(3));
    assert!(
        !decisions(&s).contains_key(&1),
        "nothing at this layer recovers a proposal forwarded into a crash: {:?}",
        decisions(&s),
    );
}

#[test]
fn adoption_needs_a_majority_and_not_one_fewer() {
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);

    // Force a fresh phase one under a ballot nobody has answered, by preempting A with a higher
    // one. It is trusted, so it scouts again.
    let high = Ballot { round: 40, leader: C };
    h.preempt(A, high);
    assert!(h.at(A).is_scouting() && !h.at(A).is_active(), "A is back in phase one");
    let ballot = h.at(A).leader_ballot();
    assert!(ballot > high, "under a ballot above the one that beat it");

    // One answer of three is not a majority: |waitfor| = 2, and 2 * 2 < 3 is false. Written the
    // other way — `waitfor.len() < acceptors.len() / 2` — integer division reads `< 1` here and
    // would demand all three.
    h.event(
        A,
        Event::Msg { from: B, msg: Wire::Synod(SynodMsg::P1b { ballot, accepted: Vec::new() }) },
    );
    assert!(!h.at(A).is_active(), "one answer of three is not a majority");
    // The second makes it: |waitfor| = 1, and 1 * 2 < 3.
    h.event(
        A,
        Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P1b { ballot, accepted: Vec::new() }) },
    );
    assert!(h.at(A).is_active(), "two of three is a majority and must adopt");
}

#[test]
fn a_later_ballot_proposes_what_an_earlier_majority_accepted() {
    // pmax, and the step the whole safety argument rests on. A sets out to propose 111 for slot 1;
    // the majority tells it 999 was already accepted under a lower ballot; it must propose 999.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);

    // Preempt A so it runs a fresh phase one whose answers this test writes.
    let old = Ballot { round: 40, leader: E };
    h.preempt(A, old);
    let ballot = h.at(A).leader_ballot();
    assert!(ballot > old, "a preempted leader climbs past what beat it");
    assert!(h.at(A).is_scouting(), "and a trusted one scouts again");

    // It intends 111 for slot 1.
    h.propose(A, 1, 111);
    h.wire.clear();

    // The majority reports a pvalue for slot 1 under a lower ballot, carrying a different command.
    let earlier = Ballot { round: 39, leader: C };
    let reported = vec![Pvalue { ballot: earlier, slot: 1, command: 999 }];
    for peer in [B, C] {
        h.event(
            A,
            Event::Msg {
                from: peer,
                msg: Wire::Synod(SynodMsg::P1b { ballot, accepted: reported.clone() }),
            },
        );
    }
    assert!(h.at(A).is_active(), "a majority answered, so it adopted");

    let commanded: Vec<u32> = h
        .synod_sent_by(A)
        .iter()
        .filter_map(|m| match m {
            SynodMsg::P2a { pvalue } if pvalue.slot == 1 => Some(pvalue.command),
            _ => None,
        })
        .collect();
    assert!(!commanded.is_empty(), "the adopted leader must command slot 1");
    assert!(
        commanded.iter().all(|c| *c == 999),
        "the leader proposed {commanded:?}, not what the majority had already accepted (999)",
    );
}

#[test]
fn at_most_one_commander_per_slot_per_ballot() {
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);

    h.propose(A, 7, 700);
    assert_eq!(h.at(A).commanded_slots().collect::<Vec<_>>(), vec![7], "one commander for slot 7");

    // A second proposal for a slot already proposed for is dropped — Figure 7's guard, and what
    // enforces C1 against a second commander for one ⟨ballot, slot⟩.
    h.wire.clear();
    h.propose(A, 7, 701);
    assert_eq!(h.at(A).commanded_slots().collect::<Vec<_>>(), vec![7], "still exactly one");
    assert!(
        !h.synod_sent_by(A).iter().any(|m| matches!(
            m,
            SynodMsg::P2a { pvalue } if pvalue.command == 701
        )),
        "the second command for a commanded slot must not go on the wire",
    );
    // The decision produced no effect whatever, so the trace cannot say it happened. The narration
    // is the only record, which is the whole reason this module narrates.
    assert!(
        h.notes_at(A).any(|n| matches!(n, Note::ProposalIgnored { slot: 7 })),
        "the leader must say it dropped the proposal, since nothing else can",
    );
}

#[test]
fn a_preempted_leader_that_omega_no_longer_trusts_stops_competing() {
    // Both roles are in this run: A leads and is then preempted, and the process that beat it is
    // the one Ω trusts. Starve A first so it leads at all, then let the heartbeats back so its
    // detector restores the others and Ω moves its answer to C.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    assert!(h.at(A).is_trusted(), "A led, which is what makes the next step mean anything");

    for _ in 0..8 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle_all();
        if !h.at(A).is_trusted() {
            break;
        }
    }
    assert!(
        !h.at(A).is_trusted(),
        "Ω must have moved to the highest process for this to be a test"
    );

    let before = h.at(A).leader_ballot();
    h.wire.clear();
    // Preempt A with a ballot above its own, through the scout it is running.
    let high = Ballot { round: before.round + 20, leader: C };
    assert!(
        h.at(A).is_scouting() || h.at(A).is_active(),
        "A must have an attempt in flight for a preemption to reach it",
    );
    h.preempt(A, high);

    assert!(h.at(A).leader_ballot() > high, "it still climbs past what beat it");
    assert!(!h.at(A).is_scouting(), "but an untrusted leader starts no scout");
    assert!(!h.at(A).is_active());
    // Nothing whatever reaches the trace from the decision to stand down, which is why it is
    // narrated: a leader correctly standing down and one that was never told look identical.
    assert!(
        h.notes_at(A).any(|n| matches!(n, Note::LeadershipYielded { to, .. } if *to == C)),
        "standing down must be narrated, since it leaves no other evidence",
    );
}

// ---------------------------------------------------------------- task 5: liveness through Ω

#[test]
fn a_settled_detector_gets_every_proposed_slot_chosen() {
    let mut s = sim_of(&FIVE, synchronous(3));
    for slot in 1..=4u64 {
        s.command(E, Cmd::Propose { slot, command: (slot * 10) as u32 });
    }
    let checked = run_checking(&mut s, Duration::from_secs(3));

    let decided = decisions(&s);
    assert_eq!(decided.len(), 4, "every proposed slot must be chosen: got {decided:?}");
    for slot in 1..=4u64 {
        assert_eq!(decided.get(&slot), Some(&((slot * 10) as u32)));
    }
    assert!(!checked.chosen.is_empty(), "and the checker really saw them");
}

#[test]
fn duelling_leaders_are_permitted_to_choose_nothing() {
    // Two processes each believing themselves leader. Driven by hand because the simulator cannot
    // starve one node of heartbeats while still carrying the algorithm's own traffic: one wire,
    // one link. A is fed no heartbeats, so it suspects B and C and trusts itself; C is the highest
    // node, so everybody else trusts C.
    let mut h = Hand::new(&THREE);
    for _ in 0..4 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle_without_heartbeats_to(A);
    }
    assert!(h.at(A).is_trusted(), "A trusts itself");
    assert!(h.at(C).is_trusted(), "and so does C");

    h.propose(A, 1, 111);
    h.propose(C, 1, 999);

    let mut rounds = 0;
    for _ in 0..12 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle_without_heartbeats_to(A);
        rounds += 1;
    }

    // Progress is **not** asserted. What is asserted is that the run really was a duel, and that
    // safety survived it — which is the whole of what this capability claims.
    let ballots_a = h.at(A).leader_ballot();
    let ballots_c = h.at(C).leader_ballot();
    assert!(
        ballots_a.round > 0 && ballots_c.round > 0,
        "neither leader was preempted, so this was not a duel: A at {ballots_a}, C at {ballots_c} \
         after {rounds} rounds",
    );
    let mut per_slot: BTreeMap<Slot, BTreeSet<u32>> = BTreeMap::new();
    for (_, slot, command) in h.all_decisions() {
        per_slot.entry(slot).or_default().insert(command);
    }
    for (slot, commands) in per_slot {
        assert_eq!(commands.len(), 1, "slot {slot} was split during the duel: {commands:?}");
    }
}

#[test]
fn safety_holds_while_the_detector_is_wrong() {
    // `detect_after` well below what the network needs, so Ω accuses correct processes and
    // leadership moves while the ballots reflect it. The risk `design.md` names, driven rather
    // than assumed.
    let wrong = Timing {
        retransmit: Duration::from_millis(10),
        heartbeat: Duration::from_millis(10),
        detect_after: Duration::from_millis(12),
    };
    let mut s: Sim<Synod> = Sim::new(
        Config::default()
            .seed(11)
            .latency(Duration::from_millis(1), Duration::from_millis(60))
            .loss(0.05)
            .max_steps(2_000_000)
            .sessions(),
        &FIVE,
        move |me| MultiPaxosSynod::new(me, FIVE, wrong),
    );
    s.deliver_session_events();
    for slot in 1..=3u64 {
        s.command(A, Cmd::Propose { slot, command: slot as u32 });
        s.command(E, Cmd::Propose { slot, command: (slot + 100) as u32 });
    }
    run_checking(&mut s, Duration::from_secs(5));

    // Non-vacuity: the detector really was wrong for a while, which shows up as more than one
    // ballot on the wire and at least one preemption.
    assert!(ballots_seen(&s).len() > 1, "only one ballot ran, so the detector never disagreed");
    assert!(preemptions(&s) > 0, "nothing was ever preempted, so no leader was ever wrong");
}

// ---------------------------------------------------------------- task 6: the link and retries

#[test]
fn a_session_ending_reaches_the_layer_above() {
    let mut s = sim_of(&FIVE, synchronous(5));
    s.command(E, Cmd::Propose { slot: 1, command: 1 });
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
fn a_request_lost_at_a_session_ending_is_retried_and_the_round_still_completes() {
    // A session ending is how this stack loses a message; the link beneath does not retransmit, so
    // the leader owns the retry. Everything in flight to the broken peer is gone.
    let mut s = sim_of(&FIVE, unreliable(13));
    s.command(E, Cmd::Propose { slot: 1, command: 1 });
    churn(&mut s, 13, 6, Duration::from_millis(150));
    run_checking(&mut s, Duration::from_secs(4));

    let decided = decisions(&s);
    assert_eq!(decided.get(&1), Some(&1), "a broken session must not stop the round: {decided:?}");
    // Non-vacuity, at its place in the sequence: the sessions really did end, and something really
    // was lost with them.
    assert!(s.trace().session_ends() > 0, "no session ended, so nothing was ever retried");
    assert!(
        s.trace().drops_because(recon_sim::DropReason::NoSession) > 0
            || s.trace().suffix_losses() > 0,
        "the endings cost nothing, so the retry was never needed",
    );
}

#[test]
fn a_lost_p1a_does_not_leave_the_leader_waiting_for_ever() {
    // Liu et al.'s first leader violation. Driven by hand with exactly that message dropped, not
    // with lossy links switched on.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    // Force a fresh phase one, and lose every p1a it sends.
    let high = Ballot { round: 20, leader: C };
    h.preempt(A, high);
    assert!(h.at(A).is_scouting(), "A is in phase one");
    let ballot = h.at(A).leader_ballot();
    h.wire.retain(|(_, _, m)| !matches!(m, Wire::Synod(SynodMsg::P1a { .. })));
    assert!(!h.at(A).is_active(), "and nothing has adopted");

    // Ticks below the escalation threshold resend; the threshold restarts phase one. Either way
    // p1a must reach the wire again — the figure would wait for ever.
    h.advance(timing().detect_after * 2);
    h.tick(A);
    let resent = h
        .in_flight()
        .iter()
        .filter(|(from, _, m)| {
            *from == A && matches!(m, Wire::Synod(SynodMsg::P1a { ballot: b }) if *b == ballot)
        })
        .count();
    assert!(resent >= 2, "phase one must be reissued to a majority, got {resent} p1a");
    assert!(h.at(A).is_scouting(), "and the leader is still in phase one rather than stuck");

    // Now let the answers through: the round completes, which is what "not waiting for ever" means.
    h.settle_without_heartbeats_to(A);
    assert!(h.at(A).is_active(), "the restarted phase one must be able to adopt");
}

#[test]
fn a_lost_p2b_does_not_leave_a_slot_undecided() {
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);

    h.propose(A, 3, 333);
    // Deliver the p2a, but lose every p2b coming back.
    h.settle(|_, to, msg| {
        !(to == A && matches!(msg, Wire::Synod(SynodMsg::P2b { .. })))
            && !matches!(msg, Wire::Detector(_))
    });
    assert!(h.decisions_at(A).is_empty(), "no p2b arrived, so nothing is decided yet");
    assert!(h.at(A).commanded_slots().any(|s| s == 3), "the commander is still outstanding");

    // A sweep *before* an answer could have arrived resends nothing. The sweep interval and the
    // resend threshold are different things: the first decides how often the question is asked, the
    // second decides the answer, and conflating them is what had this protocol resending inside one
    // round trip.
    let resent_p2a = |h: &Hand| {
        h.in_flight()
            .iter()
            .filter(|(from, _, m)| {
                *from == A && matches!(m, Wire::Synod(SynodMsg::P2a { pvalue }) if pvalue.slot == 3)
            })
            .count()
    };
    h.wire.clear();
    for _ in 0..3 {
        h.advance(timing().retransmit);
        h.tick(A);
    }
    assert_eq!(
        resent_p2a(&h),
        0,
        "three sweeps inside one round trip must resend nothing — the threshold is {:?} and only \
         {:?} has passed",
        resend_after(),
        timing().retransmit * 3,
    );

    // Past the threshold, the pvalue goes again to whoever has not answered.
    h.wire.clear();
    h.advance(resend_after());
    h.tick(A);
    let resent = resent_p2a(&h);
    assert!(resent >= 2, "p2a must be resent to the acceptors that did not answer, got {resent}");

    // Let the answers through this time.
    h.settle_without_heartbeats_to(A);
    assert_eq!(h.decisions_at(A), vec![(3, 333)], "the slot must be decided once p2b arrives");
}

#[test]
fn a_lost_preempt_sends_the_leader_back_to_phase_one_rather_than_resending_for_ever() {
    // The third violation, and the one a naive design gets wrong: resending p2a cannot help once a
    // majority holds a higher ballot. Nothing but a return to phase one recovers.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    let stale = h.at(A).leader_ballot();

    // A majority moves to a higher ballot behind A's back.
    let higher = Ballot { round: stale.round + 5, leader: C };
    for peer in [B, C] {
        h.event(peer, Event::Msg { from: C, msg: Wire::Synod(SynodMsg::P1a { ballot: higher }) });
    }
    h.wire.clear();

    // A proposes, and every reply that would tell it it has been preempted is lost.
    h.propose(A, 5, 555);
    h.settle(|_, to, msg| {
        !(to == A && matches!(msg, Wire::Synod(SynodMsg::P2b { .. })))
            && !matches!(msg, Wire::Detector(_))
    });
    assert!(h.decisions_at(A).is_empty(), "a majority holds a higher ballot; nothing can decide");
    assert_eq!(h.at(A).leader_ballot(), stale, "and A has not learnt of it");

    // Resending would loop for ever. The escalation must put A back into phase one.
    h.wire.clear();
    h.advance(timing().detect_after * 2);
    h.tick(A);
    assert!(h.at(A).is_scouting(), "the leader must go back to phase one, not resend p2a");
    assert!(!h.at(A).is_active());
    let p1a = h
        .in_flight()
        .iter()
        .filter(|(from, _, m)| *from == A && matches!(m, Wire::Synod(SynodMsg::P1a { .. })))
        .count();
    assert!(p1a >= 2, "phase one must reach a majority, got {p1a} p1a");

    // And phase one is how the higher ballot is learnt — under the same ballot, not a fresh one.
    h.settle_without_heartbeats_to(A);
    assert!(
        h.at(A).leader_ballot() > higher,
        "the p1b answers must carry the higher ballot up to the leader: it is at {} and the \
         majority holds {higher}",
        h.at(A).leader_ballot(),
    );
}

#[test]
fn an_acceptor_reached_cold_by_a_retransmitted_p2a_still_counts_toward_the_decision() {
    // The route that motivated the acceptor departure, end to end: C's p1a never arrives, the
    // scout completes without it, and the retransmitted p2a reaches C having never seen phase one.
    let mut h = Hand::new(&THREE);
    for _ in 0..3 {
        h.advance(timing().detect_after);
        h.tick_all();
        h.settle(|_, to, msg| {
            !(to == A && matches!(msg, Wire::Detector(_)))
                && !(to == C && matches!(msg, Wire::Synod(SynodMsg::P1a { .. })))
        });
    }
    assert!(h.at(A).is_trusted() && h.at(A).is_active(), "A adopted with a majority of A and B");
    assert_eq!(h.at(C).adopted_ballot(), None, "C never saw phase one — the precondition");

    let ballot = h.at(A).leader_ballot();
    h.propose(A, 8, 888);
    // Now let everything through, including the p2a that reaches C cold.
    h.settle(|_, to, msg| !(to == A && matches!(msg, Wire::Detector(_))));

    assert_eq!(h.at(C).adopted_ballot(), Some(ballot), "C adopts what it accepts");
    // Under Figure 4's `b = ballot_num` C would have answered with ⊥-derived nothing and killed the
    // commander; here its answer counts.
    assert_eq!(h.decisions_at(A), vec![(8, 888)]);
    let split: Vec<_> = h.all_decisions();
    assert!(
        split.iter().all(|(_, s, c)| *s == 8 && *c == 888),
        "one decision, undivided: {split:?}"
    );
}

#[test]
fn the_send_rate_is_flat_once_the_work_is_done() {
    let mut s = sim_of(&FIVE, synchronous(17));
    for slot in 1..=5u64 {
        s.command(E, Cmd::Propose { slot, command: slot as u32 });
    }
    s.run_for(Duration::from_secs(2));
    assert_eq!(
        decisions(&s).len(),
        5,
        "the work must actually be done before the rate is measured"
    );
    // A decision retires its commander and leaves the sweep, so the sweep is empty once the slots
    // are decided; what remains is the detector's own heartbeat, which is flat by construction.
    common::assert_send_rate_flat!(s, Duration::from_millis(400), 4);
}

// ---------------------------------------------------------------- task 7: safety over runs

#[test]
fn at_most_one_proposal_is_chosen_per_slot_under_competing_ballots() {
    let mut s = sim_of(&FIVE, unreliable(23));
    // Every process is asked for a different command for the same slots. §4.4's forwarding funnels
    // them, so what this puts in the run is not five surviving proposals but the material for
    // them: while the run is split, each side's leader keeps its own.
    for (i, node) in FIVE.iter().enumerate() {
        for slot in 1..=3u64 {
            s.command(*node, Cmd::Propose { slot, command: (i as u32 + 1) * 1000 + slot as u32 });
        }
    }
    // A partition and a heal, so more than one process leads over the run, and session churn
    // throughout so messages are genuinely lost rather than merely delayed.
    s.run_for(Duration::from_millis(200));
    s.partition(&[&[A, B], &[C, D, E]]);
    churn(&mut s, 23, 4, Duration::from_millis(200));
    s.heal();
    s.deliver_session_events();
    churn(&mut s, 5, 6, Duration::from_millis(400));

    // Competing ballots on their own do not put a *contradicting* value in front of a leader.
    // Forwarding leaves one proposal per slot at whoever Ω trusts, and Ω trusts E throughout the
    // partition and after it, so every ballot here commands the same commands. A handover is what
    // supplies the contradiction: E crashes having got slots chosen, D takes over holding no
    // proposal for them, and is then asked for different commands. That is the case `pmax` settles,
    // and the whole of what makes ignoring it a split rather than a no-op.
    let chosen_under_e = decisions(&s);
    assert!(!chosen_under_e.is_empty(), "E must get something chosen before it hands over");
    s.crash(E);
    assert!(s.is_stopped(E), "the leader really went, and before the proposals that follow");
    for _ in 0..40 {
        s.run_for(Duration::from_millis(100));
        if s.at(D).is_trusted() {
            break;
        }
    }
    assert!(s.at(D).is_trusted(), "the detector must move to D before D can lead");
    for slot in chosen_under_e.keys() {
        s.command(D, Cmd::Propose { slot: *slot, command: 9000 + *slot as u32 });
    }
    let checked = run_checking(&mut s, Duration::from_secs(3));

    // The checker asserted agreement after every event; these are the non-vacuity halves, and each
    // sits at the point in the sequence that depends on it.
    assert!(!checked.chosen.is_empty(), "nothing was chosen, so agreement held vacuously");
    assert!(ballots_seen(&s).len() > 1, "one ballot ran, so no ballots competed");
    assert!(preemptions(&s) > 0, "no ballot was ever refused, so none of them collided");
    assert!(s.trace().session_ends() > 0, "no session ended, so nothing was ever lost");
    for (slot, command) in &chosen_under_e {
        assert_eq!(
            decisions(&s).get(slot),
            Some(command),
            "slot {slot} was chosen as {command} under E, and the successor must not move it",
        );
    }
}

#[test]
fn a_chosen_proposal_is_one_that_was_proposed_and_an_unproposed_slot_stays_empty() {
    let mut s = sim_of(&FIVE, unreliable(29));
    for slot in [1u64, 2, 4] {
        s.command(E, Cmd::Propose { slot, command: (slot * 7) as u32 });
    }
    churn(&mut s, 29, 4, Duration::from_millis(200));
    let checked = run_checking(&mut s, Duration::from_secs(4));

    // S2 is asserted inside the checker after every event; here is the slot nobody proposed for.
    assert!(!checked.chosen.contains_key(&3), "slot 3 was never proposed for and must stay empty");
    assert!(checked.chosen.contains_key(&1), "and the proposed slots must actually be chosen");
    assert_eq!(decisions(&s).get(&1), Some(&7));
}

#[test]
fn safety_survives_a_minority_crashing_and_never_returning() {
    // Crash-stop, which is the source's own model: the crashed processes are **not** restarted,
    // and safety is asserted over the survivors. A process that returned having forgotten what it
    // knew would be outside what this module claims — see the module documentation.
    let mut s = sim_of(&FIVE, synchronous(31));
    for slot in 1..=3u64 {
        s.command(E, Cmd::Propose { slot, command: slot as u32 });
    }
    s.run_for(Duration::from_millis(200));

    let decided_before = decisions(&s).len();
    s.crash(A);
    s.crash(B);
    assert!(s.is_stopped(A) && s.is_stopped(B), "the crashes really happened, and before the rest");

    for slot in 4..=6u64 {
        s.command(E, Cmd::Propose { slot, command: slot as u32 });
    }
    let checked = run_checking(&mut s, Duration::from_secs(5));

    assert!(
        checked.chosen.len() > decided_before,
        "the run must make progress after the crashes, or it proves nothing about them",
    );
    // Agreement was checked after every event by `run_checking`; this pins that the survivors did
    // the work rather than the run having stalled.
    for slot in 1..=3u64 {
        assert_eq!(decisions(&s).get(&slot), Some(&(slot as u32)));
    }
}

#[test]
fn a_value_chosen_under_a_crashed_leader_is_what_its_successor_proposes() {
    // The classic Paxos scenario, and the one the crash test above does not reach: the leader
    // changes hands *over a crash*, and the process taking over must not contradict a value the
    // crashed leader already got chosen. Above, Ω trusts the highest rank throughout, so E leads
    // before and after and no handover happens. Here E — the highest, and so the leader — chooses a
    // value and then crashes, and D, the next highest, becomes leader and must adopt what E chose.
    //
    // This exercises the value-adoption path (`pmax` after a genuine leadership change) end to end,
    // where the delayed-message test drives it by hand. A leader crashing after a value is chosen is
    // the case the whole intersection argument exists for.
    let mut s = sim_of(&FIVE, synchronous(41));

    // E leads. It gets 700 chosen for slot 7.
    s.command(E, Cmd::Propose { slot: 7, command: 700 });
    s.run_for(Duration::from_millis(300));
    assert_eq!(
        decisions(&s).get(&7),
        Some(&700),
        "700 must be chosen under E before it crashes, or the constraint below is vacuous",
    );

    // E crashes for good. A crash then no restart is amnesia, not a pause: E is gone.
    s.crash(E);
    assert!(s.is_stopped(E), "the leader really crashed, and before its successor proposes");

    // D is now the highest correct process, so Ω moves to it. Wait for that before asking it to
    // propose: a process that is not yet trusted forwards its proposal to the one that is, which
    // here is the crashed E — the loss that `a_proposal_forwarded_to_a_crashed_process_is_lost`
    // covers, and not what this test is about.
    for _ in 0..40 {
        s.run_for(Duration::from_millis(100));
        if s.at(D).is_trusted() {
            break;
        }
    }
    assert!(s.at(D).is_trusted(), "the detector must move to D before it can lead");

    // Command D a *different* command for slot 7. A correct successor must discover 700 in phase
    // one and propose that, dropping 999 — Figure 7's `proposals := proposals ◁ pmax(pvals)`
    // composed with the `∄c'` guard.
    s.command(D, Cmd::Propose { slot: 7, command: 999 });
    // And a slot nobody touched before, to prove D actually leads rather than merely not-splitting.
    s.command(D, Cmd::Propose { slot: 8, command: 800 });
    let checked = run_checking(&mut s, Duration::from_secs(6));

    // No split: slot 7 is 700 for every process that learned it, and 999 was never chosen for it.
    let learners = checked.chosen.get(&7).expect("slot 7 was chosen before the crash");
    for (node, command) in learners {
        assert_eq!(*command, 700, "{node} learned {command} for slot 7, not the chosen 700");
    }
    assert_eq!(
        decisions(&s).get(&7),
        Some(&700),
        "the successor must not overwrite a chosen value"
    );

    // Non-vacuity, at its place in the sequence: D really took over and did new work, and the run
    // really contained the competing command that a broken successor would have chosen.
    assert_eq!(
        decisions(&s).get(&8),
        Some(&800),
        "the successor must actually lead, not just defer"
    );
    assert!(
        s.trace()
            .invocations()
            .any(|(_, _, cmd)| matches!(cmd, Cmd::Propose { slot: 7, command: 999 })),
        "the contradicting proposal must really have been made for the constraint to mean anything",
    );
    assert!(ballots_seen(&s).len() > 1, "leadership must really have changed hands");
}

#[test]
fn a_slot_decided_twice_is_announced_twice_and_names_one_command() {
    // Invariant A5 in the only form the layer above can see it. A later ballot re-commands a slot
    // its predecessor already got chosen — `proposals := proposals ◁ pmax(pvals)` puts the decided
    // command back into the successor's proposals, and `adopted` commands everything there — so
    // the decision is announced a second time and every process raises `Ind::Decision` for the slot
    // again. Nothing here suppresses that, deliberately: see the module's note on why a `decided`
    // set kept per *receiving* process is the wrong place to pay for it. What the repetition must
    // never do is carry a different command, and that is what this pins.
    //
    // Registered against `synod-ignore-pmax`, and it goes red there through its non-vacuity half
    // rather than through a split: `pmax` is the whole reason a successor re-commands a slot it
    // never proposed for, so without it the second decision does not happen at all.
    let mut s = sim_of(&FIVE, synchronous(43));
    s.command(E, Cmd::Propose { slot: 7, command: 700 });
    s.run_for(Duration::from_millis(300));
    assert_eq!(decisions(&s).get(&7), Some(&700), "slot 7 must be chosen under E first");

    let announced_under_e = announcements(&s, 7);
    assert!(announced_under_e > 0, "E's own announcement must have happened before the handover");
    s.crash(E);
    assert!(s.is_stopped(E), "the leader really went, and before the re-command below");
    for _ in 0..40 {
        s.run_for(Duration::from_millis(100));
        if s.at(D).is_trusted() {
            break;
        }
    }
    assert!(s.at(D).is_trusted(), "the detector must move to D, whose adoption re-commands slot 7");
    run_checking(&mut s, Duration::from_secs(3));

    // The re-command really happened, or the assertion below holds for want of a second decision.
    assert!(
        announcements(&s, 7) > announced_under_e,
        "slot 7 must be decided a second time under D's ballot for this to say anything",
    );
    // And every announcement names 700. `run_checking` asserted S1 after every event; this is the
    // same statement in the form the layer above meets it — one command, however many arrivals.
    let commands: BTreeSet<u32> = s
        .trace()
        .indications()
        .filter_map(|(_, ind)| match ind {
            Ind::Decision { slot: 7, command } => Some(*command),
            _ => None,
        })
        .collect();
    assert_eq!(commands, BTreeSet::from([700]), "slot 7 was announced as {commands:?}");
}

#[test]
fn a_majority_that_has_taken_up_a_ballot_cannot_afterwards_accept_a_lower_one() {
    // The acceptor's promise, and the only thing standing between this run and a split slot.
    //
    // A leads at `stale` and commands slot 1 with 111, but every one of those requests is *held* —
    // delayed past what happens next rather than lost, which is what makes A a stale leader that
    // does not know it. C then takes up a higher ballot, sees nothing accepted for slot 1, and
    // gets 999 chosen. Only then are A's requests released. An acceptor that honours its promise
    // refuses them and answers with the ballot it now holds, so A is preempted and decides
    // nothing. One that does not would give A a majority for 111, and the slot would hold two
    // values at once.
    let mut h = Hand::new(&THREE);
    h.make_active_leader(A);
    let stale = h.at(A).leader_ballot();

    h.propose(A, 1, 111);
    h.hold(|_, _, m| matches!(m, Wire::Synod(SynodMsg::P2a { pvalue }) if pvalue.slot == 1));
    assert!(h.at(A).commanded_slots().any(|s| s == 1), "A really is commanding slot 1");

    // C climbs above A and runs a full phase one. It is the highest process, so Ω trusts it.
    assert!(h.at(C).is_trusted(), "C is trusted, which is what lets it lead at all");
    h.preempt(C, Ballot { round: stale.round + 1, leader: E });
    let fresh = h.at(C).leader_ballot();
    assert!(fresh > stale, "C's ballot must be above A's for this to be the stale-leader case");
    h.settle_without_heartbeats_to(A);
    assert!(h.at(C).is_active(), "C completed phase one");

    // Nothing was accepted for slot 1 yet — A's requests are still held — so C is free to propose
    // its own command, and gets it chosen.
    h.propose(C, 1, 999);
    h.settle_without_heartbeats_to(A);
    assert!(
        h.decisions_at(C).contains(&(1, 999)),
        "C must get 999 chosen for slot 1, got {:?}",
        h.decisions_at(C),
    );

    // Now A's stale phase two lands on acceptors that have moved on.
    h.release();
    h.settle_without_heartbeats_to(A);

    // A learns the outcome directly now that a commander announces its decision to everyone, which
    // is a better answer than the preemption it used to get: its commander for the slot retires
    // against a decision nothing can change. What matters is unchanged — it must not decide 111.
    assert!(
        h.decisions_at(A).contains(&(1, 999)),
        "A must learn what was actually chosen for slot 1, got {:?}",
        h.decisions_at(A),
    );
    let per_slot: BTreeSet<u32> =
        h.all_decisions().iter().filter(|(_, s, _)| *s == 1).map(|(_, _, c)| *c).collect();
    assert_eq!(
        per_slot,
        BTreeSet::from([999]),
        "slot 1 was split: a majority that had taken up {fresh} accepted something under {stale}",
    );
}

#[test]
fn the_safety_suite_is_not_vacuous() {
    // "At most one chosen" is satisfied by a run that chooses nothing, and "no two disagree" by a
    // run with one leader. Both halves are asserted here, at the point in the schedule that
    // depends on them, and for both roles: the leader that preempted and the one preempted.
    let mut s = sim_of(&FIVE, unreliable(37));
    for slot in 1..=3u64 {
        s.command(A, Cmd::Propose { slot, command: slot as u32 });
        s.command(E, Cmd::Propose { slot, command: (slot + 50) as u32 });
    }
    s.run_for(Duration::from_millis(300));
    s.partition(&[&[A, B, C], &[D, E]]);
    run_checking(&mut s, Duration::from_secs(2));

    // While partitioned, the minority side cannot reach a majority: a leader there is preempted or
    // stalled, and the majority side carries on. Both roles exist in this run by construction.
    s.heal();
    let checked = run_checking(&mut s, Duration::from_secs(4));

    assert!(!checked.chosen.is_empty(), "something must actually have been chosen");
    assert!(ballots_seen(&s).len() > 1, "the run must contain competing ballots");
    assert!(preemptions(&s) > 0, "and a preemption must really have happened");
    // Both roles: somebody's ballot was refused, and somebody did the refusing.
    let refusers: BTreeSet<NodeId> = s
        .trace()
        .sends()
        .filter_map(|(from, _, m)| match m {
            Wire::Synod(SynodMsg::P1b { .. }) | Wire::Synod(SynodMsg::P2b { .. }) => Some(from),
            _ => None,
        })
        .collect();
    assert!(refusers.len() >= 3, "a majority must have answered for any of this to mean anything");
}

#[test]
fn safety_holds_across_a_sweep_of_seeds() {
    // Breadth, where the hand-driven schedules give depth. A failure here reports its seed, and the
    // seed replays the run exactly.
    for seed in 100..112u64 {
        let mut s = sim_of(&FIVE, unreliable(seed));
        for (i, node) in FIVE.iter().enumerate() {
            for slot in 1..=2u64 {
                s.command(
                    *node,
                    Cmd::Propose { slot, command: (i as u32 + 1) * 100 + slot as u32 },
                );
            }
        }
        s.run_for(Duration::from_millis(200));
        s.partition(&[&[A, B], &[C, D, E]]);
        churn(&mut s, seed, 3, Duration::from_millis(200));
        s.heal();
        s.deliver_session_events();
        churn(&mut s, seed + 3, 4, Duration::from_millis(300));

        // The handover, for the reason spelled out in
        // `at_most_one_proposal_is_chosen_per_slot_under_competing_ballots`: forwarding funnels
        // every proposal to the process Ω trusts, so until leadership moves there is only one
        // command per slot in the run and nothing for `pmax` to have to settle. E leads, so E is
        // what has to go.
        let chosen_under_e = decisions(&s);
        assert!(
            !chosen_under_e.is_empty(),
            "seed {seed} chose nothing under E, so the handover contradicts nothing",
        );
        s.crash(E);
        assert!(s.is_stopped(E), "seed {seed}: the leader really went, before the proposals below");
        for _ in 0..40 {
            s.run_for(Duration::from_millis(100));
            if s.at(D).is_trusted() {
                break;
            }
        }
        assert!(s.at(D).is_trusted(), "seed {seed}: the detector must move to D before D can lead");
        for slot in chosen_under_e.keys() {
            s.command(D, Cmd::Propose { slot: *slot, command: 900 + *slot as u32 });
        }
        let checked = run_checking(&mut s, Duration::from_secs(3));
        assert!(
            !checked.chosen.is_empty(),
            "seed {seed} chose nothing, so it proved nothing about agreement",
        );
        assert!(s.trace().session_ends() > 0, "seed {seed} never lost a message");
        for (slot, command) in &chosen_under_e {
            assert_eq!(
                decisions(&s).get(slot),
                Some(command),
                "seed {seed}: slot {slot} was chosen as {command} under E and then moved",
            );
        }
    }
}

mod common;
