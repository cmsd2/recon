## Why

The real-world set's second obligation is resource use, and it has two halves: **minimal messages
for the work done — a test that counts them against what the algorithm needs, not just that it
terminates** — and no growth of state or send rate with how long a run has been going. Multi-Paxos
meets the send-rate half and has never been asked the first. Nothing in either suite counts a
message against what the algorithm needs; `assert_send_rate_flat!` asks only that the rate stops
growing, which a protocol sending ten times too much at a constant rate satisfies.

Asked, the answer is measurable and the gap is large. Ten entries at five processes, after
leadership has settled, with a delivery bound of 20 ms:

| `retransmit` | `p1a` / `p1b` | `p2a` / `p2b` | `decision` | phase two against the minimum |
|---|---|---|---|---|
| 10 ms (the default every suite uses) | 15 / 15 | 179 / 179 | 40 | **3.6×** |
| 100 ms | 5 / 5 | 50 / 50 | 40 | **1.0×** |

The second row is the algorithm's exact cost, and it is worth writing down because it is the whole
reason Multi-Paxos exists: `n` `p1a` and `n` `p1b` **per leadership change**, then `n` `p2a`, `n`
`p2b` and `n − 1` `decision` **per entry**. Phase one is paid once for the run, not once per entry,
which is exactly what `consensus_based_total_order_broadcast` cannot say. That is an identity, of
the kind `probabilistic_broadcast_over_sessions` already asserts for gossip, and it should be
asserted rather than described.

The first row is what the code actually does under the timing every suite configures, and the cause
is not subtle: `retransmit` is 10 ms against a delivery bound of 20 ms, so the retry sweep resends
`p2a` to every acceptor that has not answered *before an answer could possibly have arrived*. Two
thirds of phase two is a protocol talking over itself.

Underneath the number is a design point. The sweep is a **stubborn link's** idiom — resend on a tick
because the network may have dropped it — and this module runs over a **session link**, which does
not drop anything while a session holds. The only loss is at a session ending, and the link raises
`SessionEstablished` to say when a resend is possible again; `session_link.rs` describes that event
as "the moment on which anything that must be resent can be". **Nothing in this repository acts on
it.** Every module that composes a session link propagates the event and does nothing else, so the
one event that says "resend now" is the one thing retransmission ignores, and a fixed tick below the
round trip stands in for it.

## What Changes

- **The cost identity is asserted**, in the shape the gossip pair's is: a test that counts each
  message kind over a run whose work is known, against a closed form in `n` and the number of
  entries, with the leadership changes counted separately so phase one's amortisation is visible
  rather than folded into an average.

- **Retransmission is driven by the scope, not by a tick below the round trip.** Two parts:

  - A `p2a` or `p1a` is resent only once the time it has been outstanding exceeds what a round trip
    can take, rather than on every sweep. The sweep interval stops being the retransmission
    interval, which is what conflating them cost.
  - `SessionEstablished` triggers a resend to that peer immediately. That is the event's stated
    purpose, and it makes recovery from a session ending prompt instead of waiting out a timeout
    chosen for a different job.

  Together these make the sweep a backstop rather than the mechanism, which is what running over a
  session link should mean.

- **A message a process sends to itself becomes a call.** §4.4 colocates the roles — this module
  "holds **both** the acceptor and the leader" — so a `p2a` from a leader to its own acceptor is an
  intra-process function call that the wire currently carries. Counting it as a message overstates
  the cost by `2` of every `3n − 1`; worse, the simulator can break the session and *drop* it,
  modelling a process failing to deliver a message to itself. Removing it takes the identity to
  `3(n − 1)` per entry and removes a fault that cannot happen.

- **The timing the suites configure is stated against the delivery bound.** `retransmit` below the
  bound is not a tuning choice, it is a mistake, and the module should say what the parameter has to
  exceed, in the same terms `detect_after`'s note already does.

## Capabilities

- `consensus/multi-paxos-synod` — modified

## Impact

- `crates/recon-protocols/src/multi_paxos_synod.rs` — the retry sweep, the scope handler, and
  `transmit`.
- `crates/recon-protocols/tests/multi_paxos_synod.rs` and `multi_paxos_replica.rs` — the cost
  identity, the resend on establishment, and the schedules that currently depend on the sweep firing
  every 10 ms. Several hand-driven tests tick and settle; a sweep that no longer resends on every
  tick may change how many rounds they need.
- `scripts/check-safety-tests.sh` — must still pass. Changing when a message is resent changes which
  interleavings a seeded run reaches, which is exactly how the decision announcement masked four
  safety tests.
- `README.md` — the real-world set section, which currently says the resource-use obligation is
  unmet. Half of it becomes met.

**Not** in scope: `docs/bounded-space.md`'s state rows. This change is about messages; §4.2 is about
state, and it remains the thing that admits Multi-Paxos to the real-world set.

The `SessionEstablished` observation generalises to every module over a session link, and this
change deliberately does not generalise it. One module acting on the event is the evidence that
doing so is worth the others; a framework built before its second consumer is the mistake this
repository already made.
