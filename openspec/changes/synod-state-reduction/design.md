## Context

`Pvalues<C>` is `BTreeMap<(Ballot, Slot), C>`, chosen so that Invariant A4 — at most one command per
ballot and slot — is the map's own property rather than something to check. That is the right shape
for the *set* the book writes, and it is the shape §4.1 replaces.

Three places hold one:

- `MultiPaxosSynod::accepted`, the acceptor's `α.accepted`.
- `Scout::pvalues`, the union `pvalues := pvalues ∪ r` over a majority's answers.
- `SynodMsg::P1b { accepted: Vec<Pvalue<C>> }`, the wire form.

`pmax` already reduces to one command per slot; it is the only reader.

## Goals / Non-Goals

**Goals:**

- Apply §4.1 as the page has it, with the paragraph quoted above the code that implements it.
- State why safety survives, in the paper's own terms, since the paper raises the doubt itself.
- Keep the safety guard's evidence intact: every registered test still red under both mutations.

**Non-Goals:**

- Bounding. The module stays a transcription, and §4.2 is what bounds it.
- `p1b` carrying a collected-slot watermark. That is §4.2's, and it exists because garbage
  collection makes absence ambiguous; §4.1 alone does not.
- Anything in `multi_paxos_replica`.

## Decisions

### `accepted` is keyed by slot, and the ballot rides in the value

`Pvalues<C>` becomes `BTreeMap<Slot, (Ballot, C)>`. Keying by slot is what §4.1 asks for, and the
ballot has to be kept because it travels in the `p1b` and because the scout compares on it when it
merges a majority's answers.

A4 stops being structural, and that is worth naming because it was the reason for the old key. It
does not stop being *true*: A4 is enforced by the leader — Invariant C1, at most one commander per
`⟨ballot, slot⟩` — and the module already says so where it explains why the acceptor's `≥` departure
is safe. What the old key bought was a second, redundant enforcement at the acceptor. What it cost
was a dimension of growth.

*Alternative considered:* keeping `BTreeMap<(Ballot, Slot), C>` and pruning lower ballots per slot
on insert. Same content, worse shape: the invariant a reader wants to see — one entry per slot — is
then a property of the pruning code rather than of the type, which is the opposite of the trade the
original key was making.

### The acceptor's reduction and the leader's maximum are different operations

Written first as one: a shared `keep_max` used by both the acceptor's `accepted` and the scout's
`pvalues`, on the reasoning that a session link can retransmit a lower ballot after a higher one and
so either could see them out of order.

**That is false of the acceptor, and a mutation is what said so.** Reducing `keep_max` to a plain
insert left the entire suite green, which sent the question back to the code: the acceptor's promise
already orders its own writes. A stored pvalue's ballot became `ballot_num` when it was stored,
`ballot_num` never falls, and the `b ≥ ballot_num` arm admits nothing below it — so every pvalue
that reaches the record is at least the one already there. The single case that is not strictly
above is equality, where A4 makes the command the same. A comparison at the acceptor is a branch
nothing can take, and this repository does not ship those.

The paper splits it the same way in the sentence being transcribed: an acceptor keeps "the most
recently accepted pvalue", and it is the *leader* that "needs to know … what the maximum pvalue is".

So the acceptor writes over its record, and `keep_max` is the scout's alone — where it is genuinely
load-bearing, because two acceptors can report different ballots for one slot and nothing orders
their answers.

### The scout's maximum was tested by nothing, and that predates this change

Mutating only the scout's collection to keep the last arrival also left the suite green — **and
`check-safety-tests.sh` passing**, which is the part worth recording. A scout that keeps the last
answer hands `pmax` a command a lower ballot proposed, and that is a split slot.

It is not a hole this change opened. Before it, the property was carried by the old
`⟨ballot, slot⟩` key's iteration order: `pmax` walked the map and the last write for a slot always
had the higher ballot. A structural property is one no test has to name, and none did. Moving the
reduction to collection time turns it into code, and code gets a test —
`a_scout_keeps_the_highest_ballot_reported_for_a_slot_not_the_last_one_to_arrive`, registered
against `synod-ignore-pmax` because that is the clause it protects.

### `pmax` stays, and becomes the identity

With the scout reducing as it collects, `pmax` reads a map that is already one command per slot. It
stays because what it names is the algorithm's step — Figure 7's
`proposals := proposals ◁ pmax(pvals)` — and a reader checking the code against the page needs to
find it. Deleting it would also leave the mutation `synod-ignore-pmax` with nothing to remove, and
that mutation is this change's principal safety evidence.

### Why safety survives, and why this is stated rather than assumed

§4.1 raises the doubt itself, and the answer is subtle enough to be worth quoting rather than
paraphrasing. After the reduction, a majority that accepted `⟨0, λ, 1, c⟩` need not still hold it: a
later ballot `⟨0, λ′⟩` accepted at one of them overwrites that acceptor's record. So there may be
"no proof that ballot `⟨0, λ⟩` even chose proposal `c`, as that part of the history has been
overwritten".

The fact survives the evidence because of C2: the leader of `⟨0, λ′⟩` had to read the maximum from a
majority, that majority intersects the one that accepted `c`, and so it selected `c`. Every later
ballot repeats the argument against the ballot below it. The chain is what carries the choice
forward once the record of it is gone.

This is the same shape as the repository's own rule about non-vacuity, seen from the other side:
what a trace can *observe* about a run and what is *true* of it are different, and here the
reduction removes an observation without removing a truth. The suite's checker reads decisions from
the trace rather than from acceptor state, so it is unaffected — which is worth knowing, because a
checker that had read `accepted` would now be wrong.

## Risks / Trade-offs

- **A4 is no longer structural** → mitigated by the fact that it never was the acceptor's to enforce
  and by C1's test, which is already in the suite. The module says which invariant is enforced
  where.

- **The safety argument now depends on a chain rather than a record** → this is the paper's own
  position and not a change introduced here, but it is newly *load-bearing* in this module. The
  mutation guard is what keeps it honest: `synod-ignore-pmax` breaks the chain's first link, and
  every registered test must still go red under it.

- **A test could pass because the reduction made a schedule unreachable** → the guard answers this
  directly. It is the same instrument that caught the decision announcement masking four tests, and
  it is run as part of this change rather than after it.

## Open Questions

None. §4.2's watermark in `p1b` is the next change and is deliberately not started here: it exists
because garbage collection makes an empty answer ambiguous, and after §4.1 alone an empty answer
still means "nothing accepted for this slot".
