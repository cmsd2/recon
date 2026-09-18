## Context

See `proposal.md` for what §4.2 says. What follows is how it lands on the two modules that exist,
and which parts are safety rather than housekeeping.

What grows today, measured by the accessors the audit asked for:

| | grows with |
|---|---|
| `MultiPaxosSynod::accepted` | slots this acceptor accepted for (one per slot since §4.1) |
| `MultiPaxosSynod::proposals` | slots this leader was asked about |
| `MultiPaxosSynod::decided` | slots this process learned decided |
| `MultiPaxosReplica::decisions`, `performed` | commands handled |
| `MultiPaxosReplica::sequence` | commands handled — **the log itself** |

The first three are §4.2's watermark. The next two are §4.2's retention window. The last is not
bookkeeping and is not collected: it is what the port exists to serve.

## Goals / Non-Goals

**Goals:**

- Leader and acceptor state bounded by membership and by the distance between the watermark and the
  frontier.
- Replica bookkeeping bounded by a retention window, with the weakened guarantee written down.
- The `p1b` watermark, and a leader that skips below it — the clause safety rests on.
- Both modules' status in `docs/bounded-space.md` and `README.md` changed to match, or the audit
  becomes a document that lags the code.

**Non-Goals:**

- §4.3, state on disk. The next change.
- Reconfiguration, which §4.2 offers as the alternative to `2 f + 1` replicas.
- Collecting the ordered sequence. A log's entries are the deliverable.

## Decisions

### The watermark travels on the Synod wire, not on a wire of the replica's

A replica has no wire: it composes the Synod core as a child and adds no header, which is what §4.4's
colocation bought and what this change must not spend. So the report goes down as a command — the
replica tells its own leader and acceptor by a call — and the Synod core gossips it, because leaders
and acceptors are what need it.

`SynodMsg::Applied { slot_out }`, sent periodically to every other process. Each process keeps
`reported: BTreeMap<NodeId, Slot>` — one entry per member, bounded by membership — and the watermark
is the highest `s` such that at least `f + 1` entries are `≥ s`. With the roles co-located the
replica set and the acceptor set are one, so `f + 1` of `2 f + 1` is a majority and
[`is_majority`] already computes it.

*Alternative considered:* piggybacking `slot_out` on `p2b`, which every acceptor already sends. It
costs no message, and it is wrong twice: a process that is not currently answering anything stops
reporting, and the acceptor's reply would carry the *replica's* variable, mixing two roles' state in
one message for a saving the periodic sweep already has a timer for.

### The acceptor's `collected`, and the skip that safety rests on

An acceptor gains `collected: Slot` — every pvalue below it is gone — and `p1b` carries it. This is
the one part of the change that is not housekeeping.

An acceptor's silence about a slot used to mean one thing: nothing was accepted for it. After
collection it means one of two, and the watermark is the only thing that distinguishes them. So
`adopted` must **skip** every slot below the highest `collected` any acceptor in the majority
reported, rather than treating it as free. A leader that proposed for a collected slot would propose
for a slot already decided, and could get a second command chosen for it — a split slot, reached
from the opposite direction to `synod-ignore-pmax`.

The highest rather than the lowest, and it is worth being explicit: `collected` values differ across
acceptors, and a slot collected at *any* acceptor in the majority is one whose decision `f + 1`
replicas hold. Taking the lowest would be safe and would collect less; taking the highest is what
the paper's "skip the lower numbered slots" means and is what makes the collection worth doing.

### The replica's retention window, and the guarantee it weakens

`decisions` and `performed` are the duplicate filter, and §4.2 is explicit that this one is bounded
by time rather than by a watermark: "it is often sufficient if such information is only kept for a
certain amount of time, making the probability of duplicate execution negligible".

So the requirement *A command decided in two slots occupies one position* becomes true **within the
retention window** and not beyond it. That is a real weakening and it goes in the specification. It
is also, as things stand, a weakening of something unreachable: a command is minted at one replica
and occupies one slot at a time, so a duplicate needs the client-retry deployment this port does not
have — which the replica module already states. The window is therefore the honest bound on a filter
whose case does not arise here, rather than a new hazard.

*Alternative considered:* bounding `decisions` on the same watermark as the leader's. Rejected, and
this is the trap the paper is warning about in its first paragraph: the watermark says `f + 1`
replicas have *applied* up to `s`, which says nothing about whether a command decided below `s` may
still be decided again *above* it. The duplicate filter has to outlive every slot a duplicate could
span, and no watermark bounds that. Time does.

### What stays unbounded, and why that is not a defeat

The ordered sequence. A log grows with what is appended to it, and that is the data rather than the
bookkeeping — `docs/bounded-space.md`'s rule is that state is bounded by membership, a window or a
configured capacity "never by the number of messages handled", and a log's entries are not messages
handled, they are the thing being kept. The module says so, and says what would bound it if a caller
needed that: snapshots, which are outside the paper.

## Risks / Trade-offs

- **The skip is a safety clause with no mutation** → the guard grows. `synod-skip-collected` removes
  the skip and every test that claims agreement must go red. Without it this change adds a clause the
  whole argument rests on and no evidence that anything tests it, which is precisely what
  `check-safety-tests.sh` exists to prevent.

- **Collecting changes what a run can observe, and the suite reads acceptor state** → several tests
  assert over `accepted_for` and `accepted_count`. A collected slot is absent from both, so a test
  that read absence as "never accepted" would now be wrong in the same way a leader would. Each is
  checked, and `agreement_survives_the_record_of_it_being_overwritten` is the one to watch: it
  already asserts that evidence can vanish while the fact stands, and collection is a second way for
  that to happen.

- **A run that never collects passes every bounded-state test vacuously** → the tests assert that
  collection *happened*, with the watermark advancing, before asserting what it bounded. A bound
  asserted over a run that collected nothing is the shape of vacuity this repository keeps finding.

- **The `2 f + 1` caveat looks like a bug** → a run with `f` crashed collects nothing, correctly and
  for ever. Tested and stated, because the alternative is somebody later "fixing" it.

## Open Questions

- **The retention window's length, and what it is measured in.** Time is what the paper says;
  entries or slots would be easier to test and easier to reason about. Decidable when the duplicate
  test is written, and it changes the specification's wording but not its shape — the guarantee is
  scoped to the window either way.

- **How often a replica reports.** It must be often enough that collection keeps up with the
  frontier and rare enough not to show in the cost identity. The identity test is what will say.
