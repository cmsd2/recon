## Why

The repository has a replicated log and cannot deploy it. `consensus_based_total_order_broadcast`
and `logged_uniform_total_order_broadcast` are correct, held to one suite through the
`TotalOrderLog` port, and both declare themselves transcriptions: one consensus instance per round
in lock-step, so **every entry pays a full consensus**, and `unordered`, `delivered` and the family
of instances all grow with the number of entries handled. `docs/bounded-space.md` lists them as the
two worst rows in the audit.

Multi-Paxos is what fixes that, by keeping the first phase for a stable leader instead of paying it
per entry. README's roadmap item 5 has been blocked on a question rather than on work: Multi-Paxos
is **not in Cachin at all**, and this repository's method needs a page to quote. This change answers
it. The source is van Renesse and Altinbuken (2015), *Paxos Made Moderately Complex* — the ACM
Computing Surveys edition, not the 2011 Cornell technical report of the same title. The two differ
enough to matter: figure numbering, a fuller treatment of §4's pragmatics, and reconfiguration
throughout rather than in one sentence. README already named this edition; an earlier draft of this
proposal named the report, and the discrepancy resolves here in the survey's favour.

**That paper rather than the alternative, and the reason is `docs/bounded-space.md`.** Kirsch &
Amir's *Paxos for System Builders* specifies more of what a deployment needs — leader election, view
change and recovery are complete in its Figures 6, 7 and 14, where this paper leaves leader election
as an adaptive timeout. But the protocol it specifies keeps a Global History indexed by sequence
number that is never truncated, and garbage collection appears in it only as something its
unspecified `Paxos-complete` variant has. Transcribing it faithfully would reproduce exactly the
defect this work exists to remove. *Paxos Made Moderately Complex* puts the bounding **on the page**:
§4.1 has acceptors keep only the most recently accepted pvalue per slot, and §4.2 collects the state
below a watermark once at least `f + 1` replicas have learned a slot's decision, with the collected
slot number carried in `p1b` so that a later leader does not read absence as "nothing was ever
accepted". So the eventual module can be an *implementation* under `docs/bounded-space.md` while
remaining a faithful transcription of its source, which is the thing no other candidate offers.

**A second source, for cross-checking rather than transcribing.** Liu, Chand and Stoller (2019),
*Moderately Complex Paxos Made Simple*, specifies the same algorithm in DistAlgo and proves it in
TLA+. It reports four liveness violations in the vRA specification **when messages can be lost**,
which is this repository's setting rather than a hypothetical one, and three of them are in the
leader and land inside this change. They are listed in `design.md` and applied; the module cites
them where it departs.

**This change is the first of three.** §2 of the paper is the Synod protocol — acceptors, scouts,
commanders and leaders — and its safety holds unconditionally: at most one proposal is ever chosen
for a slot, whatever the schedule. Its liveness does not, and the paper is blunt about it: §3 opens
by observing that duelling leaders can preempt each other forever, and that the Synod protocol as
described guarantees nothing about progress "even in the absence of any failure whatsoever". So progress
here is claimed **conditionally**, on Ω settling — which is the shape `docs/conditional-guarantees.md`
requires anyway, and the chain below Ω is already stated at each link. Slots, replicas and the log
port come next; bounding and entry to the real-world set after that.

## What Changes

- **`multi_paxos_synod.rs`** — §2 of the paper, transcribed, with its Figures 4, 6 and 7 quoted
  above the implementation as every module here quotes its algorithm. Four roles, co-located on one
  process as §4.4 describes:
  - an **acceptor**, holding `ballot_num` and `accepted`, answering `p1a` and `p2a`;
  - a **scout**, one per ballot, running phase one and reporting `adopted` or `preempted`;
  - a **commander**, one per (ballot, slot), running phase two and reporting a decision or
    `preempted`;
  - a **leader**, holding `ballot_num`, `active` and `proposals`, spawning the other two.
- **Scouts and commanders are the leader's own bookkeeping, not child protocols.** The paper spawns
  them as threads because it is written in a language where a thread is the cheapest way to say "wait
  for a majority"; neither has a vocabulary of its own, and an acceptor answers the **leader** rather
  than the thread. `design.md` states the mapping from each figure's `for ever / switch receive` arm
  to where it lives here, since a quoted contract this module diverges from structurally has to say
  where it went.
- **Over `session_link`**, not stubborn links. The paper assumes a message between non-faulty
  processes is eventually received at least once and not necessarily in order, which is what a
  session provides within itself; the scope-ended event is what tells the layer above that a suffix
  may have been lost. This is the first module to satisfy the real-world set's first obligation
  before it is a member rather than after.
- **The main departure, stated: liveness comes from Ω, not from the paper's pinging.** §3 has a preempted
  leader monitor the preempting one by pinging it on a regular basis, backing off with an AIMD
  timeout, and says outright that "this concept is called failure detection". This repository
  already has an eventual leader detector, tested against a detector that lies. The leader composes
  `EventualLeaderDetector` and acts on `Trust`: it starts a scout for its next ballot when trusted
  and stays passive otherwise. The module states the difference — Ω names one leader where the
  paper's scheme lets any correct leader win a race — and what it costs.
- **A second departure, in the acceptor: `p2a` accepts under `b ≥ ballot_num`, adopting the ballot
  it accepts.** The survey's Figure 4 requires equality, and its commander treats any other reply
  as a preemption; over a link that can lose a `p1a` at a session ending, that pairing lets an
  acceptor that missed phase 1 kill a commander with a reply naming a lower ballot, and the slot
  stalls until a timeout. The condition used is the 2011 report's, and is the fix Liu et al. give
  for their useless-replies finding; `design.md` carries the scenario and the alternatives.
- **A suite for the Synod protocol's own guarantees**, before there is a log to observe them
  through: at most one proposal is chosen per slot, a chosen proposal is one that was proposed, and
  a majority that has adopted a ballot cannot later accept a lower one. Plus the non-vacuity halves
  the method requires — the run really contained competing ballots, and really preempted somebody.

Deliberately not in scope, each for its own later change:

- **Slots, replicas and the log.** The replica of Figure 1, `slot in`, `slot out`, `decisions`, and
  satisfying
  `TotalOrderLog` so the shared suite runs against it. Change 2.
- **Bounding.** §4.1's state reduction and §4.2's garbage collection. Less is left open than an
  earlier draft of this proposal claimed, which had read the 2011 report: the survey collects once
  `f + 1` replicas have learned a decision rather than waiting for all of them, says what an acceptor
  must carry in `p1b` so the collection is not mistaken for absence, and answers the case where too
  few replicas remain to report — run `2f + 1` of them, or put the replica set in the configuration
  and replace the suspicious ones. Change 3, and what admits the module to the real-world set.
- **Leases and read-only commands.** §4.5 needs a known bound on clock drift. Per-node clocks are
  roadmap item `B` and do not exist.
- **Changing the membership**, which this source *does* specify and this change still does not build.
  The survey is the "full reconfigurable" Paxos: a client proposes a special reconfiguration command,
  it is decided in a slot like any other, and it takes effect at slot `s + WINDOW`, so up to `WINDOW`
  proposals may be pending against a configuration that is still certain. That is Lamport's `α` from
  *Paxos Made Simple* made concrete. It belongs with slots, because a configuration takes effect at a
  slot and there are no slots until change 2 — so it is change 2's or change 3's, with a source
  already chosen, rather than an open question. Kirsch and Amir assume static membership outright,
  which is one more reason they are not the source here.
- **Durability, and with it any process that returns having forgotten.** The paper's §4.3, *Keeping
  State on Disk* — a section the 2011 report did not have at all — puts acceptor and leader state on
  stable storage, and its exercise 7 says to implement it and to handle a crash mid-save. Until that exists this module is crash-stop, and
  that is the source's own model rather than a scope dodge: a crash there is permanent — a crashed
  state machine "will make no more transitions" — and a process that comes back off disk "is not
  theoretically considered crashed, it is simply slow for a while". There is no third case, and a
  process returning with nothing is the first one acting when it should not. The module states the
  boundary, because this simulator can produce that case and Ω will trust such a process again: the
  round counter restarts, a ballot is re-minted, and an acceptor still holding it can accept a second
  proposal under it. A durable ballot counter is what makes leading again after a restart legal, and
  it belongs to the fail-recovery change with the rest of §4.3.

## Capabilities

### New Capabilities

- `consensus/multi-paxos-synod`: the Synod protocol of *Paxos Made Moderately Complex* §2 — what
  ballots, acceptors, scouts and commanders guarantee about which proposal can be chosen for a slot,
  and what they deliberately do not guarantee about progress

### Modified Capabilities

None. The `TotalOrderLog` port is untouched: this change builds nothing that satisfies it, which is
change 2's job.

## Impact

- **New module** `crates/recon-protocols/src/multi_paxos_synod.rs`, and a suite beside it.
- **No change to `recon-core` expected.** Nothing here composes a run-time family of children, so
  `KeyedSlot` and `SeqSlot` are not needed; the module uses `Child` for the detector alone. If
  something turns out to be missing, that is a finding for the apply phase rather than an assumption
  here.
- **Composes** `session_link` and `eventual_leader_detector`, both existing.
- **`README.md`** — the protocol table, the suite table and counts, the specification tree, and
  roadmap item 5, whose open question this closes.
- **`docs/bounded-space.md`** — a row for the new module. It is a transcription and unbounded, and
  says so; change 3 is what alters that.
- **`CLAUDE.md`** — the reference material section, which currently names only Cachin. This is the
  first module here whose source is a paper, so the convention that a module quotes its page needs to
  say which paper and which edition, in full. Both editions of *Paxos Made Moderately Complex* exist
  and their figures are numbered differently, so naming the title alone is not enough to quote
  against. `design.md` carries the reference list.
