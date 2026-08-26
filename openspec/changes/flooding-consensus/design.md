## Context

See `proposal.md` — Why. The relevant current state is that both children already exist and are
already used together: `uniform_reliable_broadcast` drives `best_effort_broadcast` and
`perfect_failure_detector` side by side, so the composition shape is settled and this change
follows it rather than inventing one.

Two facts about the existing detector shape this design. It emits a single irrevocable `Crash`
indication — there is no `Restore`, because Module 2.6 has no need of one. And it is perfect only
because the simulator's `synchronous(BOUND)` mode makes its timing assumption true;
`accuracy_is_lost_when_the_timing_assumption_is_withdrawn` already demonstrates it accusing the
living otherwise. Both of those are load-bearing here: the first is why a split can never be
repaired, the second is how the split is provoked.

## Goals / Non-Goals

**Goals:**

- Algorithm 5.1 transcribed closely enough to read against the page, pseudocode quoted above it.
- The dependence on strong accuracy demonstrated by a schedule, not asserted in a comment.
- The `Propose`/`Decide` vocabulary settled for the consensus rungs that follow.

**Non-Goals:**

- Any repair of a split decision. There is none to have at this rung, and adding one would make
  this a different algorithm.
- Sequence consensus, or reusing one instance for a second decision.
- Anything over session links. Fail-stop consensus over a link that can end is not a rung the book
  has, and inventing one before the leader-driven family exists would be building the framework
  first.

## Decisions

**Over `best_effort_broadcast` and `perfect_failure_detector`, not the session stack.** The
session rungs exist because they are what a deployment would run; this one is not, and putting it
there would imply otherwise. The pairing also matches `uniform_reliable_broadcast`, which is the
module this one will be read against.

*Alternative considered:* a session variant alongside, as the broadcast rungs have. Rejected — the
contrast that made session broadcast worth writing was that the two rungs *diverge* over a link
that ends. Flooding consensus does not diverge; it simply stops working, because a lost round
message stalls a round for ever and a false suspicion splits it. That is a result about the model,
not a rung.

**`type Scope = Infallible`.** No session, so no scope end can be constructed — the same choice
both children make.

**The proposal type is `Ord`.** The book: "the processes can apply any deterministic function to
their accumulated proposal set, provided this function is agreed upon in advance and is the same
at all processes. In our case, the process decides the minimum value ... we implicitly assume here
that the set of all possible proposals is totally ordered and the order is known by all
processes." `P: Ord + Clone` renders that assumption as a bound, which is where it belongs — a
type that cannot be totally ordered cannot be proposed. `BTreeSet<P>` then gives the accumulated
set deterministic iteration for free, which the ordered-maps guard requires anyway.

**Round-indexed state is a `BTreeMap<u64, _>`, not an array.** The book writes `receivedfrom :=
[∅]^N`, an array of size N because a run enters at most N rounds. A map keyed by round is the
natural Rust rendering, holds only the rounds actually entered, and stays bounded for the same
reason the array is sized N: a round that does not decide requires a newly detected crash, and
detections are permanent, so there can be at most N of them. `receivedfrom[0]` is seeded to the
full membership at construction, which is what makes a first-round decision require having heard
from everyone.

**The decision rule is a standing condition, re-evaluated after every event that could satisfy
it.** `upon correct ⊆ receivedfrom[round] ∧ decision = ⊥` is not triggered by a message; it is a
guard the book re-checks continuously. In particular **a `Crash` indication alone can satisfy it**,
by shrinking `correct` to a set already heard from. Rendering it as a private `check_round` called
from both the broadcast path and the detector path — exactly as `uniform_reliable_broadcast` calls
`check_deliverable` from both — is what keeps that true. Checking only on message delivery would
lose termination in precisely the case the algorithm exists to handle.

**One wire type multiplexing two children, mirroring the uniform broadcast.** `Wire<P>` has a
`Broadcast` arm carrying this layer's own message and a `Detector` arm carrying the heartbeat. The
broadcast payload is itself two-armed — a proposal tagged with its round, and a decision — because
Algorithm 5.1 sends exactly those two. No other field is added: this layer adds per-round state,
so it adds a round number, and nothing else.

**The next round re-broadcasts the *previous* round's proposal set.** The pseudocode reads
`trigger ⟨ beb, Broadcast | [PROPOSAL, round, proposals[round − 1]] ⟩` after `round := round + 1`.
It is an easy detail to render wrong as `proposals[round]`, and the difference only shows up in a
run with a crash cascade. Quoted in the module for that reason.

**The split is provoked by a partition inside synchronous mode.** This is the mechanism
`uniform_agreement_breaks_when_the_timing_assumption_is_withdrawn` already uses: delivery within
each side stays bounded and well-behaved, while across the partition it is not, so each side
accuses the other of crashing. Both sides then complete their own rounds and decide the minimum of
their own halves' proposals.

The partition is a device for producing a false suspicion, not a claim that fail-stop consensus
ought to survive a partition — the fail-stop model has a reliable network and no partitions, and
running outside the model is the whole point of the exercise, as it is for the two sibling tests
this one is shaped after.

**What the split is *not*.** It is not the correct set decaying towards empty. Every process is
correct throughout: none crashes, and the run is one in which all four are reachable again before
it ends. What each side holds is a non-empty *proper subset* of the membership, wrongly, and the
two subsets are disjoint. That is what makes it an agreement violation between correct processes
rather than an artefact of crashes, and it is the shape a false suspicion actually takes.

*Alternative considered:* the lossy asynchronous configuration the detector's own accuracy test
uses. Rejected as the primary vehicle — it breaks accuracy, but it also breaks delivery, so a run
that fails to agree might have failed to agree for want of a message. The partition isolates the
variable.

**Why the split is permanent, stated correctly.** It is tempting to say the split persists because
this detector's accusations are irrevocable — Module 2.6 has `Crash` and no `Restore`. That is
true of the detector but it is the wrong reason, and it makes the result look like an artefact of
using P outside its model.

The model these algorithms are written against is not one in which the correct set decays. It is
one of **eventual stability**: the system stabilises, timing bounds come to hold, and after that
point the correct set is agreed and stays agreed. `docs/scope-annotated-modules.md` names this
Assumption F and observes it is the partial-synchrony global stabilisation time and the ◇ of an
eventually-accurate detector in the same clothes. An eventually perfect detector would therefore
*withdraw* both false suspicions after the heal, and every process would again be held correct by
every other.

**The split would still be there.** A consensus decision is irrevocable — that is what deciding
means — and both sides decided during the unstable interval. Stabilisation arrives too late to
help. That is the real lesson, and it is what separates this rung from the leader-driven family:
flooding consensus commits during instability and therefore cannot be rescued by Assumption F,
whereas a quorum-based algorithm declines to commit until it has evidence that no conflicting
decision is possible, and so has something left for stabilisation to rescue. Stated this way the
result survives replacing P with ◇P, which the "irrevocable accusation" story does not.

## Risks / Trade-offs

- **The splitting test passes for the wrong reason** — one side never decides, making it a
  termination failure dressed up as an agreement failure. → Assert positively that *both* sides
  decided, that the decisions differ, and that no process crashed in the run. A liveness failure
  cannot satisfy all three.
- **The standing condition is rendered as a message handler**, so a crash that should complete a
  round does not. → A test where the only event that can complete the round is the crash
  indication itself, with no message in flight.
- **The next-round broadcast sends the wrong proposal set.** Invisible without a crash cascade. →
  A test that crashes processes in consecutive rounds and asserts the decision still reflects
  proposals made by processes that crashed after broadcasting.
- **Agreement assertions pass vacuously** if nothing ever decides. → Minimum decision counts
  asserted alongside every absence-of-violation property, per the standing practice in
  `tests/method.rs`.
- **Round state grows** if the map is keyed carelessly or entries are added for rounds never
  entered. → A test that runs many messages through repeated rounds and asserts the state stays
  bounded by membership and rounds entered.
- **This rung invites being mistaken for a usable consensus.** → Its status is stated in the module
  documentation and in the README table, and the specification names the limit as a requirement
  rather than burying it in a comment.
