## Why

`CLAUDE.md` now names a real-world set: a small number of protocols maintained as things that would
ship, held to a second standard — they run over the session link rather than the stubborn one, and
their resource use is checked, not assumed. The gossip pair is that set today, and neither module
yet meets the standard:

- **Both default to the fair-loss link**, which is right for reading against Algorithm 3.9 and
  wrong for a deployment. Worse, `lazy_probabilistic_broadcast`'s gossip child is **hard-wired** to
  `FairLossLink` — only its recovery traffic takes the link parameter — so it cannot be put over a
  session link at all today.
- **Identity does not survive an originator restarting.** A `BroadcastId` is `(origin, seq)` with a
  volatile `seq`, and every receiver's `delivered` window is keyed on it. An originator that crashes
  and comes back restarts at one, and its first `window` broadcasts are silently discarded
  everywhere as duplicates. Under the fair-loss model nobody restarts; in a deployment they do, and
  a session boundary cannot tell a receiver which — the link cannot distinguish a reconnect from a
  restart (`docs/conditional-guarantees.md`), and a gossip receiver's link is to a relayer, not to
  the originator anyway.
- **Nothing counts the messages.** The suites assert coverage, termination and bounded state.
  Nothing asserts that a broadcast costs what the algorithm says it costs and not one message more,
  or that an idle gossip sends nothing — which over a session link it should, since there is
  nothing beneath it retransmitting.

## What Changes

- **Aliases in `stacks.rs`**: `ProbabilisticBroadcastOverSessions<P>` and
  `LazyProbabilisticBroadcastOverSessions<P>`. The lazy module's gossip child becomes generic over
  its link, so both halves of it run over sessions; two `SessionLink` instances share one wire the
  way `uniform_reliable_broadcast`'s two children do.
- **`BroadcastId` gains the originator's incarnation** — a value drawn from the seeded RNG at the
  originator's `Init`, so a restarted originator names its broadcasts differently without stable
  storage and without any receiver having to guess what a session boundary meant. Departure from
  the book, which keys on the message itself; the existing departure to an identifier is extended,
  not replaced.
- **Resource use is asserted**, in new suites over the session link: a broadcast's send count equals
  exactly what Algorithm 3.9 specifies for the receipts that occurred, and is bounded by the closed
  form; an idle gossip sends **nothing**; a session ending is propagated and costs only what was in
  flight; lazy recovery repairs what a session ending lost, with requests bounded by gaps and
  answers by requests; state stays bounded by the window and send rate stays flat.
- **Both suites keep their fair-loss form.** The book's reading stays testable against the book's
  link; the real-world form is an additional suite each, not a replacement.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `broadcast/probabilistic-broadcast`: identity is scoped to the originator's incarnation, so no
  duplication holds across an originator's restart; a broadcast sends exactly what the algorithm
  specifies and an idle process sends nothing; a session boundary is propagated
- `broadcast/lazy-probabilistic-broadcast`: the gossip half runs over the same kind of link as the
  recovery half; recovery bridges a session ending; requests and answers are bounded by gaps and by
  requests

## Impact

`probabilistic_broadcast::BroadcastId` changes shape (wire-visible; nothing durable holds one).
`LazyProbabilisticBroadcast` gains a second link type parameter with a default, so today's call
sites compile unchanged. `stacks.rs`, `README.md`'s protocol and suite tables, and
`docs/bounded-space.md`'s deployment table are dated. Two new suites.
