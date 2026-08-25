# What it cost

Notes written after the fact, per tasks 6.1 and 6.2.

## The resend filter was wrong, and a test found it

The clause this change exists to add was, at first:

```rust
// resend on re-establishment what the peer has not been seen to acknowledge
.filter(|(id, _)| !self.ack.get(id).is_some_and(|a| a.contains(&peer)))
```

It reads as an obvious economy and it deadlocks. `ack[m]` records who relayed `m` **to me**. It
says nothing about whether **my** relay reached them — and my relay is the token they are waiting
for. The failure needs four processes and one broken session:

- A broadcasts; D receives it and relays. D's `ack[m]` now contains A, because A's copy arrived.
- A's session with D ends before D's relay lands. A is missing exactly one acknowledgement.
- The session comes back. D consults `ack[m]`, sees A in it, concludes A already has `m`, and
  resends nothing. A waits forever.

`urb_uniform_agreement_holds_across_endings_and_re_establishment` failed on the third seed it
tried, with A alone in delivering nothing. Under Algorithm 3.4's perfect links the question never
arises: a relay sent once is a relay eventually delivered, so `ack` fills in on its own. The
session link withdraws that, and the relay has to become repeatable.

There is no cheaper sound rule available without a new message. Stopping once *I* have delivered
`m` is also unsound, by the same argument with the roles unchanged: delivering means everyone
correct relayed to me, which says nothing about my relay reaching them. Knowing when to stop
requires an acknowledgement, and an acknowledgement is a new communication step — excluded for
these rungs. So the resend is unconditional over `pending`.

Two things follow. Algorithm 3.4 never prunes `pending`, so a re-establishment costs one message
per broadcast ever made — the transcription's unbounded growth showing up as traffic rather than
only as memory. And the resend is directed at the peer whose session came back, which needed one
addition to the best-effort layer, `Cmd::SendTo`. That is not in Module 3.1, which has only
`broadcast`; it is the same wire message over the same link to strictly fewer recipients, so it
adds no communication step, and re-broadcasting to everyone on every reconnect would have been
an N-fold amplification for nothing.

## Did the separate modules read as copies?

Partly, and the parts that repeat are the parts that should.

`session_best_effort_broadcast` is close to a copy of its predecessor with the session reports
threaded through instead of swallowed. `session_reliable_broadcast` is nearly a copy: the eager
relay is unchanged, and the only difference is that it forwards two more indications.

`session_uniform_reliable_broadcast` is not a copy. It has the resend clause, the `SendTo` path,
and — the thing that could not have been shared — an entirely different relationship with the
session link. Reliable broadcast *cannot* repair a lost relay: it keeps identifiers rather than
payloads, relays once, and consults no detector, so it has nothing to send and no reason to send
it. That is the whole content of `the_difference_is_attributable_to_resending_and_accusation`.

This vindicates the decision against a generic session-aware layer, though not for the reason
originally given. The argument had been that the boilerplate is small. It is not especially small.
The argument that holds is that the divergence is at exactly the point a generic layer would have
had to abstract over: what a protocol does when told a suffix was lost. Best-effort broadcast
reports it, reliable broadcast reports it and cannot act, uniform reliable broadcast repairs it.
A shared layer would have had to make that a parameter, and the parameter is the algorithm.

## Did the two liveness paths separate in testing?

Yes, and only because the timing was chosen to force it. The design document flagged the risk that
a partition healing near the detection timeout could be resolved by accusation while appearing to
test the resend. Both tests now pin which mechanism fired by inspecting `correct` as well as the
deliveries:

- `urb_resends_on_re_establishment_with_the_peer_still_correct` breaks sessions for well under the
  detection timeout and asserts every process still calls D correct at the end.
- `urb_progresses_by_accusation_when_a_peer_never_returns` partitions for six detection timeouts,
  asserts D has left `correct` everywhere, and separately asserts from the trace that no message
  reached D after the cut — so the exclusion, and not a message getting through, is what unblocked
  the others.

One thing that test surfaced and the design did not anticipate: D, alone in a minority partition,
eventually suspects everyone else and becomes its own majority, so it delivers. That is the
detector's accuracy assumption being withdrawn, not a violation of uniform agreement — every
process that delivers, delivers the same message — but it means "the cut-off process delivers
nothing" is not an assertion this stack supports, and the test says so where it would have been
natural to assume otherwise.

## What moving the report cost

Splitting one session event into two — an ending naming the epoch that ended, an establishment
naming the epoch now in force — cost five tests in the session-link suite that had assumed lazy
establishment, and gained the property the change was for: liveness that does not depend on the
layer above having something to send. `urb_liveness_does_not_need_the_layer_above_to_send` heals a
partition and then broadcasts nothing at all; the link reconnects on its own, the establishment is
reported, and the resend follows.
