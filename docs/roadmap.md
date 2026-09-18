# What comes next

This was the `Next` section of `README.md` until 2026-09. It is kept whole because the items record
decisions as well as intentions; the README carries a summary.

Two tracks. The **protocol** track continues the book's sequence; the **evidence** track builds what
would let a failure be found rather than anticipated. They are independent until the last item,
where they meet.

```
protocol track                        evidence track
──────────────                        ──────────────
1. accrual detector                   A. non-transitive partitions        ✓ built
2. defensive re-announcement          B. per-node clocks, and skew
3. bounding what grows                C. invocations in the trace          ✓ built
4. a replicated-log port              D. indeterminate outcomes
5. multi-Paxos: Synod ✓ built         E. shrinking                        ✓ built
   ZAB         ├── over it ───┐       F. logging and tracing              ✓ built
   VR, Raft…   ┘              │       G. a concurrent workload
                              └───────┴──▶  H. a checker, written once
                                             against the port
```

`H` is the only item needing both tracks, and it is last for a reason given under it.

**The evidence track is shared infrastructure, and that is what makes it worth its cost.** A
history model, a nemesis, a shrinker and a checker are built once and then serve multi-Paxos, ZAB,
viewstamped replication and anything else that replicates a log — the same way `link.rs` serves
every broadcast and `detector.rs` now serves Ω. None of it is scaffolding for one protocol.

## What "Jepsen tests" means here, and what it does not

Not the tool. Jepsen is Clojure, drives a real cluster over SSH, and cannot reproduce a failure from
a seed — all three are things constraints 1, 2 and 3 rule out for now. What is worth having is its
**discipline**: record a history of client operations as intervals, inject faults while it runs, and
then *check the history against a model* rather than against a property somebody thought to assert.

Measured against that, most of the machinery is already here. The simulator is a better nemesis host
than a real cluster — seeded, reproducible, and with faults a real cluster cannot express, such as
`crash_on_next_write`, which kills a process *inside* the write and lets the seed decide whether it
landed. What is missing is the history and the checker, and items `C` to `H` are that.

## 1. An accrual detector

Algorithm 2.7 adapts its timeout by adding Δ on every false suspicion and never subtracting, which
is a ratchet: one bad period leaves detection permanently sluggish, and nothing reports that it has.
`eventually_perfect_failure_detector` fixes the ratchet and caps the growth. The shape a deployment
actually wants goes one further — a **φ-accrual** detector (Hayashibara et al., 2004; Cassandra and
Akka both use one) keeps a bounded window of inter-arrival times and reports a *suspicion level*
rather than a verdict, leaving the threshold to the caller. That is the point of it: Ω picking a
leader can be aggressive, because being wrong costs one epoch, while a layer deciding to stop waiting
for an acknowledgement must be conservative, because being wrong costs safety. One detector, a
threshold each, priced by what a mistake costs that caller. A boolean detector forces one answer on
everybody. It needs the detector port `decreasing-eventually-perfect-detector` built.

## 2. Defensive re-announcement of standing facts

The current leader, the current epoch, the membership: things that are *state*, not events, and that
a process joining late or recovering has no way to ask for. `epoch_change` already carries one repair
for this — a process tells a leader where it has reached, because an edge-triggered `⟨Ω, Trust⟩`
never tells a leader that never changed its mind — and that repair is narrow, reactive, and was found
by a test failing rather than by design. The general form is that a holder of a standing fact
re-announces it periodically, so a process that missed the edge converges without anyone having had
to anticipate how it missed it. It costs standing traffic against a stack that is otherwise silent
when idle, so it wants measuring rather than assuming, and it belongs to the real-world set.

**The larger risk is not the traffic. It is that re-announcement hides the bugs it repairs.** This
repository has the instance: the leader trusted by everyone that announced nothing was found
*because nothing repaired it*. The stack deadlocked and stayed deadlocked, so a test could see it.
Add periodic re-announcement and that run converges after one period — the test passes, and the
defect beneath it sits there unfound until it resurfaces as latency, or under a longer period, or in
the one configuration nobody ran. A safety net that catches silently is a bug-concealer.

So the mechanism is only half of it. **A repair that is recorded and asserted on is a detector rather
than a concealer**, and item `F` is what makes that possible: a re-announcement that actually changed
something at the receiver narrates it, and the correctness suites assert the count. The assertion
cannot be "never" — a process that was crashed or unreachable genuinely missed the edge, and
repairing it is the feature working. The sharp form is attributable, and the trace holds everything
it needs:

> A repair is legitimate when the receiver **could not** have had the edge — down, unreachable, or
> newly joined across the interval in which the edge was sent. A repair to a process that was up and
> connected throughout is a bug upstream, and the test fails.

A fault-free run therefore asserts zero repairs, and a faulted run asserts every repair falls inside
a fault interval that explains it. The precedent is already here and already passes:
[`epoch_change`'s suite](../crates/recon-protocols/tests/epoch_change.rs) requires that a settled stack
sends no reports at all. Generalising the mechanism without generalising that assertion is the way
this item goes wrong.

Two consequences. Its acceptance criteria include the attribution test, not only the traffic
measurement. And it depends on `F` rather than being independent of it, which is one more reason `F`
came first.

## 3. Bounding what actually grows

This item used to read "`Stop`", on the reasoning that every logged protocol inherits an unbounded
outstanding set from its stubborn children because nothing retires a transmission. The premise is
true — **nothing in the repository calls `Stop`, on either the link or the broadcast** — but the
conclusion had the causation backwards, and the item was aimed at the wrong target.

A stubborn link's outstanding set is unbounded *because that is its specification*. Module 2.2's
`SL1` is that a message sent once is delivered "an infinite number of times". A link that stopped
would not be a bounded stubborn link, it would not be a stubborn link, and there is no such thing as
a bounded one. Nothing would ever run it either; the answer to wanting a bounded link is a session
link, which already exists here. See [`docs/bounded-space.md`](bounded-space.md), whose table
had marked this as a defect and no longer does.

What genuinely grows without bound, and is the real content of this item:

| | grows with |
|---|---|
| `perfect_link.delivered` | messages ever received |
| `reliable_broadcast.delivered` | messages ever delivered |
| `uniform_reliable_broadcast` — `pending`, `ack`, `delivered` | messages ever seen |
| `logged_link.delivered`, `logged_uniform_reliable_broadcast` | messages ever seen, **in stable storage** |

`Stop` is no help with any of them: they are all *receiver* state, and a sender letting go of a
transmission does not tell a receiver it may forget having seen it. The mechanism is the one
`docs/bounded-space.md` already describes — a delivered **cursor** rather than a delivered set, so
an indication says "everything up to sequence `n`" and the state is bounded by membership. It costs
per-sender ordering, which the link beneath does not currently promise, and it weakens the guarantee
to a scope. That is a change with a proposal.

`Stop` remains worth wiring where a layer genuinely knows a transmission is finished — an epoch that
aborted or decided — and it costs no property, because `SL1` is conditioned on a sender that sends
once and says nothing about one that retracts. It is a tidying, not this item.

## 4. A total-order log port ✓ built — and 5, the protocols that implement it

The first object here with a *concurrent* interface: appends and reads that overlap in time, which
is what `G` and `H` were waiting for and what nothing before this could offer. `total_order_log.rs`
is the port, on the model of `link.rs` and `detector.rs`; the suite belongs to it and the
implementations are type arguments, so a property is written once and both members are held to it.

**Three implementations, and the differences are the point.** `consensus_based_total_order_broadcast`
is Algorithm 6.1 in the crash-stop model and `logged_uniform_total_order_broadcast` is the
fail-recovery one; between those two, exactly one thing differs — whether the sequence survives a
restart — and it is asserted where the second one is. `multi_paxos_replica` is the third, and it is
a different algorithm: one consensus per *leader* rather than one per round, so six appends become
six commanders outstanding together where the pair run one instance at a time. Every shared property
runs against all three, and the suite's header says what makes each of them mean something against
the third rather than assuming the pair's schedule carried over.

**Both are transcriptions, and say so.** The book's construction runs one consensus instance per
round in lock-step, so every entry pays a full consensus, and `unordered`, `delivered` and the
family of instances all grow without bound. That is the page. Bounding any of them weakens a
guarantee to a scope and belongs to its own change.

One departure, in the port: **the book has no read.** Its abstraction is `Broadcast` and `Deliver`,
and clients observe deliveries. Both algorithms nonetheless maintain `delivered`, so `read(from)`
exposes what the page keeps and does not offer, served locally — the claim is a total order, not
that a read sees the latest append.

Building it cost three changes to `recon-core`, each found by the compiler rather than foreseen:

- **Composition's mapper widened from `fn` to `impl Fn`.** A pointer captures nothing, so a parent
  could not stamp a child's messages with its own state. Every layer before this was fine because
  the stamp lives in the *child* — `epoch_consensus` writes `Tagged { ets, .. }` because the epoch is
  its own identity. The round belongs to the parent, and the consensus has never heard of rounds.
- **`SeqSlot`**, the sequence half of `Slot`. `store.rs` had described exactly this and declined to
  build it: "nothing needs it… building it now would be the framework before its second consumer."
  The second consumer is the fail-recovery member, which keeps a durable record of its own *and*
  composes the one protocol here that appends.
- **`KeyedSlot`**, for a *family* of durable children — one consensus instance per round, each
  keeping its own record. The key is passed as data rather than captured, so a slot is still one
  fixed function, which is what `Slot`'s own note about not capturing was protecting.

**Item 5 had no page, and that was the open question. It is answered, and the first of three changes
is built.** Multi-Paxos is not in Cachin at all, so the source is van Renesse & Altinbuken's *Paxos
Made Moderately Complex* — the 2015 ACM Computing Surveys edition, not the 2011 Cornell report of the
same title. It was chosen over Kirsch & Amir's *Paxos for System Builders* for the reason
[`docs/bounded-space.md`](bounded-space.md) makes central: Kirsch & Amir specify more of what a
deployment needs, but the protocol they specify keeps a Global History that is never truncated, so
transcribing it faithfully would reproduce the defect this work exists to remove. The survey puts the
bounding **on the page** — §4.1 has an acceptor keep only the most recently accepted pvalue per slot,
and §4.2 collects state below a watermark — so the module can become an implementation while staying
a faithful transcription of its source. §4.1 is applied; §4.2 is the change that remains.

[`multi_paxos_synod.rs`](../crates/recon-protocols/src/multi_paxos_synod.rs) is §2, the Synod protocol:
ballots, acceptors, scouts, commanders and leaders, with safety holding unconditionally and progress
claimed only where Ω settles. [`multi_paxos_replica.rs`](../crates/recon-protocols/src/multi_paxos_replica.rs)
is §2.1 and Figure 1 on top of it, and **there is a log**: `slot_in`, `slot_out`, `requests`,
`proposals` and `decisions`, satisfying `TotalOrderLog` so the shared suite runs against it.

The replica composes the Synod core and nothing else — no link, no broadcast, no wire of its own —
which §4.4's colocation is what buys: a replica hands its proposal to the leader on its own machine,
and a leader that cannot act on it forwards it once to the one Ω trusts. The cost is stated and
tested rather than assumed, and it is that proposal *delivery* now rests on the detector.

**Both of the source's reductions are applied**, which is what makes these implementations rather
than transcriptions. §4.1 is the first and costs no guarantee: an acceptor keeps one pvalue per slot rather than one per `⟨ballot, slot⟩`, and a `p1b`
carries those rather than everything it has ever accepted, so phase one no longer grows with the
ballots a run has seen. The paper raises a doubt against its own reduction — after it there may be
no majority still holding a proposal that was nevertheless chosen — and the module quotes both the
doubt and its answer, because what the reduction discards is the *evidence* of a choice and not the
choice. Applying it also found that the acceptor's reduction and the leader's maximum are different
operations and that only the second needed code, and that the second was tested by nothing until a
mutation said so.

§4.2 is the second, and it is what bounds the state. Replicas report how far they have applied;
leaders and acceptors discard everything below the slot `f + 1` of them have passed. Two things
came out of building it that the section only implies. The skip that keeps a leader off a collected
slot is needed in **two** places, and the one that is not on the page — a proposal arriving at an
already-active leader — split a slot before it existed. And collecting at `f + 1` strands the other
`f`: everything that could help a replica which missed a decision is precisely what gets collected,
so §4.2's "replicas can learn decisions […] from one another" is not an aside but a part of the same
change. `docs/bounded-space.md` carries both rows, now ✅, with the conditions each bound rests on.

## A. Non-transitive partitions — **built**

Connectivity is now between pairs, so `sever(A, C)` on three processes leaves a bridge: `A` reaches
`B`, `B` reaches `C`, `A` does not reach `C`, all three correct. `partition` is the special case
where the severed pairs span two groups.

What it found is in [`docs/conditional-guarantees.md`](conditional-guarantees.md): the first
fault under which this project's chain of conditions lapses link by link — `◇P2`, then `ELD1`, then
`UC4` for the processes with no majority they can reach — while `UC2`, which is `[always]`, holds
through a topology whose two majorities intersect in a single process. The stack routes around the
bridge rather than stalling, which is not what was predicted; the change recorded the question and
let the run answer it.

## B. Per-node clocks, and skew

`Sim` holds one global `now`, so there is no way to express two processes disagreeing about the time.
The failure detector is *entirely* about time, and the accrual detector above derives its threshold
from measured intervals — so this is the fault that stack is most exposed to and least tested
against. It is a real change to the simulator rather than a knob.

## C. Invocations in the trace ✓ built

The trace held completions without the instants that began them. `Sim::command` scheduled an
operation and recorded nothing, so a run could say what a process *concluded* and never what it was
*asked*:

```
Jepsen history               this trace, before
──────────────               ──────────────────
{:invoke :read  …}           (nothing)
{:ok     :read 3}            Indicated { at, node, ind }
     ▲         ▲
     └─────────┴─ an interval        an instant
```

`Sim::command` and `command_at` now mint an `OpId` and return it, and `TraceEvent::Invoked` records
the operation **when the handler ran** — not when the command was scheduled. A handler's effects
cannot precede the handler, so that is a valid left-hand end of the interval containing the effect
and a tighter one than the moment the caller asked; suites here routinely schedule a batch at time
zero, and recording *that* would show every operation overlapping every other.

A test can now ask when an operation began, how long it took, and whether two of them overlapped.
None of that was possible before.

**What it does not do is pair a completion to its invocation**, and that is deliberate rather than
deferred work. The pairing does not exist in the algorithms here: every correct process raises
`Ind::Decide`, including processes that proposed nothing, and the value need not be the proposer's; a
broadcast's `Deliver` is an event arriving at a process rather than a reply to anything it asked.
Marking indications with the operation they complete would be fiction for twenty-five of twenty-six
modules, and fiction a checker would trust. It would also oblige every protocol to hold a
driver-assigned identity across a crash, which is the defect the 2026-08 audit found three times.
Pairing belongs to `4`, the replicated-log port, where an operation has a caller waiting for a
result. Tests pair by hand meanwhile, knowing their own protocol — which is honest, because they do
know and the simulator does not.

**An operation can also no longer vanish.** A command to a process that is not running was discarded
in silence; it is now recorded, with why — crashed, stalled, or not a process in this run. Asked-for
and never-begun is a different fact from never-asked-for, and a record that cannot tell them apart is
one a checker reasons from falsely.

The stalled case is the interesting one, and the answer is not the obvious one. The simulator holds
timers, deliveries and scope events for a suspended process, so holding commands looks like the
missing case — but that rule is about network traffic inside a live session, where a message
discarded with no `SessionEnded` to announce it is loss nobody is told of. A command is not that. A
`Deliver` crosses into a stalled process from outside and waits in a receive buffer, which is a real
thing; a `Cmd` comes from the layer *above*, on that same process, which is stalled with it. There is
nothing between an application and its protocol to hold anything. So the discard was right and the
silence was wrong, and only the silence is fixed — which also leaves *certainly did not happen*
available as a fact, distinct from the *may or may not have happened* that `D` is about.

## D. Indeterminate outcomes

Jepsen's third result is `:info` — *this may or may not have happened* — and it is the one that
matters most, because it is what a client experiences when its connection drops mid-write. A
`Propose` whose process crashes before any `Decide` is exactly that, and this repository currently
models it as *nothing happened*, which is a claim the code is not entitled to make: the value may
well be sitting in a quorum's `pending`. A checker fed that history would be reasoning from a false
premise. Needs an operation identity spanning invocation and outcome, so it follows `C`.

## E. Shrinking ✓ built

The thing this project can do that Jepsen cannot. Jepsen hands you a ten-thousand-operation history
and a failure and you read it; it cannot reliably reproduce, so it cannot minimise. Here a run is a
function of its inputs, so a candidate reduction can be *run* and the question "does it still fail?"
answered rather than estimated.

`recon_sim::Scenario` is a run as data — a configuration with its seed, a membership, timed `Step`s
and a horizon — and `shrink` reduces a failing one against a predicate: the horizon by binary search,
the steps by delta-debugging, a partition by merging its groups, the membership last, to a fixed
point. What comes back is rendered as Rust to paste. A reduced scenario is a **different run that
also fails**, not a prefix of the original: removing a step changes what every later draw takes from
the generator, which is why every candidate is re-run rather than reasoned about.

**This section used to claim more than it could deliver, and correcting it was part of the change
that built it.** It said the three diagnoses in these notes — the epoch that climbed to 647,309, the
send rate that grew 12.6k → 76.6k per window, the leader trusted by everyone that announced nothing
— were ones "a shrinker would have handed over". It would have handed over none of them. The first
needed to *see* `ets`, and was already a one-crash run with nothing to minimise. The second was a
measurement, not a failure. The third needed bisection across the **stack** — Paxos, then
epoch-change, then Ω, then ◇P — not across a schedule. All three are `F`.

What it does buy was measured rather than assumed.
[`shrinking_a_real_defect.rs`](../crates/recon-protocols/tests/shrinking_a_real_defect.rs) puts the
send-rate defect back behind a test-only switch and reduces a scenario exhibiting it from nine faults
across five processes to **one `Propose` and one process**, in fifty candidate runs and half a second.
The one-process result was new: the defect needs no peers at all. It said nothing about *why*, and it
cost two wrong predicates to get there — one returned a 17 ms scenario the *sound* stack satisfied
too, because a run that short is all startup, and the next an 80 ms one that failed the same way,
because an epoch consensus is supposed to send more as it works through `READ`, `WRITE` and
`DECIDED`. The shrinker did not mislead; it exposed a predicate naming a symptom rather than a
property, which is a service — but the cost of fixing the predicate was about the cost of the probe
it was meant to replace.

So a shrinker answers *when* and *with how little*. `F` is what answers *why*, and on this evidence
it is the one to build next.

## F. Logging and tracing ✓ built

What a protocol *says* it is doing, as against what the trace records happening *to* it. The trace
cannot hold a decision that produced no effect, and that is the shape of the diagnoses that cost the
most here: a leader trusted by everyone that announced nothing leaves **nothing at all** behind, and
finding it meant bisecting the whole stack by hand.

`Protocol::Note` names the vocabulary a protocol narrates in and `cx.note(..)` records a decision;
`Sim::record_notes` puts what was said into the trace beside what happened, and
`Sim::enable_tracing` renders every recorded event to a `tracing` subscriber as it is recorded.
Both are off by default, and narrating cannot change the run — a test asserts that a seed observed
and the same seed unobserved agree on every event but the narration.

Three things decided the shape, and the first is the one that matters:

- **Narration has to be checkable, not merely present.** A `tracing::info!` beside a state change is
  a second statement about it, and second statements go stale — this repository has already shipped a
  docstring quoting a resend clause its own comment said a test had replaced. So a note goes into the
  trace, where a test can require it to agree with the run rather than trust it.
- **A note earns its place only where the trace cannot say the same thing.** One beside
  `cx.indicate(..)` restating it adds nothing and can drift. What qualifies is a decision that
  produced no effect, or the *reason* for an effect the trace cannot attribute — `epoch_change` puts
  the same `NACK` on the wire whether it is refusing an announcement or volunteering how far it has
  reached, and a reader cannot tell those apart.
- **A vocabulary belongs to the run, not to a layer**, exactly as a `TimerId` does, so a note passes
  through composition untouched. The alternative — each layer naming its own and parents converting —
  was built and discarded: it put a `where` clause relating two type parameters on every layer
  generic over a port, to express a conversion that was the identity everywhere it was instantiated.

Constraint 2 is not bent. `tracing`'s dispatcher is thread-local, so protocols never touch one; they
call `cx.note`, and only the simulator — a driver, which is allowed to — renders. Narration is
output-only besides: nothing a protocol can observe reveals whether anything is listening, which is
why a run is reproducible whether or not anybody watched.

`epoch_change` is narrated, as the module whose silences caused a real diagnosis; its suite checks
its own narration against the run at three stated strengths — an action taken agreeing with its
effect, an action refused agreeing with the absence of one, and every announcement that arrived
accounted for as either entered or refused. The last is the one that catches narration quietly
falling out of a clause. The other twenty-five modules narrate nothing yet, deliberately: a
vocabulary is better designed against decision points somebody is trying to read.

## G. A concurrent workload

A generator that issues overlapping operations against many processes at once, rather than the fixed
scripts every suite writes by hand today. Needs `C`, because an operation without an invocation has
no interval to overlap with.

## H. A checker, written once against the port

Last, and the ordering is the point: **a checker over a history that is trivially linearizable
proves nothing.** Single-shot consensus decides one value once, and its agreement, validity and
termination are three lines of direct assertion — better than a general checker, not worse. The
question becomes hard only when operations overlap and read each other's writes, which is `4`.

Two models, and which one a protocol claims is part of what it is. A replicated log claims a **total
order**: every process sees the same sequence of entries, which is checkable directly from the
histories without searching. A register above it claims **linearizability**, which is the harder
question and needs the interval from `C` — an operation may take effect anywhere inside it. Written
against the port, both checks apply to every implementation behind it.

One caution particular to this project. A checker is exactly the kind of test
[`tests/method.rs`](../crates/recon-protocols/tests/method.rs) exists to reject: it passes trivially on
a history with no concurrency, and Jepsen has no answer to that. Adopting one means extending the
non-vacuity discipline to it — assert the history *contained* overlapping operations, and assert the
checker can fail, by feeding it a mutated history and confirming it is rejected. A checker that has
never rejected anything is a checker nobody has tested.

## Further out

Running the real thing, against a real cluster, over a real network — which is constraint 5's
territory and not before it. The value of doing `A` to `H` first is that by then a failure Jepsen
finds can be *reproduced* in the simulator from a seed, which is the half Jepsen itself cannot do.
