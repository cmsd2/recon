## Why

`docs/bounded-space.md` marks `multi_paxos_synod` and `multi_paxos_replica` ❌ — state growing with
the slots and the commands handled — and names §4.2 as what fixes it. This is that change. It is the
last thing between Multi-Paxos and the real-world set: the session-link obligation is met, the
message half of the resource-use obligation is met as an identity, and the state half is not met at
all.

§4.2 separates two things under one heading, and the separation is the whole shape of this change.

**Unavoidable, and bounded by time rather than by a watermark:**

> because commands may be decided in multiple slots, replicas each maintain a set of all decisions
> to filter out such duplicates. In practice, it is often sufficient if such information is only
> kept for a certain amount of time, making the probability of duplicate execution negligible.

**Unnecessary, and bounded by a watermark:**

> The leader maintains state for each slot in which it has a proposal. In addition, when a leader
> becomes active, it spawns a commander for each such slot. Similarly, even if the state reduction
> of Section 4.1 is implemented, each acceptor maintains state for each slot. However, once at least
> `f + 1` replicas have learned about the decision of some slot, it is no longer necessary for
> leaders and acceptors to maintain this state — replicas can learn decisions, and the application
> state that results from those decisions, from one another.

The mechanism is one sentence and the trap is the next one:

> Thus, much state, and work, can be saved if each replica periodically updates leaders and
> acceptors about its `slot out` variable. Once a leader or acceptor learns that at least `f + 1`
> replicas have received all decisions up to some slot `s`, all information about lower numbered
> slots can be garbage collected. **However, we must prevent other leaders from mistakenly
> concluding that the acceptors have not accepted any pvalues for the garbage-collected slots.** To
> achieve this, the state of an acceptor can be extended with a new variable that contains a slot
> number: all pvalues lower than that slot number have been garbage collected. This slot number must
> be included in `p1b` messages so that leaders can skip the lower numbered slots.

Note what the safety argument turns on. Collecting an acceptor's pvalue does not destroy the
information that a proposal was chosen — it **moves** it, from the acceptors to the `f + 1` replicas
that hold the decision. An acceptor's silence about a slot stops meaning "nothing was accepted" and
starts meaning one of two things, and the watermark in `p1b` is the only thing that tells them
apart. A leader that read a collected slot as empty would propose freely for a slot already decided,
which is a split slot — the same failure `synod-ignore-pmax` models, arrived at from the other
direction.

## What Changes

- **A replica reports how far it has applied.** Periodically, `slot_out`. §4.4 colocates the roles,
  so a replica tells its *own* leader and acceptor by a call; the leaders and acceptors of other
  machines learn by a new Synod message. Keeping the report on the Synod wire is what lets the
  replica go on having no wire of its own.

- **Leaders and acceptors collect below the `f + 1` watermark.** Each tracks what it has been told,
  takes the highest slot at least `f + 1` processes have applied to, and drops everything below it:
  the acceptor's pvalues, the leader's proposals, its record of decided slots. This is the state
  `docs/bounded-space.md` marks, and after this it is bounded by membership and the window between
  the watermark and the frontier rather than by the slots handled.

- **An acceptor carries `collected` and puts it in `p1b`**, and a leader **skips** slots below the
  highest `collected` any acceptor in its majority reports. This is the trap above, and it is the
  one clause in this change that safety depends on.

- **The replica keeps decisions for a retention window**, which is the "unavoidable" half and a
  different mechanism: bounded by time rather than by a watermark, because there is no watermark for
  it — a replica's duplicate filter has to outlive every slot a duplicate could span. This
  **weakens a guarantee to a scope**: "a command decided in two slots takes one position" becomes
  "within the retention window", which is exactly the kind of weakening `docs/bounded-space.md` says
  belongs in a specification rather than in a cleanup.

- **The status of both modules changes**, and this is what the change is for. `multi_paxos_synod`
  becomes an **implementation**: bounded by membership and by the window between the watermark and
  the frontier. `multi_paxos_replica` becomes an implementation too, on a retention window, with
  the ordered sequence itself the one thing that still grows — which is the log, and is the data
  rather than the bookkeeping.

- **The `2 f + 1` caveat is stated and tested.** The paper: "if there are fewer than `2 f + 1`
  replicas, the crash of `f` replicas would leave fewer than `f + 1` replicas to send periodic
  updates and no garbage collection could be done." Collection stalling is the *correct* behaviour
  there, not a bug, and a test has to say so — otherwise the first person to see a run stop
  collecting will treat it as one.

## Capabilities

- `consensus/multi-paxos-synod` — modified
- `consensus/multi-paxos-replica` — modified

## Impact

- `crates/recon-protocols/src/multi_paxos_synod.rs` — a watermark message, the reported map, the
  collection, `collected` on the acceptor and in `p1b`, and the skip in `adopted`.
- `crates/recon-protocols/src/multi_paxos_replica.rs` — the periodic report, and the retention
  window on `decisions` and `performed`.
- Both suites — the collection measured rather than asserted, the skip driven by hand, and the
  stall under too few replicas.
- `scripts/check-safety-tests.sh` — the mutation set should grow. Collecting below a watermark and
  skipping below `collected` are both clauses safety now rests on, and neither has a mutation.
- `docs/bounded-space.md` — both rows, and the audit's own conclusion about which modules are
  bounded.
- `README.md` — the protocol table's status and space columns for both modules, the real-world set
  section, and the roadmap.

**Not** in scope: §4.3, keeping state on disk. It is the next change and it is what makes leading
again after a restart legal. And not reconfiguration, which §4.2's own last paragraph offers as the
alternative to `2 f + 1` replicas — the membership here is fixed for the run and says so.

`synod-message-cost` is complete and unarchived at the time of writing. Its delta adds three
requirements and does not touch the space requirement this one modifies, so the two do not collide;
archive it first regardless, so that this change's delta is written against a synced main spec.
