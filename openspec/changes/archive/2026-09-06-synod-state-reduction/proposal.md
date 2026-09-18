## Why

`multi_paxos_synod`'s acceptor keeps `accepted` keyed by `⟨ballot, slot⟩`, so a slot commanded
under three ballots holds three pvalues, and `on_p1a` returns **the whole map** in every `p1b`. A
leadership change therefore puts every pvalue this acceptor has ever accepted on the wire, and the
scout unions those sets from a majority before `pmax` reduces them to one command per slot.

The source names this as the first thing to fix, and gives the reason in one sentence: a leader
"only needs to know if this set is empty or not, and if not, what the maximum pvalue is". Everything
below the maximum is read by nothing. §4.1 is a page and a half of the paper this module is a
transcription of, and applying it keeps the module a faithful transcription rather than departing
from one.

This is the smallest of the three changes `docs/bounded-space.md` names for Multi-Paxos and the only
one that costs no guarantee. It does **not** bound the module: an acceptor still keeps one pvalue
per slot and slots still grow, so the capability stays a transcription and stays unbounded. What it
removes is a second dimension of growth — ballots — and the message size that came with it.

## What Changes

- **`accepted` is keyed by slot, holding the highest-ballot pvalue for it.** §4.1: "acceptors only
  maintain the most recently accepted pvalue for each slot (`⊥` if no pvalue has been accepted)".
  An acceptor asked to accept under a ballot at or above the one it holds for that slot replaces it;
  one below leaves it alone.

- **`p1b` carries one pvalue per slot**, which is the whole point — the message is what grew
  fastest, and it grew with the number of *ballots* the run had seen as well as the slots.

- **The scout's `pvalues` accumulator is reduced the same way**, so unioning a majority's answers
  keeps one command per slot rather than one per `⟨ballot, slot⟩`. `pmax` then reads a map whose
  every entry it uses.

- **The module documents §4.1's "worrisome effect" and why safety survives it.** The paper raises it
  itself: after the reduction there can be no majority of acceptors storing the same most recently
  accepted pvalue, "and in fact no proof that ballot `⟨0, λ⟩` even chose proposal `c`, as that part
  of the history has been overwritten". The reduction destroys the *evidence* that a proposal was
  chosen without touching the *fact*, and Invariant C2 is what carries the fact forward. That
  distinction is exactly the kind this repository states rather than assumes, so it goes in the
  module beside the departures.

- **The capability's space requirement is corrected.** It currently says an acceptor "keeps every
  proposal it has accepted", which stops being true. It stays unbounded, and stays a transcription.

## Capabilities

- `consensus/multi-paxos-synod` — modified

## Impact

- `crates/recon-protocols/src/multi_paxos_synod.rs` — the acceptor, the scout, and `Pvalues`.
- `crates/recon-protocols/tests/multi_paxos_synod.rs` — a test that the reduction happened, and the
  existing safety suite unchanged.
- `scripts/check-safety-tests.sh` — must still pass with every registered test red under both
  mutations. This change touches the state the whole safety argument reads, so the guard is the
  evidence that it did not quietly weaken it.
- `docs/bounded-space.md` and `README.md` — the row and the roadmap item that name §4.1 as pending.

No other module composes `MultiPaxosSynod` except `multi_paxos_replica`, which sees no change: the
reduction is entirely below `Ind::Decision`.
