## Context

The first object here with a concurrent interface. Everything up to now decides one value once or
delivers one message, which is why the evidence track stalled: a workload generator has nothing to
issue overlapping operations against, and a checker over a trivially linearizable history proves
nothing.

Both algorithms are already supported by what is built. Algorithm 6.1 needs reliable broadcast and
consensus; the fail-recovery one needs logged uniform reliable broadcast and logged uniform
consensus. All four exist.

## Goals / Non-Goals

Goals: a port on the model of `link.rs`; the two algorithms behind it; one suite written against the
port and run against both.

Non-goals: multi-Paxos; bounding either collection; linearizable reads; retiring a decided round's
consensus instance beyond what dropping the child already does.

## Decisions

### This is a transcription, and the space bound is stated rather than fixed

Both algorithms grow `unordered` and `delivered` without bound, and the fail-recovery one grows its
sequence in stable storage too. That is the page, and `docs/bounded-space.md` is explicit that
inheriting the book's omissions is correct of a transcription and disqualifying of an implementation.
So each module states it, and neither collection is bounded here.

This is what made the change tractable. An earlier reading of item `4` had it building multi-Paxos,
where retiring a decided slot's consensus instance is not optional — a parent rewriting one blob
holding `n` children's records writes `O(n²)` bytes over its life, which is the defect the logged
modules already had and fixed. A transcription of the page has no such obligation, and the whole of
that work moves to the change that builds something deployable.

### The port offers a read, and the read is local

The book's interface is `Broadcast` and `Deliver`, with no read. Both algorithms nonetheless maintain
`delivered`, the totally ordered sequence, and a log's clients read it — so the port exposes what the
page keeps and does not offer, which is a departure of one method rather than of the algorithm.

Local, because that is all either algorithm can honestly serve. A read that observed every completed
append would have to go through consensus or hold a lease, which is not on the page and would change
what is being transcribed. The claim is therefore a **total order** — checkable directly from the
histories — and not linearizability. Item `H` already distinguishes the two and says which needs
which.

The consequence worth stating: reads and appends now genuinely overlap, which is what items `G` and
`H` were waiting for, and prefix-consistency between two reads is a property a checker can be held
to without searching.

### The pair is the point, and the suite belongs to the port

Uniform reliable broadcast against its majority-ack twin; flooding consensus against Paxos. Here it
is crash-stop against fail-recovery, and what differs is exactly one thing: whether the sequence
survives a restart. A shared suite makes that visible rather than asserted, and it means the second
implementation costs its own module and nothing else.

### Consensus instances are a family, not one replaced — and the book says so

**Corrected during implementation.** This section, and task 2.2, had the instance "held as a `Child`
and replaced as the round advances, the mechanism `leader_driven_consensus` already uses for its
epoch consensus". That was an import from an algorithm where it is right — a new epoch genuinely
supersedes its predecessor — into one where nothing supersedes anything.

The page indexes instances: "Initialize a new instance `c.round` of consensus", and
`⟨ c.r, Decide | decided ⟩` names an arbitrary `r`. So instances are a family, held in a map keyed by
round and created on demand — including when a message arrives for a round this process has not
started, which is what lets a peer that is ahead make progress.

A second argument was offered for the family while this was being written — that replacement opens a
liveness hole, because processes drift and a message for a round a peer has not started would be
dropped once and never resent by the deduplicating link beneath. **Measured, it does not arise for
this member of the pair.** Algorithm 6.1 is fail-stop, its consensus needs a perfect failure
detector, and it therefore runs synchronously: every process decides a round in the same instant and
starts the next in the same instant. Across five rounds no process is ever ahead of another, and one
replaced instance would have sufficed here.

So the family rests on faithfulness alone, which is enough — `c.round` is what the page writes — and
the drift argument is recorded as unproven rather than quietly kept.
`under_synchrony_no_process_runs_ahead_of_another` pins the lock-step, so that if drift ever appears
the assertion says so instead of the family silently starting to matter. The fail-recovery member,
over a Paxos that assumes no synchrony, is where it is expected to.

### A conditional event handler is a runtime facility this repository does not have

The other half of the same correction, and the more general fact.

`upon event ⟨ c.r, Decide | decided ⟩ such that r = round` is not a guard that discards. The book
states its meaning outright: "An algorithm that uses conditional event handlers relies on the
run-time system to **buffer external events until the condition on internal variables becomes
satisfied**." A decision arriving for a round this process has not reached is *held*, not lost.

`Cx` and `Sim` provide no such facility — every event is delivered immediately, and nothing is ever
held on a condition. So **every `such that` in every algorithm transcribed here is a departure the
module discharges itself**, and this one buffers decisions by round and acts on each when `round`
reaches it.

There is precedent, which is what confirms the reading rather than merely permitting it:
`leader_driven_consensus` already holds a `StartEpoch` in `pending` until the abort it triggered
completes, because `upon event ⟨ ep.ts, Aborted | state ⟩ such that ts = ets` needs exactly this. The
pattern is established; what was missing was noticing that it is the book's convention rather than an
implementation detail of Paxos.

### Composition's mapper widens from `fn` to `impl Fn`

The core change, and the reason it is not expedience. A function pointer captures nothing, so a
parent cannot stamp a child's messages with anything it knows. That has never bitten because the
stamp has always belonged to the child — `epoch_consensus::Tagged` exists precisely so "a parent
cannot forget to". The round here belongs to the parent: `FloodingConsensus` has no notion of one,
and without a stamp there is no way to route a message to its round's instance, which is the safety
failure `Tagged`'s own documentation describes.

Three ways round it were tried and all hit the same wall one level down, because everything composes
through the same mapper: a stamping wrapper protocol, a stamping link, and putting the round in the
proposed value.

Two things make widening the right fix rather than a workaround. `Effect::map` already takes
`impl FnOnce`, so the sinks were the last place pinning a pointer — an inconsistency rather than a
stance. And `cx.rs`'s stated reason for the type is that mappers "are normally enum variant
constructors, so a wrong one is a type error", which closures preserve exactly.

### The consensus beneath is not a type parameter

Every other composing layer takes its child as a parameter with a default. This one cannot, and the
reason is worth recording because it looks like an oversight: consensus instances are created **at
run time**, one per round, so the layer would need a link *factory* rather than a link, and a factory
is the runtime indirection static composition exists to avoid. The reliable broadcast keeps its
parameter, because there is one of it and the caller supplies it once.

### Where the capability lives

Under `consensus/`, because the book places total-order broadcast in *Consensus Variants* and because
inventing a domain level for two modules would not pay for itself. The port gets its own capability,
as `links/link-port` does.

## Risks / Trade-offs

- **Transcribing through OCR.** The indexed text loses some symbols — `if m ∈ delivered` should read
  `∉`, and `upon unordered = ∅` should read `≠`; both are unambiguous from the algorithm, but the
  module's quoted pseudocode must be checked against the page rather than pasted from a search
  result.
- **A deterministic sort must be genuinely deterministic**, and this repository has already been
  bitten by iteration order: `HashMap` is banned outright because its order varies per process. The
  sort key must be a total order over `(sender, entry)` that every process computes identically.
- **Two modules and three specs is a large change** for something that is deliberately not
  deployable. → The port and the suite are what justify it, and both are reused by everything item
  `5` eventually adds.
- **The instance family is a third unbounded collection**, alongside `unordered` and `delivered`, and
  is never pruned. → Faithful: the book keeps `c.round` for every `r`. Pruning a past round's
  instance is what would break the liveness argument above, so this is not an omission that could be
  quietly fixed. Both implementation specs already require the module to state that its state is
  unbounded; this is a third thing for that statement to name.
- **Lock-step rounds make for slow runs.** One consensus per round, no pipelining, and consensus here
  is Paxos over an epoch-change. Suites will need to be sized for that, and it is worth measuring
  early rather than discovering in the last group.

## Migration Plan

1. The port, with its `compile_fail` that a protocol is not a log by accident.
2. Algorithm 6.1, quoted from the page, with its departures listed.
3. The shared suite, written against the port, run against 6.1.
4. The fail-recovery variant, and the suite run against it too, plus what only it claims — that the
   sequence survives a restart.
5. Docs: the roadmap's `4–5`, the spec tree, the protocol tables, the suite table and counts.

## Open Questions

- **Which consensus sits under Algorithm 6.1.** The page says `Consensus`, which `flooding_consensus`
  satisfies literally; `leader_driven_consensus` provides *uniform* consensus, which is stronger and
  also satisfies it. Flooding consensus is fail-stop and needs a perfect failure detector, so the
  choice is really between "what the page says" and "what would survive a partition". Resolve when
  the module is written, and record which and why.
- **Whether the fail-recovery variant's algorithm number should be quoted.** The search returned its
  name and text but not its number; find it in the page before writing the module header, rather than
  guessing one.
