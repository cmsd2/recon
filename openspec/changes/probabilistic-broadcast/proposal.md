## Why

The gossip protocols are the reason this project exists. `docs/postmortem.md` opens on the fact
that seventeen months produced 574 lines of algorithm — `upb.rs` and `lpb.rs`, imported once "for
posterity" — against several thousand lines of transport rewritten four times. Everything built
since has been the ordering that failure taught: links, then a detector, then the broadcasts, then
consensus, each tested against its stated guarantees under a deterministic simulator.

That ordering is now discharged far enough to reach the thing it was protecting. Probabilistic
broadcast is also the first abstraction here whose guarantee is *probabilistic* — it holds with high
probability rather than always — and the first whose verification therefore cannot be a single
seeded run. The simulator was built precisely so that "nine processes on ports 9000–9008 and tail one
log file", the post-mortem's summary of the old verification strategy, would not be the answer twice.

## What Changes

- **Eager probabilistic broadcast** (Cachin, Guerraoui & Rodrigues, Module 3.7 / Algorithm 3.9).
  On broadcast and on first receipt, relay to a random subset of peers with a rounds-to-live
  counter. Composes over the link port, so it runs over a perfect link, a session link, or an
  application's own.
- **Lazy probabilistic broadcast** (Module 3.8 / Algorithms 3.10–3.11 — the book splits it into a
  gossip half and a recovery half) over the eager one, adding
  per-sender sequence numbers, gap detection, retransmission requests to a random peer, and a store
  of messages held for recovery.
- **Both are bounded implementations, not transcriptions.** The book omits garbage collection
  explicitly (page 100: "garbage collection of the stored message copies is omitted in the
  pseudo code for simplicity"), so the retention mechanism is this project's own design and its cost
  is part of the specification. The guarantees are stated as scoped to the retention window rather
  than absolute.
- **A way to assert a probabilistic property.** A guarantee that holds with high probability is
  asserted over many seeds against a stated threshold, with both halves checked: that coverage
  clears the bound, and that it is not total — a fan-out that always reaches everyone is not
  probabilistic, and the test that cannot fail is the one this repository already guards against.

Explicitly **not** in this change: no transport, and no change to the link port, the timer handle,
or the simulator's fault model. If the simulator turns out to need a knob these protocols require,
that is its own change measured against `docs/conditional-guarantees.md`.

## Capabilities

### New Capabilities

- `broadcast/probabilistic-broadcast`: best-effort delivery to every correct process *with high
  probability*, by gossip — the guarantee, its dependence on fanout and rounds, and the fact that it
  may legitimately fail to reach everyone on some runs.
- `broadcast/lazy-probabilistic-broadcast`: the same delivery guarantee strengthened by recovery —
  a gap in a sender's sequence prompts a retransmission request, so a message missed by the gossip
  is recovered from a peer that stored it, within the retention window.

How the probabilistic guarantee is *evidenced* — many seeds, a stated threshold, and a non-vacuity
half checking the property is not trivially total — is a requirement of
`broadcast/probabilistic-broadcast` itself rather than a capability of its own. A separate
`verification/` capability was considered and rejected: this repository's spec tree has no such
top-level, one convention with two users is not yet a capability, and building the framework ahead
of its consumers is what `CLAUDE.md` constraint 4 exists to prevent. If a third probabilistic
protocol wants the same convention, extracting it then is a change with a proposal.

### Modified Capabilities

None. Both protocols compose over ports that already exist, and the many-seed sweep uses the
simulator exactly as it stands — a seed already reproduces a run, which is the only property the
sweep needs of it. Nothing in `simulation` or `protocol-core` changes, and a delta is not invented
to make the change look larger.

## Impact

- `crates/recon-protocols`: two new modules and their suites. Both compose over the link port
  introduced by `link-parameterisation`, so neither names a concrete link.
- `crates/recon-sim`: expected to be unaffected in behaviour. Many-seed sweeps use the existing
  `Sim` and `Config`; what is new is the shape of the assertion, not the simulator.
- `crates/recon-core`: unaffected. Randomness already arrives through `Cx::rng`, which is what makes
  a gossip protocol expressible here at all without touching `thread_rng`.
- `README.md`: a protocol table row for each module, and a suite row; `docs/bounded-space.md`, which
  currently records that everything above the failure detector is a transcription and unbounded —
  these two would be the first bounded implementations, so the document's own audit changes.
- **The reference implementation is a source of questions, not answers.** `archive/recon-gossip/`
  is read as notes; `docs/postmortem.md` records that of four bugs once claimed in it, three were
  false positives found by reading against remembered pseudocode, and were only settled by going to
  the book line by line. This change goes to the book. The defect that does stand — a garbage
  collection pass that is linear in everything ever received, run on every event — is exactly the
  cost the bounded-from-the-start decision has to avoid rather than reproduce.
