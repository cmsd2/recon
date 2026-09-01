## Why

`Sim::partition` takes groups, and `connected` asks whether two processes share one. So a partition
here is always symmetric **and transitive**: the network is a set of islands, and inside an island
everybody agrees about everybody.

Real networks are not like that, and the difference is not cosmetic. Under a transitive partition,
every process in a group sees the same world, so a leader detector agrees *by construction* — each
computes `maxrank` over an identical suspected set. The hard case has never been expressible:

```
    A ←──────→ B          A reaches B          A suspects {C} → trusts B
               │          B reaches C          B suspects {}  → trusts C
               ↕          A does NOT reach C   C suspects {A} → trusts C
               C
```

Three correct processes, three views, none of them wrong, and no partition to heal because nothing
is *broken* in the sense the simulator can express. Whether Ω converges here, and what epoch-change
does if it does not, is currently unknown and untestable.

That matters because this repository's guarantees are scope-annotated and several of those scopes
are conditions on the network. `◇P2` holds *provided the true delay is within the cap and eventually
stable*; `ELD1` inherits it; `leader_driven_consensus`'s termination is `[majority correct ∧ detector
settles]` while its agreement is `[always]`. A bridge is the fault that makes those conditions fail
one after another — and the suites have never run one, so the conditions have been documented rather
than demonstrated.

## What Changes

- **The network fault becomes a set of severed pairs** rather than a partition into groups.
  `Sim::sever(a, b)` cuts one pair, `Sim::reconnect(a, b)` restores it, and `Sim::partition(groups)`
  keeps its signature and meaning — it severs every pair that spans two groups. Every existing call
  site compiles and behaves identically; roughly thirty of them across the suites.
- **`Sim::reachable(a, b)`** so a test can assert the topology it built, and in particular that it
  really is non-transitive.
- **The session model follows for free.** `end_severed_sessions` is already written against
  `connected`, so a severed pair's session ends and re-establishes as it does today.
- **Tests of what the bridge does to the stack**, in the suites that own the claims: that Ω's
  agreement is not reached, that the detector beneath is the reason, and — the one that matters —
  that Paxos's agreement holds anyway, because it is `[always]` and a bridge is exactly the schedule
  that would break it if the scope annotations were wrong.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `simulation`: connectivity is between pairs rather than groups, so partitions may be
  non-transitive, and a test can ask what is reachable

## Impact

`recon-sim`'s `partitions` field changes shape and `connected` with it; `partition` and `heal` keep
their signatures. `docs/conditional-guarantees.md` gains the bridge as the fault its conditions lapse
under. New tests in `simulation.rs`, `eventual_leader_detector.rs`, `epoch_change.rs` and
`leader_driven_consensus.rs`.
