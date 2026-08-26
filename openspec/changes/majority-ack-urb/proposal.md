## Why

Every uniform reliable broadcast in this repository depends on a failure detector that must never
be wrong. `uniform_agreement_breaks_when_the_timing_assumption_is_withdrawn` demonstrates what
happens when it is: a wrongly accused process is dropped from `correct`, the delivery condition is
satisfied too early, and a message is delivered by some processes and not others. Flooding
consensus showed the same dependency one rung up, and showed that stabilising afterwards does not
repair it.

Algorithm 3.5 removes the dependency rather than mitigating it, and the book states the change
exactly:

> Except for the function `candeliver(·)` below and for the absence of ⟨Crash⟩ events triggered by
> the perfect failure detector, it is the same as Algorithm 3.4.
>
> `function candeliver(m) returns Boolean is return #(ack[m]) > N/2;`

One predicate, and the detector comes out. What replaces a detector that must never be wrong is a
**majority that must be correct** — `N > 2f` — which is a standing assumption about the deployment
rather than a moment-to-moment assumption about the network. It is the same trade the leader-driven
consensus algorithms make, available here for a single function, and having it beside the all-ack
version is what makes the trade legible.

It matters most on the session stack. There, the detector is currently load-bearing for liveness:
a peer that never returns has to be *accused* before anything can be delivered. Under a majority
quorum nobody needs to be accused at all — the majority is reachable or it is not — which leaves
resending on re-establishment as the only liveness mechanism and removes an entire failure mode.

## What Changes

- A new module transcribing Algorithm 3.5 over `best_effort_broadcast`: the same algorithm as
  `uniform_reliable_broadcast` with `candeliver` replaced and the failure detector, its heartbeats,
  its timer and its wire arm all removed. **`Cmd::Start` disappears**, because there is nothing to
  start.
- A new module applying the same predicate over `session_best_effort_broadcast`, keeping the resend
  clause that `session_uniform_reliable_broadcast` added and dropping the detector alongside it.
  **The wire stops multiplexing**, because only one child sends.
- **The contrast is demonstrated, not asserted.** The schedule that breaks the all-ack version's
  uniform agreement must leave the majority-ack version intact, and the reason must be visible: no
  process is ever excluded, because no process is ever accused.
- Both modules state the assumption they now rest on — a correct majority — and what fails without
  it: with `N ≤ 2f` the algorithm blocks rather than diverging, which is a different and more
  tractable failure than a split delivery.

Explicitly **not** in scope: garbage collection of `pending`, `ack` or `delivered`, which stay as
the book leaves them; the eventually perfect failure detector; and any change to the existing
all-ack modules, which stay exactly as they are so the contrast has two sides.

## Capabilities

### New Capabilities

- `broadcast/majority-ack-uniform-reliable-broadcast`: uniform reliable broadcast in the
  fail-silent model — the same four guarantees, resting on a correct majority instead of on a
  perfect failure detector, with no detector and no failure-detection messages at all.
- `broadcast/session-majority-ack-uniform-reliable-broadcast`: the same over session links, where
  a lost suffix is repaired by resending on re-establishment and a peer that never returns needs
  no accusation because it was never waited for.

### Modified Capabilities

None. The all-ack capabilities keep their requirements unchanged; the point of this change is that
there are now two, and they differ in what they assume.

## Impact

- Two new modules in `crates/recon-protocols/src/`, registered in `lib.rs`, and two new test
  suites.
- Each is a smaller protocol than the one it is derived from: one child instead of two, no wire
  multiplexing in the session case, no `Start` command, and `correct` gone from the state.
- No change to `recon-core` or `recon-sim`. No new fault-injection knob — the schedules that
  distinguish the two versions already exist in the all-ack suites.
- `README.md`: two rows in the protocol tables and a note on what the trade is.
- The bounded-space position is unchanged: `pending` and `ack` still grow. Removing the detector
  removes a timing assumption, not the collection debt.
