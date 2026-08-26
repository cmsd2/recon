## Why

The ladder reaches consensus. Uniform reliable broadcast was the last rung below it, and
constraint 6 fixes what comes next.

Flooding consensus is worth writing not because it would be deployed — it would not — but because
it is the sharpest available demonstration of what a *perfect* failure detector buys and what it
costs. Its agreement property rests entirely on the detector's strong accuracy: the book's own
proof says so, and one false suspicion splits the decision permanently. The consensus algorithms
that are deployed — the leader-driven family, Paxos and its descendants — are built the opposite
way round, safe regardless of what the detector says, with the detector buying only termination.
That contrast is the point, and it cannot be drawn without the first half of it.

It also establishes the `Propose`/`Decide` vocabulary that every consensus rung after it reuses.

## What Changes

- A new protocol module transcribing Cachin, Guerraoui & Rodrigues Algorithm 5.1, over the
  existing perfect failure detector and best-effort broadcast. Rounds of flooded proposal sets; a
  round in which nobody new was detected as crashed is a round in which it is safe to decide.
- **Its dependence on strong accuracy is demonstrated, not asserted.** A schedule is found in
  which a false suspicion — the detector withdrawn from the timing assumption that makes it
  perfect — leaves two correct processes deciding differently. This is the same shape as the
  existing `best_effort_broadcast_does_violate_agreement_under_the_same_test` and
  `reliable_broadcast_does_violate_uniform_agreement_under_the_same_test`: the rung below fails
  the property the rung above holds, and here it is the *assumption* below that fails.
- The module is labelled a transcription, academic, fail-stop, with its space bound stated.

Explicitly **not** in scope: hierarchical consensus (Algorithm 5.2), the uniform variants
(Algorithms 5.3 and 5.4), the eventually perfect failure detector, eventual leader election, and
anything leader-driven. Each is its own rung.

## Capabilities

### New Capabilities

- `consensus/flooding-consensus`: regular consensus in the fail-stop model — termination,
  validity, integrity and agreement, the last of these holding only while the failure detector is
  perfect. Rounds, the flooded proposal set, the decision rule, and the scope of the guarantee.

### Modified Capabilities

None. The perfect failure detector and best-effort broadcast are used exactly as specified; no
requirement of either changes.

## Impact

- New module `crates/recon-protocols/src/flooding_consensus.rs`, registered in `lib.rs`.
- New suite `crates/recon-protocols/tests/flooding_consensus.rs`.
- Composes two children, so it follows the established pattern: concrete typed fields, a private
  helper per child, `Cx::with_child_consuming` for the transforming path. Same shape as
  `uniform_reliable_broadcast`, which also drives a broadcast and a detector.
- Adds a `consensus/` tree to `openspec/specs/`.
- `README.md`: a consensus section in the protocol tables, and the "Next" line moves on.
- No change to `recon-core` or `recon-sim`. Nothing new on the wire beyond this layer's own
  messages, and no new fault-injection knob — the schedule that breaks accuracy is reachable with
  the partition and timing controls that already exist.
