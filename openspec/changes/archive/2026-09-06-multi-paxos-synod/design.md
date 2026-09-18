## Context

See `proposal.md` — Why, for the motivation and for why this source rather than Kirsch & Amir.

What shapes the approach here is that the source describes five *processes* that spawn threads —
replica, acceptor, leader, scout, commander — and this repository has no threads, no spawning, and a
composition model built for layers rather than for siblings. §4.4 of the paper says the roles are
co-located on one machine in practice, which is the shape to build; the question this design settles
is which of them are protocols and which are bookkeeping.

Three constraints from `CLAUDE.md` bear directly:

- **Constraint 4** — compose statically, and extract the framework only after two or three hand-written
  consumers. Using machinery because it exists is the failure this repository already made.
- **A timer is a handle, not a type**, so a layer that registers a timer compares before acting, and
  an expiry is offered to every layer.
- **Identity is as durable as the state it keys.** Ballot numbers cross the wire, so their generator
  is state with a scope, and the scope has to be stated.

## Goals / Non-Goals

**Goals:**

- One module readable against Figures 4, 6 and 7 of the source, with the pseudocode quoted above it.
- Safety that holds unconditionally, asserted over runs containing competing ballots and crashes.
- Progress claimed only where Ω settles, with the condition stated rather than assumed.
- A shape that change 2 can put a replica on top of without rework.

**Non-Goals:**

- Any per-slot ordering, `slot_num`, or client-facing log. There is no `Replica` here; the leader is
  driven directly by the suite, and slots are just numbers it is asked to fill.
- Matching the source's process decomposition where this repository's own model contradicts it. The
  algorithm is what must be faithful, not the concurrency structure of a paper written for threads.

## Decisions

### Scouts and commanders are state inside the leader, not child protocols

The proposal guessed they would be a run-time family of `Child`ren keyed by ballot and slot, and
that `KeyedSlot` was what they needed. Reading the figures says otherwise, and the deciding question
is the one `link.rs` already asks: does this thing have a vocabulary of its own?

It does not. An acceptor replies to the **leader**, not to the scout: Figure 4 sends
`⟨p1b, self(), ballot_num, accepted⟩` to `λ`, and `⟨p2b, self(), ballot_num⟩` to `λ`. Scouts and
commanders own no link, no timer, and no durable record. What a scout is, concretely, is a `waitfor`
set and a union of pvalues; what a commander is, is a `waitfor` set. They are the leader's
bookkeeping over responses it is already receiving, and the paper models them as threads because it
is written in a language where a thread is the cheapest way to say "wait for a majority".

So: `scout: Option<Scout>` and `commanders: BTreeMap<Slot, Commander>` as plain fields. The paper
also constrains their number in ways worth encoding — a leader runs at most one scout, "only for its
own ballots", and at most one commander per `(ballot, slot)` (Invariant C1). One optional scout and a
map keyed by slot within the current ballot says both in the types.

*Alternative considered:* make them `Child<Scout>` and a `BTreeMap<(Ballot, Slot), Child<Commander>>`,
using the family machinery built two changes ago. Rejected because the wrap function would have
nothing to wrap — the child would emit messages in the parent's own vocabulary — and because it would
put the `on_init`-on-creation hazard into the one module that creates the most children. The
machinery keeps its consumer in `logged_uniform_total_order_broadcast`; it does not need a second one
here to justify itself.

### Acceptor and leader are one protocol, not two

Per §4.4 the roles are co-located, and this repository's unit of composition is one protocol per
process. `MultiPaxosSynod` holds both roles as fields of itself rather than as children, for the same
reason as above: neither has a vocabulary the other does not, and both send and receive on the same
wire.

The wire is one enum with four variants — `P1a`, `P1b`, `P2a`, `P2b` — carrying ballots and pvalues,
which is the whole of Figures 4 and 6. A layer that adds no per-hop state adds no wire field, and
this layer adds the ballot because the ballot is its own concept.

*Alternative considered:* separate `Acceptor` and `Leader` protocols composed as children of a
third. Rejected as two extra wrap functions and a message enum with a `Left`/`Right` split that buys
nothing, since every message from either goes to the same peers over the same link.

### Ω replaces the source's pinging, and the leader is passive by default

§3 has a preempted leader ping the preempting one and back off with an AIMD timeout, and says
outright that "this concept is called failure detection". This module composes
`Child<EventualLeaderDetector>` and acts on `Trust`: start a scout for the next ballot when trusted,
stay passive otherwise. A `preempted` message updates `ballot_num` past the ballot that beat it but
does **not** start a scout unless this process is still trusted — which is what stops the duel the
source's §3 opens by describing.

The difference to state in the module: Ω names *one* leader, where the paper's scheme lets any
correct leader win a race and simply makes the loser wait longer each time. Ω is the stronger
assumption and the cheaper mechanism, and the chain it rests on is already written down link by link
in `docs/conditional-guarantees.md`.

*Alternative considered:* transcribe §3. Rejected as a second failure detector in the tree with its
own timeout knob interacting with `detect_after`, to reimplement something already tested against a
detector that lies.

### Ballots are `(round, NodeId)`, and their scope is the incarnation

Lexicographic pairs, as §2 has them, so a ballot names its leader and any two are comparable. The
round counter is volatile, and the module says so: its scope is **this incarnation**, and the source
is what makes that sufficient, because in its model a process that returns without its state has not
recovered — it is a crashed process making a transition it is not permitted. See the spec
requirement covering the boundary.

What the module must not do is pretend the simulator cannot produce that case. It can, and Ω will
trust such a process again, and the consequence is concrete: a re-minted ballot, an acceptor still
holding it, and a second proposal accepted under one ballot and slot. The module documents that, and
names the durable counter as what a fail-recovery variant buys.

*Alternative considered: give a recovered process a fresh identity, so it cannot re-mint a ballot it
has already used.* This works for one of the two roles and fails for the other, and the split is
worth recording because it explains why the source reaches for a disk instead.

**As a leader it is sound.** Ballot uniqueness is a proposer-side obligation, and the proposer set
does not have to be fixed for safety — any process may propose under any ballot provided no ballot
is ever reused for two different proposals. A returning process that leads under a new identity mints
`(0, E′)`, which no acceptor has ever seen, so the hazard above disappears. The cost is that the
fresh identity has to come from somewhere monotonic across incarnations, and with no disk that means
an external source — a boot id, an incarnation number handed in at construction. That is a real
assumption rather than a free one, and it is the same shape as the session-epoch idea in the
reference notes: an identifier that increments on restart so the layer above is told this is a new
incarnation rather than the old one continuing.

**As an acceptor it breaks safety, and not subtly.** The acceptor set is what majorities are counted
over, and changing it without a reconfiguration protocol destroys the intersection the whole argument
rests on. With acceptors `{A, B, C, D, E}` and majorities of three: `{A, B, E}` accepts `⟨b, s, p⟩`,
so `p` is chosen. `E` returns as `E′`. A later leader's scout collects from `{C, D, E′}` — a majority
of the new set — and none of them has ever seen `⟨b, s, p⟩`, so `pmax` yields nothing for `s` and the
leader is free to propose something else, which that same majority accepts. Two values chosen for one
slot. The two majorities do not intersect, because they are majorities of different sets.

So a returning process could lead under a fresh identity but must not be counted as an acceptor until
a reconfiguration admits it, and until then the cluster runs with one fewer acceptor than it was
configured for — it tolerates one fewer failure, silently, which is the kind of degradation worth
refusing rather than shipping. Keeping acceptor state on disk is cheaper than a reconfiguration
protocol, which is why the source gives §4.3, *Keeping State on Disk*, instead. The idea is not wrong; it is
a membership change wearing a disguise, and membership changes are their own algorithm — which this
source has, so the disguise is unnecessary. §5's Cheap Paxos does exactly this and does it safely:
it "reconfigures the system replacing the suspected acceptor with a fresh one", through the
reconfiguration command rather than by swapping an identity underneath a fixed quorum. A returning
process rejoining as a new acceptor is legitimate when a reconfiguration admits it and unsound when
it just appears.

### The link is a type parameter defaulting to a session link

`MultiPaxosSynod<L: Link = SessionLink<..>>`, following every other composing layer here. The
default is the session link rather than the perfect link because the real-world set's first
obligation is running over one, and this is the first module here that can meet it before joining
rather than after. `Boundary::Ended` is classified and propagated; this layer bridges nothing,
holding no redundancy that outlives a session beyond the other processes.

### Three liveness fixes from the cross-check, applied rather than discovered later

Liu, Chand and Stoller (2019) specify this same algorithm in DistAlgo, prove it in TLA+, and report
four liveness violations in the vRA specification **when messages can be lost**. That condition is
not hypothetical here: this module runs over a link that loses messages, so all four are reachable
and three are inside this change.

| Where | What is lost | What happens | What the leader must do |
|---|---|---|---|
| Phase 1 | `p1a` | No `p1b` majority and no preemption ever arrives, so the leader waits for ever | time out the wait and start phase 1 again |
| Phase 2 | `p2b` | No decision for that slot; if it happens at every leader, the replicas above stall too | resend `p2a` for that slot after a timeout |
| Phase 2 | `preempt` | A majority has moved to a higher ballot, so `p2a` can never reach one — the leader sends for ever and decides nothing | start **phase 1** again after a timeout, not merely resend |

The fourth is in the replica and belongs to change 2: if no decision arrives for a slot, every
replica stops applying from that slot, `slot out` stops moving, `WINDOW` fills, and the system
wedges. Its fix is for a replica to re-propose after a timeout. Recorded here so change 2 does not
have to rediscover it.

The third of these is the one worth naming, because it is what a naive design gets wrong. Resending
`p2a` cannot help once a majority holds a higher ballot; the leader has to go back to phase 1. A
single retransmission sweep over unanswered requests — which is what an earlier draft of this design
described — recovers from the first two and loops for ever on the third.

The same paper has a fourth finding against these figures that is not a liveness violation, the
useless reply from the acceptor, and it changes a line of Figure 4 here; it gets the next decision
to itself.

### The acceptor accepts under `b ≥ ballot_num`, which is the report's condition rather than the survey's

The survey's Figure 4 accepts a pvalue only when `b = ballot_num`, and its Figure 6 commander
treats any `p2b` naming a different ballot as a preemption. The two only compose because §2.3
asserts that every `p2b` carries `b′ ≥ b`, and that assertion rests on the acceptor having seen
phase 1 before phase 2, which this module's link does not provide. The case is concrete: an
acceptor's `p1a` dies at a session ending, the scout completes with a majority that excludes it,
and the retransmitted `p2a` finds `ballot_num < b`. Under the survey's text the acceptor refuses
and replies with its own lower ballot; the commander reads the mismatch as a preemption and exits;
the leader ignores the preemption because the ballot in it is below its own; and the retry sweep no
longer covers the slot, because the `waitfor` set that drove it left with the commander. The slot
stalls until the phase-two escalation notices, a whole timeout after the run held the majority it
needed.

So the acceptor here accepts under `b ≥ ballot_num` and adopts `b` in the same transition. That is
the condition the 2011 report's Figure 2 uses, and it is the fix Liu et al. give for what they name
the useless-replies issue in this acceptor. Every `p2b` then names a ballot at least as high as the
request's, the commander's else-arm is a genuine preemption again, and the property §2.3 asserts
without support holds here by construction. The departure is stated in the module beside the quoted
Figure 4, with both editions named: this line is the one place in §2 where the two editions differ
materially, which is why task 1.1 pins the edition before anything is transcribed.

*Alternatives considered:* keep the survey's condition and have the commander ignore a `p2b` naming
a lower ballot — rejected because the reply stays useless (Liu et al.'s point) and the acceptor
stays unable to help with that ballot until the next phase 1; keep both figures verbatim and key
the phase-two escalation to proposed-but-undecided slots rather than to live commanders — rejected
as the most faithful reading and the slowest recovery, turning a reachable case into a full timeout
plus a rerun of phase 1.

### What folding the threads into fields actually costs: two more departures

Both were found by building it, and both have one cause. A thread has an identity, and that identity
is doing two jobs the figures never name: it routes a reply to the attempt that made the request,
and it makes a reply to an attempt that has **exited** unroutable. A field has neither.

**A `p2b` must name its slot.** Figure 4 answers with `⟨p2b, self(), ballot_num⟩`, which is enough
for a commander thread because the reply is addressed to it. A leader running commanders for several
slots at once cannot tell from `⟨p2b, α, b⟩` which of them answered, so the slot travels in the
message. No guarantee changes — the slot was already determined by the request the reply answers —
and `p1b` needs no equivalent, because a leader runs at most one scout.

**A reply naming a ballot below the attempt's is stale and must be discarded.** This is the half
that bites, and taking Figure 6 literally is a liveness bug that was measured: a leader is preempted,
climbs, and starts a scout for the new ballot; the second acceptor's answer to the *old* ballot then
arrives, fails `b' = b`, takes the `else` arm, and discards the running scout while reporting a
preemption the leader correctly ignores as beneath its current ballot. The leader ends with no scout,
not active, and nothing in the sweep to restart it. It stops for ever while Ω goes on trusting it.
So a reply is classified against the attempt it reaches: above is a preemption, equal is an answer,
below is an answer to an attempt that has already exited. Under the figures' own model the third case
cannot arise, which is why they do not name it.

Neither was foreseen here, and that is worth recording rather than smoothing over: the decision to
make scouts and commanders fields was right, and its price was two clauses the source had no reason
to write down.

### Timers

One periodic timer, for retransmitting `p1a` and `p2a` to acceptors that have not answered, **and**
for the two escalations above: a phase-one attempt that has neither adopted nor been preempted
restarts, and a phase-two attempt that has neither decided nor been preempted goes back to phase
one rather than resending indefinitely. Both escalations rerun phase one under the **same**
ballot. For a lost `p1a` the rerun is idempotent: an acceptor that already adopted the ballot
answers again and the scout recollects. For a lost preemption the rerun is how the leader learns
what it missed: acceptors that moved answer `p1b` naming their higher ballot, the scout reports
that as a preemption, and only then does `ballot_num` move. Minting a higher ballot on timeout was
considered and rejected — it discards phase-two work already accepted under the current ballot and
learns nothing a same-ballot rerun does not. The
source assumes messages between non-faulty processes are eventually delivered and leaves
retransmission implicit; a session link does not resend across an ending, so the leader owns the
retry. The timer is compared against the registered `TimerId` before acting, as the convention
requires.

The retry set is the union of the outstanding `waitfor` sets. Each is bounded by membership, but
their number is not: it grows with the slots proposed and not yet decided, so the sweep is bounded
by membership times the slots in flight, and a stalled leader with many slots open resends in
proportion to them. What retires a commander is its decision, so once the work completes the sweep
is empty — which is the window `assert_send_rate_flat!` measures. Unlike the stubborn children
elsewhere, nothing here resends history: a decided slot leaves the sweep, and only membership and
the undecided frontier set its size.

### How the suite earns its verdicts

The simulator is the standard of evidence here, and this suite leans on it four ways. Each is a
decision now so the apply phase does not improvise one.

**Safety is checked at every step, not at the end of the run.** The agreement property compares
what observers learned, and a comparison made once at the end points at nothing when it fails. The
suite keeps a checker beside the sim, fed from the trace, holding per-acceptor promise history and
per-slot chosen sets, and asserts after every event: promises only rise (A1), accepts happen only
at the promise (A2), one command per ballot and slot (C1), and per-slot agreement. A violation then
names the first event that broke it, and the seed replays it.

**Schedules come from two sources, and both are needed.** Seeded sweeps — the same property checks
run across a batch of seeds with a partition window and **session churn** switched on — find the
interleavings nobody thought of, and the failing seed is the reproduction. Churn rather than
`Config::loss`, and this is a trap the first draft of the suite fell into: in session mode the
simulator applies no loss, duplication or reordering whatever, because that is what a session *is*.
A sweep configured with `.loss(0.1).sessions()` runs on a perfectly clean network while reading as
adversarial, which is exactly the silent vacuity this repository keeps finding. The way to lose a
message under a session link is to end the session, so every such test asserts `session_ends() > 0`
at its place in the sequence. Hand-driven
`step_with` schedules cover the edges randomness rarely lands on: adoption at exactly the majority
and not one fewer, two leaders alternating phases over one slot, a preemption arriving between a
majority's last `p2b` and the decision being noticed, the stale-`p2a` route of the acceptor
departure. The sweep is evidence of breadth, the hand-driven schedules of depth, and neither
substitutes for the other.

**Every safety claim has a sabotage that must turn it red.** The durability guard's instrument
generalises: break the thing a test claims to protect and require the red. Two feature-gated
mutations, compiled only for this check, each remove one clause the safety argument rests on — a
leader that ignores `pmax` and proposes what it set out to propose, and an acceptor that accepts
below its promise. The safety suite runs under each, and every registered test must fail; one that
stays green is not testing what it says. Agreement makes the same silent substitution available
that durability did — a run with one settled leader satisfies it vacuously, whatever the code
does — which is exactly the case the instrument exists for. Like the durability check, this
rebuilds the crate per mutation and lives in its own script rather than in `check.sh`.

**Non-vacuity counters have places in the sequence.** The counters the method requires — something
was chosen, ballots really competed, a preemption really happened, a crash really landed before
the step that depends on it — are asserted at their place in the schedule, not once at the end,
per the convention that caught a death counted after the recovery it was meant to justify. And
both roles get them, per the both-roles rule: the preempting leader and the preempted one, the
acceptor that refused and the leader it refused.

## Risks / Trade-offs

- **Scouts and commanders as plain state diverges structurally from the figures** → the module quotes
  the figures and states the mapping explicitly: which fields are the scout's, which the commander's,
  and where each `for ever / switch receive` arm went. The repository's method is reading code
  against its quoted contract, so the mapping is part of the contract rather than a note.

- **Ω's answer and the ballot's leader can disagree** → a process trusted by Ω that keeps being
  preempted by a higher ballot from a process Ω does not trust will escalate on each `preempted`.
  This terminates once Ω settles, which is exactly the conditional the spec states, but a test should
  drive the disagreement rather than assume it does not happen — a run where the detector is wrong
  for a while and the ballots reflect it.

- **A crash-stop module in a repository with a fail-recovery simulator** → nothing prevents a test
  from restarting one of these and getting an unsafe run. Mitigated by the spec requirement, the
  module documentation, and a suite that crashes without restarting. Not mitigated by the type
  system, and that should be said plainly rather than implied.

- **Safety is a property over a whole run, not an assertion at a point** → the suite has to collect
  what was chosen, per slot, across every process and every ballot, and compare. That needs the trace
  rather than protocol state, since the point of the property is that no *pair* of observers
  disagrees. Same shape as the total-order suite's agreement property.

- **Non-vacuity is harder here than usual** → "at most one proposal chosen per slot" is satisfied by
  a run that chooses nothing, and "no two disagree" by a run with one leader. Both halves need
  asserting: that something was chosen, and that the run really contained competing ballots and a
  preemption. `tests/method.rs` is the precedent.

## Open Questions

- **How many slots should the safety suite drive?** Enough that a leader has several commanders in
  flight at once, which is what exercises Invariant C1, but the number is a tuning question that does
  not change the specs or the tasks.

- **Whether the retry timer is per-outstanding-request or one sweep.** One sweep is simpler and
  bounded the same way; per-request gives tighter timing. Decidable when the retransmission test is
  written. Either way it carries the three escalations above, which is a question of what the timer
  does rather than how many there are.

## References

- van Renesse, R. and Altinbuken, D. (2015) 'Paxos Made Moderately Complex', *ACM Computing
  Surveys*, 47(3), pp. 1–36. doi:10.1145/2673577. **The source this module transcribes.** Figures 4
  (acceptor), 6 (commander and scout) and 7 (leader); §2 for the protocol, §3 for liveness, §4 for
  the pragmatics that change 3 will need — §4.1 state reduction, §4.2 garbage collection, §4.3
  keeping state on disk, §4.4 colocation, §4.5 read-only commands, §4.6 exercises — and §5 for the
  variants. The section numbering differs from the 2011 report's, which is one reason the edition has
  to be named.
- van Renesse, R. (2011) *Paxos Made Moderately Complex*. Technical report. Cornell University.
  The earlier edition of the same title, superseded here and **not** what is quoted: its figures are
  numbered differently and its §4.2 differs materially from the survey's.
- Liu, Y.A., Chand, S. and Stoller, S.D. (2019) 'Moderately Complex Paxos Made Simple', in
  *Proceedings of the 21st International Symposium on Principles and Practice of Declarative
  Programming*, pp. 1–15. doi:10.1145/3354166.3354180. The cross-check: a DistAlgo specification of
  the same algorithm with machine-checked TLA+ safety proofs, and the source of the three liveness
  fixes above.
- Kirsch, J. and Amir, Y. (2008) *Paxos for System Builders*. Technical Report CNDS-2008-2. Johns
  Hopkins University. Considered and not chosen; `proposal.md` says why. Worth reading for its
  Figures 6, 7 and 14, which specify leader election, the prepare phase and recovery more fully than
  the survey does.
- Lamport, L. (2001) 'Paxos Made Simple', *ACM SIGACT News*, 32(4), pp. 51–58. The `α` sketch that
  the survey's `WINDOW` makes concrete.
- Ongaro, D. and Ousterhout, J. (2014) 'In Search of an Understandable Consensus Algorithm', in
  *Proceedings of the 2014 USENIX Annual Technical Conference*, pp. 305–319. Not a source for this
  module; named because its §6 is the other fully specified membership change, should the survey's
  reconfiguration prove awkward to transcribe.
