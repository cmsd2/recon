## Context

See `proposal.md` — Why. Four things about the present state shape the approach.

**There is a previous implementation, and it is evidence rather than a template.**
`archive/recon-gossip/src/{upb,lpb}.rs` transcribes these two algorithms. `docs/postmortem.md`
records that four bugs were once claimed in it and **three were false positives** — each found by
reading the code against remembered pseudocode, each settled only by going to Cachin, Guerraoui &
Rodrigues §3.8 line by line. That episode is the strongest available argument for a rule this design
adopts: **every clause is settled against the book, not against the old code and not against a
claim about the old code.** The old code is worth reading for the shape of the data structures,
which is work already done once, and for nothing else.

**The book contradicts itself in one place, and the contradiction matters.** Algorithm 3.10 stores
a message when `random([0,1]) > α`; the prose on page 99 says it stores "with probability α". Page
100 breaks the tie — setting α = 0 is described as everyone storing — which only holds under the
pseudocode's reading. The old implementation copied the pseudocode into its code and the prose into
its comment, reproducing the inconsistency faithfully. This design has to pick one and say so.

**Randomness already arrives correctly.** `Cx::rng` is seeded and virtual, so a gossip protocol is
expressible here without touching `thread_rng`. This is the one thing the first attempt got right
by accident and this project got right on purpose: a probabilistic protocol whose randomness came
from the environment could not be replayed, and the whole verification approach below depends on
replay.

**The link is a parameter now.** `link-parameterisation` landed, so these modules bound on the port
and name no implementation. Nothing here needs the session half of it.

## Goals / Non-Goals

**Goals:**

- Two modules that can be read against pages 95–100 with every departure stated.
- A verification approach for a probabilistic property that is neither vacuous nor flaky.
- Bounded state whose reclamation cost does not grow with the run, since that is the one defect in
  the previous implementation that survived scrutiny.

**Non-Goals:**

- Tuning fanout and rounds to a target reliability. The specification requires a threshold to be
  *stated and justified*, not that any particular number be achieved.
- The epidemic-broadcast literature beyond these two algorithms — no anti-entropy, no push-pull, no
  membership protocol. `Π` stays fixed and known, as everywhere else in this repository.
- Making the guarantee deterministic. A configuration that always reaches everyone is explicitly a
  test failure, not a success.

## Decisions

### The α in lazy probabilistic broadcast follows the pseudocode: store when `random > α`

*Note added while implementing.* `docs/postmortem.md` disagrees with itself here. Its re-examination
concludes the pseudocode's `random([0,1]) > α` is right and the book's prose is loose; its own worked
sketch further down writes `cx.rng().gen_bool(self.alpha)` — storing *with* probability α — and calls
it "right way round". Both are true if the field is renamed to mean the store probability, which is
what this module does: it takes a `store_probability`, and the book's α is one minus it. The
parameter is named for what it does precisely so that this question cannot be asked again.

So α is the probability of *not* storing, and α = 0 means store everything. The module documents
that the book's prose says the opposite, and why the pseudocode wins — page 100's own example only
makes sense under this reading.

*Alternative considered — follow the prose, store with probability α.* It reads better and it is
what the field name in the old code suggests. It contradicts page 100, and it would make α = 0 mean
"store nothing", under which recovery cannot work at all. Rejected, but the parameter is named for
what it does rather than for the book's letter, so that no reader has to remember which way round it
is.

### Verification is a sweep with a stated threshold, and the threshold is derived, not tuned

A test states fanout, rounds and membership, states the coverage it requires, and shows the
reasoning for the number. It then runs many seeds and compares. Each seed remains a reproduction
handle for its own run.

The number is derived from the configuration rather than observed and pasted back, because a
threshold read off a run is a record of what the code did, not a claim about what it should do —
and it will be re-pasted the next time it fails, which is how a suite stops being evidence.

*Alternative considered — pin specific seeds known to give full coverage.* Deterministic and
cheaper. It asserts a fact about those seeds, and it passes unchanged if the fanout is set to the
whole membership, which is the specific way this abstraction can be broken into a different one.
Rejected on that.

*Alternative considered — assert only the deterministic mechanism.* Honest and much simpler, and it
leaves the headline guarantee — the reason the abstraction exists — untested. Rejected, but the
mechanism assertions are kept alongside the sweep, because they are what will localise a failure
when the sweep goes red.

### Every sweep carries a non-vacuity half asserting coverage is not total

`assert!(reached < total)` beside `assert!(reached >= threshold)`. This is the same guard as
`tests/method.rs`'s: an absence-of-violation property is satisfied by a protocol that does nothing,
and a coverage property is satisfied by a protocol that has stopped being probabilistic. A fanout
raised to `|Π|` turns this abstraction into best-effort broadcast, all assertions still passing.

The consequence, accepted: the suite's configuration cannot be made "safer" by widening the fanout,
because that breaks the test. That is the point.

### The retention window is a count of messages, not a duration

A process keeps the last *N* delivery records per sender, and the stored copies likewise, with `N`
configured. Reclaiming happens when a record is inserted, by evicting from the far end — constant
work per event.

*Alternative considered — expiry by age, as the previous implementation did.* This is the one
defect in that code that survived scrutiny: `delivered_gc()` drained and rebuilt the entire
delivered-set on every `poll()`, so receiving one message cost time linear in everything ever
received. A count-bounded window has no such pass. Age is also the wrong axis here — what makes an
old record safe to discard is that the sender has moved far enough past it, which is a count.

*Alternative considered — no bound, as the book has it.* This is what every other broadcast in this
repository does, and it is what `docs/bounded-space.md` calls the failure mode that hides. Rejected
deliberately: these are the first two modules specified as bounded implementations, and the window
is stated in their guarantees rather than omitted from them.

### Lazy has two children, not one — corrected against the book

An earlier draft of this section said lazy "composes over eager". Algorithm 3.10's header says
otherwise:

```text
Uses:
    FairLossPointToPointLinks, instance fll;
    ProbabilisticBroadcast, instance upb.   // an unreliable implementation
```

Both. Data goes out through `upb` and is gossiped by it; **requests and their answers go directly
over `fll`**, bypassing the gossip entirely. That is what makes the recovery phase *pull* rather
than more push, and it is the reason the algorithm is called lazy. A version routing requests
through `upb` would flood every recovery, which is the cost the second phase exists to avoid.

So this layer multiplexes two children into one wire, the same shape
`uniform_reliable_broadcast` already uses for its broadcast and its detector.

### Three more corrections the book supplied

Recorded because each was about to be written the other way, and because the section's whole lesson
is that these questions are settled by the page rather than by reading code.

- **Sequence numbers start at one.** `next := [1]^N`, not `[0]^N`. A zero-based `next` would leave
  every process waiting for a message its senders never send.
- **The timeout skips *past* the gap, not to it.** `if sn > next[s] then next[s] := sn + 1` — so the
  message at `sn` is abandoned along with everything before it. Setting `next[s] := sn` would deliver
  a message the process has already decided to skip over.
- **Draining `pending` is a standing condition, not a procedure call.**
  `upon exists [DATA, s, x, sn] ∈ pending such that sn = next[s]` is re-evaluated whenever `next` or
  `pending` changes, so closing a gap can release an arbitrarily long run in one go. Written as a
  loop after every mutation of either, which is the same thing and is what a handler-based
  implementation has to do.

### The eager relay stays outside the delivery guard

Algorithm 3.9 places the relay at the same level as the `if m ∉ delivered` block, not inside it, so
a process relays messages it has already delivered. This is deliberate in the book, which names the
consequence on the same page — "any given process may receive the same message many times".

Recorded as a decision rather than left to the code because it *looks* like a bug, was once reported
as one, and will be reported as one again by the next careful reader. The module quotes the
pseudocode with the indentation intact and says so.

## Risks / Trade-offs

- **A threshold too close to the observed rate makes the suite flaky.** A sweep that passes at 190
  of 200 today fails on an unrelated change that shifts the schedule slightly. → Derive the
  threshold with margin and state the margin. A flaky probabilistic test teaches people to re-run
  CI, which costs more than the test is worth.

- **The sweep is slow.** Two hundred runs of a multi-process simulation, twice over for the
  recovery comparison, in a suite that currently completes in seconds. → Keep membership and message
  counts to the minimum that makes the property visible, and measure the cost before settling the
  numbers. `alloc_probe.rs` already takes eight seconds and is the current worst; this should be
  compared against that rather than allowed to become the new worst by default.

- **Bounding the window changes what the module can claim, and the claim is easy to overstate.**
  No-duplication holds within the window and not beyond it. → The scope is in the requirement, in
  the module's guarantee table, and pinned by a test that lets a record expire and observes the
  second delivery. The failure mode is a module that quietly claims the book's unqualified property.

- **A false positive is the most likely failure of this work, not a false negative.** The
  post-mortem's episode is three careful readings producing three wrong bug reports. → Every
  departure from the page is justified in the module against a quoted clause, and anything that
  looks wrong but is the book's own is recorded as such — the relay placement and the α direction
  are both already known cases.

## Migration Plan

Additive throughout; nothing existing changes behaviour.

1. Eager probabilistic broadcast over the link port, with the deterministic mechanism tests —
   fanout size, rounds decrementing, termination, no duplication, no creation.
2. The retention window and its bound test, before the sweep, so the sweep runs against the module
   as it will ship rather than against an unbounded draft.
3. The coverage sweep and its non-vacuity half. Measure the wall-clock cost here and settle the
   numbers against it.
4. Lazy probabilistic broadcast over the eager module: sequence numbers, gap detection, requests,
   the pending set, the timeout that moves past an unrepairable gap.
5. Its own window bound, then the recovery comparison — the same seeds with and without recovery,
   in a configuration where gossip alone demonstrably fails.

Rollback is per-step; each step is a module or a test and reverting one leaves the rest standing.

## Open Questions

- Whether the two modules should share a sequence-number type, or whether the eager one should carry
  no sequence at all and let lazy add it. The book gives eager no sequence number; adding one there
  would simplify lazy at the cost of departing from Algorithm 3.9. Answerable when lazy is written,
  and it changes neither specification.
- Whether the recovery comparison belongs in the lazy suite or in `tests/method.rs`, which already
  exists to verify that assertions are worth something. It is a claim about two protocols rather
  than about one, which is an argument for the latter. Does not change the approach.
