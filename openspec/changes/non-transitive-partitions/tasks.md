## 1. Connectivity is between pairs

- [x] 1.1 Replace `partitions: Option<Vec<BTreeSet<NodeId>>>` with a normalised set of severed
      pairs, and read it from `connected`
- [x] 1.2 Reimplement `partition(groups)` over it — sever every pair spanning two groups — and
      `heal()` as clearing the set. **Every existing suite passes untouched**, which is this step's
      whole evidence: roughly thirty call sites depend on `partition` meaning what it meant. 529
      tests, no suite edited
- [x] 1.3 Verify the session model follows: a severed pair's session ends and re-establishes as a
      partitioned one does today, with the existing session suites unchanged

## 2. Severing a pair

- [x] 2.1 `Sim::sever(a, b)` and `Sim::reconnect(a, b)`; verify no message crosses a severed pair in
      **either** direction, and that traffic to and from every other process is unaffected
- [x] 2.2 `Sim::reachable(a, b)`; verify it reflects every severing and healing applied so far,
      including through `partition` and `heal`
- [x] 2.3 Verify a non-transitive topology is expressible and is what it claims: some pair reachable,
      some pair not, a third process reaching both. This is the non-vacuity guard for every test
      below — a bridge that is accidentally transitive tests nothing
- [x] 2.4 Verify severing and reconnecting a pair mid-run behaves as `partition`/`heal` does, and
      that `heal` clears a severing made by `sever` as well as one made by `partition`

## 3. What the bridge does to the stack

Each of these **records** what happens rather than asserting a predicted answer, except 3.4 which is
the safety claim and is the reason the fault is worth having.

- [x] 3.1 `◇P` under a bridge: the two processes that cannot reach each other suspect each other, and
      the suspicion is never withdrawn while the bridge stands. That is `◇P2`'s condition failing —
      the network's, not the implementation's — and the module says so; assert it happens
- [x] 3.2 Ω under a bridge: record whether the three converge on one leader. They may not, and not
      converging is `ELD1`'s stated condition lapsing rather than a defect
- [x] 3.3 `epoch_change` under a bridge: record whether epochs settle. If they do not, record whether
      the reason is a repairable one, as the healed partition's was, and scope any fix as its own
      decision rather than absorbing it here. **They settle**, and not for the reason expected: `A`
      trusts a process that trusts somebody else and so never announces, and `A` starts nothing
      because it is not its own leader. The disagreement is stable rather than churning. No repair
      needed and none made
- [x] 3.4 **`leader_driven_consensus` under a bridge: agreement holds.** `UC2` is `[always]` and a
      bridge is the schedule that breaks it if the scope annotations are wrong. Over many seeds, and
      with a non-vacuity half confirming the runs really were bridged and really did decide
- [x] 3.5 Record whether the majority `{B, C}` makes progress while `A` is left behind — the outcome
      the design's open question asks about, and interesting either way. **It routes around the
      bridge.** With `{A,B}` severed from `{D,E}` and `C` reaching everyone, `{C,D,E}` decides in
      20 of 20 seeds and `{A,B}` never does. Zero splits — `UC2` held through the schedule built to
      break it

## 4. What this dates

- [x] 4.1 `docs/conditional-guarantees.md`: the bridge is the fault under which the chain of
      conditions lapses in order, and the first one this simulator can express
- [x] 4.2 `README.md`: the simulator's fault list, the suite counts, and the roadmap — `A` moves from
      next to built
- [x] 4.3 `./scripts/check.sh` passes in full
