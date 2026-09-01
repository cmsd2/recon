## 1. Identity survives the originator

- [ ] 1.1 Add `incarnation` to `probabilistic_broadcast::BroadcastId`, drawn from the seeded RNG in
      `on_init`; state the departure and the 2⁻⁶⁴ residual in the module
- [ ] 1.2 Verify a restarted originator's first broadcasts are delivered everywhere, and confirm
      the run without the incarnation would have discarded them (non-vacuity: the restarted
      sequence numbers collide with ones still in every window)
- [ ] 1.3 Update `the_wire_survives_encoding` and any test constructing a `BroadcastId`

## 2. The lazy module's gossip half takes its link as a parameter

- [ ] 2.1 `LazyProbabilisticBroadcast<P, L, G = FairLossLink<…>>`; `Gossiper<P, G>`; every scope
      event routed to both children; today's call sites compile unchanged
- [ ] 2.2 Verify both children receive a scope event, and that a boundary from either is
      propagated upward exactly once

## 3. Stacks

- [ ] 3.1 `ProbabilisticBroadcastOverSessions<P>` and `LazyProbabilisticBroadcastOverSessions<P>`
      in `stacks.rs`, with the prose saying what each does about a boundary

## 4. Eager gossip over sessions — `tests/probabilistic_broadcast_over_sessions.rs`

- [ ] 4.1 Coverage over sessions: a broadcast reaches everyone with a generous fanout, and a fanout
      that cannot cover the membership still leaves processes out — `PB1` stays probabilistic
- [ ] 4.2 **Message count is an identity**: `sends == k × (1 + receipts with ttl > 1)` over the
      whole run, and `sends == Σ kⁱ` per broadcast when no session ends
- [ ] 4.3 **Quiescence**: after the run is quiet, `sends` over a further window is zero
- [ ] 4.4 A session ending mid-gossip is propagated upward, what was in flight is lost and counted
      (`suffix_losses() > 0`), and nothing else is lost
- [ ] 4.5 Send rate is flat and state is bounded by the window, over sessions

## 5. Lazy gossip over sessions — `tests/lazy_probabilistic_broadcast_over_sessions.rs`

- [ ] 5.1 A gap caused by a session ending is repaired by recovery: the run contains a session end
      with suffix loss, and every process still delivers in sequence
- [ ] 5.2 Requests are bounded by gaps detected; answers by requests received where the message was
      stored; both asserted from the trace
- [ ] 5.3 Quiescence, flat send rate, bounded windows — over sessions
- [ ] 5.4 A boundary is propagated once, not once per child

## 6. What this dates

- [ ] 6.1 `README.md`: the two stack aliases, the two suites, the real-world-set section
- [ ] 6.2 `docs/bounded-space.md`: the gossip row in the deployment table says the second
      obligation is met
- [ ] 6.3 `./scripts/check.sh` passes in full
