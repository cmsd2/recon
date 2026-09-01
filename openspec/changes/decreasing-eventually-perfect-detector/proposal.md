## Why

Ω is derived here from the **perfect** failure detector, and Algorithm 2.8 says
`Uses: EventuallyPerfectFailureDetector`. The module records the substitution as a departure. In the
crash-stop model it was a harmless one — a detector that is never wrong satisfies "eventually right"
trivially. In the fail-recovery model it is not, because a crashed process **comes back**:

- `P`'s accusations are permanent by construction, so `suspected` only grows and `maxrank` only ever
  walks *downward* through the membership. A recovered process can never lead again, and after
  enough crash/recover cycles there is no candidate left at all.
- Every restart test above Ω is written around this. `leader_driven_consensus`'s no-majority test
  says so in its own comment: it uses crashes rather than a partition because a healed partition
  never heals for the detector.
- `P` is only implementable where a delivery bound Δ is known in advance. Nothing outside a
  simulator has one, so the real-world set cannot use it.

The book's answer is Algorithm 2.7, "Increasing Timeout", which adds `Δ` to its timeout on every
false suspicion. **It never subtracts.** That is a ratchet, and its cost is not the unboundedness
but the irreversibility: one bad period leaves detection permanently sluggish, long after the
network recovered, with nothing reporting that it has. A capped detector's failure lasts as long as
the bad network and then clears; the ratchet's lasts for the rest of the run.

## What Changes

- **A new module implementing Algorithm 2.7**, quoting the page, with two departures: the timeout
  **decreases** during sustained quiet, and its growth is **capped**. Both are stated, and so is
  what they cost the guarantee.
- **A detector port**, as `link.rs` is a link port. Two detectors now exist, which is the second
  consumer that justifies extracting one — build it now and not before, per constraint 4.
- **Ω takes its detector as a type parameter, defaulting to `◇P`**, which removes the departure its
  own documentation records. `stacks.rs` names the composition over `P` for anything that wants the
  old behaviour.
- **Uniform reliable broadcast and flooding consensus keep `P`, and say why.** Algorithm 3.4's
  uniform agreement and Algorithm 5.1's agreement both require *strong* accuracy; a detector allowed
  to be wrong would break them, which is precisely what the majority-ack broadcast beside them
  exists to avoid. This change must not quietly widen.
- **The stack above Ω is re-verified** under a detector that retracts, which it has never seen.

## Capabilities

### New Capabilities

- `failure-detection/eventually-perfect-failure-detector`: eventual strong accuracy, a suspicion
  that can be retracted, and a timeout that adapts in both directions within a stated bound

### Modified Capabilities

- `failure-detection/eventual-leader-detector`: the detector beneath is a parameter, and trust may
  return to a process that was suspected
- `consensus/epoch-change`: a leader is told where the processes trusting it have reached, so that
  one which never observed a change of its own still starts an epoch — the liveness gap a detector
  that retracts exposes, found while implementing this change and recorded in `design.md`

## Impact

New module and suite. `eventual_leader_detector` gains a type parameter with a default that
**changes which detector it uses**, so the epoch-change, Paxos and logged-Paxos suites all run
against a detector that can retract for the first time — the risk this change carries. `stacks.rs`,
`README.md`, `docs/bounded-space.md` and `docs/conditional-guarantees.md` are dated.
