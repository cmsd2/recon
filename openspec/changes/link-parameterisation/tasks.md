## 1. The port

- [x] 1.1 Replace the `Link` trait now on the branch — which pins the perfect link's request and
      indication types, and so cannot admit the session link — with the port proper, carrying the
      request and indication types as associated types, and verify the project builds
- [x] 1.2 Define `ScopedLink` as `Link` plus the reporting of scope endings and establishments, and
      verify a link cannot implement it without raising those boundaries
- [x] 1.3 Implement `Link` for the perfect link, and verify it declares no scope boundary — a link
      may not name a scope it cannot observe. A blanket impl will not do: it would make every
      protocol a link, including the ones composed over one
- [x] 1.4 Implement `Link` and `ScopedLink` for the session link, and verify `cargo test --workspace`
      still passes with nothing composed over the new traits yet
- [x] 1.5 Add a compile-fail or negative test showing a link that does not satisfy the port is
      rejected when the project is built, so the seam is checked rather than asserted

## 2. Best-effort broadcast over any link

- [x] 2.1 Parameterise `BestEffortBroadcast<P, L>` over `L: Link<P>`, defaulting to
      `PerfectLink<P>`, and verify every existing call site compiles untouched
- [x] 2.2 Add a constructor taking the link, and verify `cargo test -p recon-protocols --test
      best_effort_broadcast` passes unchanged
- [x] 2.3 Move `delivered_count` to the impl block for the default link, since it is specific to the
      perfect link, and verify the reliable-broadcast tests that call it still pass
- [x] 2.4 Pass scope boundaries reported by the link upward when `L: ScopedLink`, and verify both an
      ending and an establishment reach the layer above, distinguishable from one another
- [x] 2.5 Offer the directed send to one member on the base module, and verify only the addressed
      member receives it

## 3. The layers above

- [x] 3.1 Parameterise `ReliableBroadcast` over the broadcast beneath, with a default, and verify
      its existing suite passes unchanged
- [x] 3.2 Parameterise `UniformReliableBroadcast` and verify the establishment prompts a directed
      resend while the ending prompts nothing. The resend is **not** bounded on `ScopedLink` — it is
      called from the `Link` impl's indication arm, so the tighter bound would fall on every link;
      what makes it unreachable over an unscoped link is the port's own guarantee. See `design.md`
- [x] 3.3 Parameterise `MajorityAckUniformReliableBroadcast`, keeping the resend unconditional and
      directed, and verify a test that a filtered resend would deadlock still passes
- [x] 3.4 Parameterise `FloodingConsensus`, and verify its existing suite passes unchanged
- [x] 3.5 ~~Thread each layer's `Timer` type parameter with a default mirroring the layer's own~~ —
      **obsolete**. `opaque-timers` removed `Protocol::Timer`, so no layer's timer type mentions its
      child's and there is nothing to thread. Nothing to do; verified by there being no `type Timer`
      in the workspace

## 4. Collapsing the forks

- [x] 4.1 Move the `session_best_effort_broadcast` suite onto `BestEffortBroadcast` with a session
      link, and verify every test passes without weakening an assertion
- [x] 4.2 Move the `session_broadcast` suite likewise, and verify the reliable-versus-uniform
      contrast it draws still holds
- [x] 4.3 Move the `session_majority_ack_uniform_reliable_broadcast` suite likewise, including the
      stall test that spends the `suspend` knob
- [x] 4.4 Merge each pair of module docstrings, checking the quoted pseudocode and departures list of
      both originals survive, and verify no departure is lost by reading the merged text against
      both
- [x] 4.5 Delete the four `session_*` modules and their registrations in `lib.rs`, and verify
      `cargo test --workspace` passes with the counts accounted for
- [x] 4.6 Delete the four `openspec/specs/broadcast/session-*` directories as part of the same
      change, and verify `openspec validate --all --strict` passes

## 5. Somebody else's link

- [x] 5.1 Add a test link satisfying `Link` with no retransmission, no deduplication and no timer,
      and verify a broadcast delivers over it
- [x] 5.2 Verify non-vacuously that the foreign link really is a different stack — its wire carries
      the bare payload, not the built-in link's identifier
- [x] 5.3 Run consensus over the foreign link and verify every correct process decides, no two
      decide differently, and what is decided was proposed
- [x] 5.4 Verify the seam runs one way only. The stated check — that the diff is confined to test
      files — cannot be run, because this change edits every protocol to *build* the seam. What was
      checked instead: no module under `src/` mentions the foreign link, and `foreign_link.rs`
      imports no link at all, naming its own request and indication types rather than borrowing the
      perfect link's. It was borrowing them until this task was worked; fixing that is what makes
      task 5.2's claim true

## 6. The documents this dates

- [x] 6.1 Update `docs/conditional-guarantees.md` where it says the seam is a rule layers are asked
      to follow, and verify the section describes the port as it is built
- [x] 6.2 Update `README.md`'s protocol table for the four removed modules, and verify the suite
      counts and totals it claims against what `cargo test --workspace` prints. The specification
      tree gains `links/link-port` only when this change is archived and its New Capability is
      synced, so that line stays as it is until then
- [x] 6.3 Update `CLAUDE.md`'s composition conventions to say a parent names its child's port rather
      than its child, and verify no other convention it states has been contradicted
- [x] 6.4 Run `./scripts/check.sh` and verify it passes in full
