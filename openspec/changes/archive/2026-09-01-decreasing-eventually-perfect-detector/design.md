## Context

`P` promises *if a process is detected, it has crashed* — never wrong, never retracted. `◇P` weakens
that to *eventually, no correct process is suspected*, and therefore must be able to take a suspicion
back: it has a `Restore` indication `P` does not. That one extra indication is the whole difference,
and it is what lets leadership return to a process that was wrongly accused or that has recovered.

## Goals / Non-Goals

Goals: a `◇P` that is deployable rather than academic; Ω resting on the detector its algorithm names;
the stack above re-verified under retraction. Non-goals: an accrual detector (roadmap — see below);
moving anything but Ω off `P`; changing what Ω computes from the suspected set.

## Decisions

### The timeout decreases, and the departure is the point

Algorithm 2.7 as printed:

```text
upon event ⟨ Timeout ⟩ do
    if alive ∩ suspected ≠ ∅ then
        delay := delay + Δ;                    // I was wrong; wait longer next time
    …
```

Increase on a false suspicion, and never decrease. Two departures here, and they are separable:

**Decrease during quiet.** After `quiet_rounds` consecutive rounds with no false suspicion, `delay`
comes down by one `Δ`, never below the configured floor. Down slowly, up fast: a detector that
decreased as eagerly as it increases would oscillate around the true bound and suspect the living
for ever.

**A cap.** `delay` never exceeds `max_delay`.

What each costs is different and both are stated in the module:

- The decrease trades *strict* eventual accuracy under partial synchrony for accuracy under a
  weaker and more realistic assumption — that the true delay bound eventually stops **changing**,
  rather than merely being finite. Under a bound that never settles, a detector that can come down
  can be wrong for ever. Under one that settles, it converges and the ratchet does not.
- The cap makes eventual accuracy conditional on `Δ_true ≤ max_delay`. Unconditional accuracy needs
  unbounded growth, because partial synchrony refuses to let you assume any bound in advance — but
  that is a property of the model, not of networks, and an operator knows their RTT distribution to
  within orders of magnitude.

**Why capping is the right loss.** Ask what a wrong `◇P` breaks. Ω trusts the wrong leader, which
costs an epoch change and an abort — liveness, never safety. Paxos above it is built for exactly
this: agreement is `[always]`, termination is already `[majority correct ∧ detector settles]`. So a
cap that is occasionally too small costs progress during a network episode and nothing else, and it
clears when the episode does. The uncapped ratchet costs progress *permanently* and silently. Both
are liveness failures; only one recovers.

### The decrease eases off on "nothing suspected", not "nothing withdrawn" — found during apply

The obvious reading of "a quiet round" is one in which no suspicion was taken back. It is wrong, and
wrong in the worst case: a network bad enough that suspicions are *never* withdrawn produces no
withdrawals, so the delay comes down exactly while the detector is being consistently wrong. Measured
against a network twelve times the initial delay, the delay drifted back to the floor instead of
holding near the cap.

The condition is therefore that nothing is suspected at all — no outstanding claim that could be
wrong, and the network evidently keeping up. A genuinely crashed peer, permanently suspected, then
**freezes** the delay wherever it reached. That is deliberate and stated: with a crashed process in
the membership there is no clean signal to ease off on, and freezing is better than the ratchet,
which grows, and than easing off blindly, which gets more wrong. Telling the two apart means
measuring the observed silence of the peers you are *not* suspecting — which is an accrual detector,
and the reason it is the next change.

### Algorithm 2.7's increase is driven by correction, not by error

`alive ∩ suspected ≠ ∅` fires when a suspected process is heard from: a false suspicion caught **in
the act of being corrected**. So the delay climbs with the rate at which the detector is *caught*,
not the rate at which it is wrong — and a detector consistently wrong, because every peer is beyond
the delay every round, is never corrected and never climbs. That is the book's behaviour rather than
anything added here, and it is the sharpest argument for the cap: the increase alone does not
converge on a bad network, so what bounds the failure is the ceiling, stated.

### `◇P2` is conditional, and the table says so

```text
◇P1 [always]                 Strong completeness — every crashed process is eventually permanently
                             suspected by every correct process
◇P2 [Δ_true ≤ max_delay ∧    Eventual strong accuracy — eventually no correct process is suspected
     Δ_true eventually
     stable]
```

The same shape as `PB2 [window]` and `SL1 [session]`: the guarantee is scoped and the scope is named,
rather than the guarantee being quietly weaker than the page's.

### A detector port, now that there are two detectors

```rust
pub trait Detector: Protocol { fn classify(ind: Self::Ind) -> DetectorInd; }
pub enum DetectorInd { Suspect(NodeId), Restore(NodeId) }
```

`P` classifies its `Crash` as `Suspect` and never produces `Restore`; `◇P` produces both. Ω handles
both arms and needs to know nothing else — the same shape as `Link::classify`, and for the same
reason. Constraint 4 says extract after two or three consumers; this is the second detector and the
first time anything could be generic over one.

*Alternative — give `P` a `Restore` variant it never raises.* Rejected for the reason `link.rs`
gives about `ScopedLink`: a variant that cannot occur is a case every consumer must write and no test
can reach.

### Ω defaults to `◇P`, and that is the risky part

Algorithm 2.8 names `◇P`; defaulting to it removes a departure rather than adding one. But it
changes the behaviour of `epoch_change`, `leader_driven_consensus` and both logged modules, none of
which has ever seen a detector retract. Expect leadership to return to recovered processes, more
`Trust` changes, and therefore more epoch churn — each costing an abort.

That churn is the thing to measure rather than assume, and `epoch_change`'s existing
`the_churn_after_a_leadership_change_is_finite` is the test that will say. If it proves unbounded
under a flapping detector, the fallback is in the open questions.

### What does not move

`uniform_reliable_broadcast` (Algorithm 3.4) and `flooding_consensus` (Algorithm 5.1) keep `P`, and
gain a line saying why: their agreement rests on strong accuracy by name, and one false suspicion
splits it permanently. `flooding_consensus`'s own suite already demonstrates that. Widening this
change to them would be converting two working modules into broken ones.

## Risks / Trade-offs

- **The stack above Ω may lose liveness under a detector that flaps.** → That is the point of the
  change and the reason it is worth doing; it is also why the churn test is named above. Measured,
  not assumed.
- **A decreasing timeout can oscillate.** → Down by one `Δ` after `quiet_rounds`, up by one `Δ`
  immediately: the asymmetry is what damps it, and a test drives a network whose latency steps up
  and then down and asserts the delay follows without thrashing.
- **A cap is a number someone picked.** → The simulator is this project's standard of evidence, so
  the suite sweeps the cap against a latency distribution and asserts the shape of the curve —
  false suspicions rising as the cap falls below the true delay. That also makes the trade
  non-vacuous rather than assumed.

## Migration Plan

1. The detector port, with `P` satisfying it and nothing else changed.
2. `◇P` — Algorithm 2.7, then the decrease, then the cap, each with its tests.
3. Ω generic over the port, still defaulting to `P`: no behaviour change, suites unchanged.
4. Flip the default to `◇P`; run everything above; fix or record what moves.
5. `stacks.rs`, docs.

Steps 1–3 are safe individually. Step 4 is the one that can surface work.

## The liveness gap this change exposed, and the options

Found at step 4 and measured rather than predicted: after a partition heals, every process trusts
the highest-ranked one, and that process never announces an epoch, because `⟨Ω, Trust⟩` is
edge-triggered on the leader *value* and it was already its own leader throughout. The other group's
epochs ran ahead, so nothing can proceed. Unreachable under `P`; crash-and-restart is unaffected,
because a restarted Ω trusts afresh from `⊥`.

- **A. Ω re-raises `Trust` whenever `suspected` changes**, even when the leader does not. Three
  lines. The leader is then told something changed and announces. Costs an epoch — an abort — on
  every suspicion change anywhere, including ones that cannot affect who leads. Ω's specified
  property survives, since it says trust is a function of the suspected set and that an *unchanged*
  set raises nothing; the scenario asserting a restoration below the incumbent raises nothing would
  be withdrawn.
- **B. A process prompts the leader it trusts.** Trusting `ℓ ≠ self` while in an epoch led by
  someone else, it sends `ℓ` a `Nack { nts: lastts }`, and the leader jumps its candidate above
  `nts`. Reuses the existing message and handler, targets exactly the stuck case, and incidentally
  fixes the climb — today a leader behind by 30 needs six round trips at `+N` each. A departure from
  Algorithm 5.5 and a change to `epoch_change`'s spec.
- **C. The leader re-announces periodically.** What production systems do, and robust against more
  than this one case. Costs a timer and standing traffic, against a stack that is otherwise silent
  when idle.
- **D. Keep `P` as Ω's default**, ship `◇P` and the port, and let the fail-recovery stack opt in
  where it is tested to work. Everything in this change stands; only the default changes.

**B was chosen**, with a delta spec for `consensus/epoch-change`. It is the smallest departure that
fixes the mechanism, reuses the message and handler already there, preserves quiescence — nothing is
sent while nothing has changed — and improves a convergence rate that was separately poor. The
general form of the problem, a holder of a standing fact re-announcing it so that whoever missed the
edge converges, is on `README.md`'s roadmap: this repair is reactive and narrow, and was found by a
test failing rather than by design.

## Open Questions

- ~~**If epoch churn under a flapping Ω proves unbounded**, keep `P` as Ω's default.~~ **Closed.**
  The churn test passes unchanged and a settled stack is silent. The fallback was not needed, and
  `stacks::EventualLeaderDetectorOverPerfectDetection` exists for anything that wants the old
  behaviour anyway.
- **The accrual detector is deliberately not here.** Reporting a *suspicion level* rather than a
  verdict, and letting each caller pick its own threshold — aggressive for Ω, where being wrong
  costs one epoch; conservative for a layer whose mistake costs safety — is the shape a deployment
  actually wants, and it needs the detector port this change builds. It is on the roadmap in
  `README.md`, and it wants its own change.
