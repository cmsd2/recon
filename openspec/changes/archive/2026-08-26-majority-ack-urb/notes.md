# What the exercise showed

Notes written after the fact, per task 6.2.

## Did the majority versions come out smaller?

Yes, and by more than the one function the book advertises. Counting what left rather than what
changed:

| | all-ack | majority-ack |
|---|---|---|
| Children | broadcast, detector | broadcast |
| Wire type | an enum multiplexing two children | the child's message, unwrapped |
| Commands | `Start`, `Broadcast` | `Broadcast` |
| Timer type | an enum over two children | the child's timer |
| State | `pending`, `ack`, `delivered`, `correct` | `pending`, `ack`, `delivered`, `members` |
| Delivery guard called from | two paths — a delivery, and a crash | one path — a delivery |

The last row is the one worth dwelling on. In the all-ack version the delivery predicate has two
inputs that can change, so `check_deliverable` has to be called from both child helpers, and the
flooding consensus work showed how easily that second call looks redundant and how badly it is
missed. Here `ack` growing is the only thing that can satisfy the guard, so there is one call site
and no way to forget the other. Removing an assumption removed a place to make a mistake.

The wire type is the other one. This is the first place in the stack where a message type gets
*simpler* going up: with one child there is nothing to multiplex and no discriminant to add. Every
other rung so far has accumulated structure on the way up.

## What the five-process requirement cost, and what it caught

The design fixed on five processes because the all-ack suite's schedule for breaking uniform
agreement splits four processes two and two, and against a quorum *neither side is a majority*:
nothing is delivered anywhere, and "no two processes disagree" passes without meaning anything.
Five gives a three-two split with a real majority side, so the contrast is asserted alongside a
positive delivery count.

That was the right call and it was not sufficient. Mutating `2k > N` to `2k >= N` — the off-by-one
that lets exactly half count as a majority — **passed all seventeen tests**, because with an odd
`N` the two predicates are identical. Half of five is not a whole number, so the boundary the
mutation moves does not exist there.

Both suites now pin the boundary at an even membership as well, running a four-process instance
solely for that purpose. The mutation fails there, as it should. The general shape of the mistake:
the constant chosen to make one property observable made another property unobservable, and only
mutating the code revealed it. Same lesson as the flooding consensus near-miss, arrived at from
the opposite direction.

Two further mutations were checked on the session version: `>=` (caught by the even-membership
test) and `ack.len() == members`, which is all-ack with no detector (caught by three tests,
including `a_peer_that_never_returns_needs_no_accusation`).

## Was "no detector" assertable from the trace?

Yes, and the first two attempts were not. Both began as assertions that every message sent was a
broadcast payload — which is trivially true, since the message type has no other variant, so the
assertion reduced to `sends().count() == send_count()` and could never fail.

What is actually observable is the *absence of traffic when there is nothing to say*. The all-ack
version sends heartbeats forever whether or not anything is broadcast; the majority version, given
nothing to broadcast, sends nothing at all. Both suites now assert that directly, and the
fair-loss one asserts the contrast in the same test by running the all-ack version through the
same idle window and showing it chattering.

That is the honest form: a property about what the protocol does not do, asserted as a difference
in behaviour rather than as a restatement of the type.

## What this implies for the rungs after it

- **The quorum discipline is the thing to carry forward**, not the detector. Leader-driven
  consensus is safe under any behaviour of `Ω` for exactly the reason majority-ack is safe under
  any behaviour of the network: the safety argument counts, and counting cannot be wrong about who
  has crashed.
- **`◇P` and `Ω` are still needed, but for liveness only.** Having built a rung whose safety does
  not depend on a detector, the shape of what a detector is *for* is clearer: it buys termination,
  and buying it badly costs a round rather than a guarantee.
- **The collection debt is untouched and now more conspicuous.** These are the first modules that
  could plausibly be deployed on their assumptions alone, and `pending` still grows for ever, and
  a re-establishment still sends all of it. That is the next thing that is wrong.
