## Context

See `proposal.md` — Why, for the motivation and for the colocation decision that shapes everything
below.

What is already here is the whole of the difficulty. `MultiPaxosSynod` agrees on a command for a
slot the caller names, over a session link, with Ω driving leadership. `TotalOrderLog` is a port with
one suite behind it and two implementations held to it. This change is the join, and the join is
narrow: the replica is bookkeeping over a child's indications, in the shape `Child<P>` was extracted
for.

Three constraints from `CLAUDE.md` bear directly:

- **Composition picks one of two forms**, decided by one question: does the layer transform its
  child's indications, or pass them on? This one transforms — a slot decision becomes a position in
  a sequence — so the child is a `recon_core::Child<P>` run with `child.run(cx, wrap, f)`.
- **A protocol constructed at run time is owed exactly one of `on_init` and `on_recovery`.** Not a
  hazard here, because the child is built in the constructor, but the composed detector beneath it
  is the one that has bitten twice, and the rule is why `on_init` must reach it.
- **Identity is as durable as the state it keys.** The replica mints request identifiers that cross
  the wire, so their generator is state with a scope, and the scope has to be stated.

## Goals / Non-Goals

**Goals:**

- One module readable against Figure 1, with the pseudocode quoted above it and R1–R5 stated.
- `TotalOrderLog` satisfied, so the shared suite runs unmodified against a third implementation.
- The Synod modification kept to what §4.4 and Figure 6(a) actually say, with its cost tested rather
  than assumed.
- A shape change 3 can bound without rework: §4.2 collects on `slot_out`, so `slot_out` has to be
  the thing the replica reports.

**Non-Goals:**

- An application state machine. See `proposal.md`; the port is a log.
- Any change to the Synod core beyond the two messages the proposal names. In particular the
  acceptor, the scout, the commander and `pmax` are untouched.
- Bounding, durability, reconfiguration, leases.

## Decisions

### The replica composes the Synod core and nothing else

`MultiPaxosReplica<V, L = SessionLink<..>>` holds one field of consequence:
`synod: Child<MultiPaxosSynod<Command<V>, L>>`. No link, no broadcast, no wire of its own, and
`type Msg = <MultiPaxosSynod<..> as Protocol>::Msg` — this layer adds no header, which puts it with
the three protocols that already add none.

That is only possible because of the colocation modification. Without it the replica would need a
second link and a `BestEffortBroadcast` child to reach remote leaders and remote replicas, giving
two links per process, two sets of sessions, and a wire multiplexing them — the shape
`lazy_probabilistic_broadcast` has and pays two type parameters for.

*Alternative considered:* the faithful separation, with the replica owning that broadcast and the
Synod core untouched. Rejected for the reason §4.4 gives — the roles are on one machine, so a
message between them is a function call — and because it would have the replica broadcast a
`propose` to processes that are, in this deployment, the same processes its child is already
talking to.

### The two new Synod messages, and the loop rule

`SynodMsg` gains two variants, and the leader gains one field.

**`Propose { slot, command }`.** `on_propose` currently ends in one of two states: commanded, if
active; or remembered in `proposals`, if not. A third case is needed — this process will not lead,
so remembering is the same as dropping. The leader therefore keeps `trusted: Option<NodeId>` (who Ω
trusts) rather than `trusted: bool` (whether that is me), and a process that is neither active nor
trusted forwards. Active-but-untrusted keeps the proposal and commands it: its adopted ballot
stands until something preempts it, so commanding is a round trip where forwarding is a round trip
plus a phase one, and "stops competing" means starting no new ballots, not abandoning the one a
majority already adopted. The accessor keeps meaning "trusted *is me*", which is what the existing
suite's assertions read.

**The forward is not forwarded again**, and this is the clause worth naming because a naive
implementation loops. Two processes whose detectors disagree pass a request back and forth for as
long as they disagree. The message carries no hop count and needs none: a `Propose` that *arrived*
is handled locally or dropped, never forwarded, so the path is at most two hops by construction. The
spec states the drop, and the replica's re-proposal timeout is what recovers it.

*Alternative considered:* a hop count, so a request could cross a longer chain of disagreeing
processes. Rejected as a field on the wire buying a case that does not arise: Ω converges, and while
it has not, a second hop is as likely to be wrong as a third.

**`Decision { slot, command }`.** Figure 6(a)'s own last line, restored. The commander broadcasts to
every process and raises `Ind::Decision` locally, so a decision costs one fan-out per decided slot.
This is new work per decision and `docs/bounded-space.md` gets the amendment.

*Alternative considered:* have the deciding process's replica broadcast instead, leaving the Synod
core alone. Same messages, same hop count, but it puts a decision on the wire in the replica's
vocabulary when Figure 6(a) already puts it in the commander's — and it would need the replica to
have a link after all, which is the thing this design is buying.

### Slots are positions, and a position is not a slot

`Position` and `Slot` are both `u64` and are **not** interchangeable, which is the trap. The book's
slots count from 1 and `Position::START` is 0; more importantly, `perform` skips a command already
decided at a lower slot, so a slot can pass without a position being taken. The mapping is therefore
`slot_out` counts slots and `Position` counts entries actually ordered, and the two diverge exactly
by the number of duplicate commands.

The module states this, because the seductive shortcut — `Position(slot)` — is right until the first
duplicate command and then silently wrong, and the suite would not catch it unless a test drives one
command into two slots deliberately. That test is in the plan.

### `Command<V>` is `⟨κ, cid, op⟩`, and `cid`'s scope is the incarnation

The port's `Ordered { position, from, value }` needs `from`, and the duplicate check needs a way to
tell two appends of the same value apart. Both come from the source's own command shape.

`cid` is a counter, volatile, and the module says so: **its scope is this incarnation**, exactly as
the ballot's round is. A restarted process re-mints `cid` values it has already used, and a request
carrying a reused `⟨κ, cid⟩` can be mistaken for one already ordered and dropped. That is the same
class as the durable-identity rule's worked example, and it is one more thing the fail-recovery
change buys.

### The re-proposal timeout, and what it does *not* try to distinguish

Liu et al.'s fourth violation. One periodic timer, compared against its registered `TimerId` before
acting, as the convention requires. On each fire, any proposal outstanding longer than a threshold is
proposed again — for the **same** slot, because the slot is still unfilled and re-proposing there is
what unblocks `slot_out`.

The replica cannot tell why a decision has not arrived: the proposal may have been forwarded to a
crashed process, the decision may have been lost at a session ending, or consensus for that slot may
simply still be running. It does not need to, because every case is answered by the same message and
none is made unsafe by asking. Where the slot is genuinely open, the leader's `∄c'` guard drops the
repeat and the attempt in flight fills it. Where the proposal itself was lost, the re-proposal is
the first the leader hears of it and is proposed normally. Where the slot is **decided**, the leader
answers with the decision — the requirement below — so the timeout can be generous and wrong without
being unsafe, and the one thing it must not be is absent.

### A leader answers a re-proposal for a decided slot, or the timeout above is a loop

The half of Liu et al.'s fix that belongs to the leader, and the reason it cannot be omitted: a
decision is broadcast **once**, by the commander that counted the majority, which then exits. A
process the broadcast never reached has no other way back. Replicas do not gossip decisions to one
another (that arrives with §4.2, whose watermark exchange is change 3), the session link does not
resend across an ending, and the retry sweep walks the `waitfor` sets of *live* commanders — this
one is gone. The only incidental re-broadcast is a preemption forcing re-adoption, which re-commands
every proposal, and that fires least in exactly the run that matters most: a stable leader. Without
the answer, a re-proposal for a decided slot hits the `∄c'` guard, is noted `ProposalIgnored`, and
the asker re-proposes for ever while its `WINDOW` fills.

So the leader keeps `decided: BTreeSet<Slot>`, marked when a commander completes, and a `Propose`
for a member is answered with `Decision { slot, command }` sent to the **asker alone** — one message
where the original announcement was a fan-out. The command comes from `proposals[slot]`, which for a
decided slot is the decided command: it was the commanded value in the deciding ballot, and any
later adoption's `pmax` writes the same command back by Invariant A5. The set grows with slots
decided, which changes nothing about the capability's stated bound — the leader's `proposals` map
already grows identically — and §4.2's watermark collects both in change 3.

*Alternative considered:* restart a commander for the slot instead of answering, avoiding the
`decided` set. Rejected as a full phase-2 fan-out and round trip to re-deliver one message to one
process, and because it makes the common recovery indistinguishable in the trace from a genuine
competing command.

*Alternative considered:* re-propose into a *new* slot rather than the stalled one. Rejected: it
leaves the old slot unfilled for ever, and `slot_out` never passes it, which is the wedge the fix
exists to prevent.

### What the module quotes, and where each of Figure 1's parts went

The mapping is part of the contract, as it was for the scouts and commanders:

| Figure 1 | Here |
|---|---|
| `var state := initial_state` | absent — the port is a log; see `proposal.md` |
| `slot_in`, `slot_out` | fields, counting slots |
| `requests`, `proposals`, `decisions` | fields; `decisions` is append-only as the page has it |
| `function propose()` | `propose`, run after every request and every decision, as the page's `for ever` loop does |
| the `isreconfig` branch in `propose()` | absent — reconfiguration is a later change; the `WINDOW` guard around it is kept |
| `function perform()` | `perform`, minus `op(state)` and the response; the already-decided check is kept and is what stops one command taking two positions |
| `case ⟨request, c⟩` | `Cmd::Append`, the port's own |
| `case ⟨decision, s, c⟩` | the child's `Ind::Decision` |
| `∀λ ∈ leaders : send(λ, ⟨propose, …⟩)` | a call into the child, per §4.4 |
| `send(κ, ⟨response, cid, result⟩)` | absent, with `state` |

## Risks / Trade-offs

- **Proposal delivery now depends on Ω** → the cost of colocation, and the reason it is a spec
  requirement rather than a note. A proposal forwarded to a process the detector wrongly trusts is
  lost, and only the re-proposal timeout recovers it. A run in which the detector is wrong *and* a
  forward is lost is in the plan, asserting that the sequence still completes, and the non-vacuity
  half asserts the forward really was lost rather than the run having been lucky.

- **The forwarding loop is prevented by a convention, not by a type** → a `Propose` that arrived must
  not be forwarded, and nothing in the signature says so. Same shape as the timer-comparison rule,
  and it gets the same treatment: a test that two processes disagreeing about leadership exchange a
  bounded number of messages rather than an unbounded one.

- **`Position` and `Slot` are both `u64`** → a mix-up compiles. Mitigated by a test that drives one
  command into two slots and asserts the sequence has one entry, and by the module stating the
  divergence. A newtype would be stronger; it is not proposed here because `Position` is
  `recon-core`'s and `Slot` is the Synod module's, and changing either for this reason is a wider
  change than this one.

- **The shared suite may pass for the wrong reason** → `tests/total_order_log.rs` was written for
  two implementations that decide a whole batch per round. Multi-Paxos decides slots independently
  and out of order, so a property that happened to hold structurally there may hold here by a
  different route or not at all. Each shared property is checked to still be non-vacuous against
  this implementation rather than assumed, and the suite's own non-vacuity test — that the run
  contained overlapping operations — is the one to watch.

- **Two capabilities move in one change** → the Synod delta and the new capability. They cannot be
  separated: the forward and the decision broadcast exist only to serve a replica, and a replica
  cannot be built without them. The tasks keep them in order so that the Synod suite is green on its
  own before the replica is written.

## Open Questions

- **The re-proposal threshold, relative to `detect_after`.** It must exceed a decision's round trip
  or it re-proposes healthy slots, and it must be well below the point at which `WINDOW` fills.
  Decidable when the stalled-decision test is written, and it changes no spec.

- **How many slots the shared suite should drive against this implementation.** Enough that several
  commanders are in flight at once, which is what makes out-of-order decisions ordinary rather than
  incidental. A tuning question.
