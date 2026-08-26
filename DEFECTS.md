# Contract audit — defects and fixes

2026-08-26, against the working tree containing the synchronous-store (`log-storage`) change.
Scope: every module in `recon-protocols` read against its quoted Cachin–Guerraoui–Rodrigues
pseudocode and stated properties; `recon-core` and `recon-sim` read against the invariants in
`docs/conditional-guarantees.md` and `docs/scope-annotated-modules.md`. Line numbers are from
this working tree. What checked *clean* is listed at the end, so absence from this file means
verified, not unexamined.

## Critical — a stated guarantee can actually break

### 1. Logged URB reuses `BroadcastId`s after recovery

`crates/recon-protocols/src/logged_uniform_reliable_broadcast.rs:139,158,275-276`

`seq` is volatile: the constructor zeroes it, `Meta` is `()`, and `on_recovery` replays records
without restoring it. A recovered process that broadcasts something *new* mints
`BroadcastId { origin: me, seq: 1 }` again. Replay then makes `pending[(me,1)]` last-write-wins
across two distinct payloads, acks accumulated for the old payload count toward the new one, the
delivered-by-id guard lets at most one of the two deliver locally, and different processes can
log-deliver *different payloads under the same id*. No-creation, validity, and uniform agreement
all fail — in the one module whose purpose is the fail-recovery model, where a recovered process
is *correct* and expected to keep participating.

**Fix:** in `on_recovery`, recompute `self.seq` as the max `seq` over replayed records with
`origin == me`. This is safe: every own broadcast appends a `Pending` record in the same handler
that emits it, and a torn write discards that handler's sends, so no escaped broadcast lacks a
record. (Persisting the counter in `Meta` also works but costs a metadata write per broadcast.)

**Test that would have caught it:** crash A, restart A, have A broadcast a fresh message, assert
everyone delivers both the old and the new. No recovery test currently starts new work after
recovering — they all resume pre-crash broadcasts.

### 2. Logged link has the same volatile-`seq` hazard

`crates/recon-protocols/src/logged_link.rs:151,235-236`

A restarted sender reuses `MsgId (me, 1), (me, 2), …`; a recipient whose durable log already
contains those ids silently drops the distinct new payloads forever, while the sender's stubborn
link retransmits them forever. Strictly hidden behind LPL1's sender-doesn't-crash premise, but it
is an undocumented consequence of the documented id-keyed departure (`:66-68`), in a
fail-recovery module. Unlike defect 1, the sender cannot recompute from its own store — the log
holds *received* messages only.

**Fix:** persist the send counter as `Meta` (this is exactly what `Meta` is for — "a position, a
count, an epoch"), or key the durable dedup set by content as the book does. Either way, state
the choice in the departures list. `perfect_link.rs:84` has the same shape; there it is
legitimately out of scope (crash-stop — a crashed process is not correct), but one sentence
documenting the sender-incarnation hazard is owed, since the sim freely restarts processes.

### 3. Suspension silently absorbs loss inside a live session

`crates/recon-sim/src/sim.rs` — `suspend`, `dispatch`

`suspend` does not end sessions, but a `Deliver` to a suspended node is dropped (the `crashed()`
predicate counts `Suspended`) while the session stays up — and `ScopeEnd` events to a suspended
node are dropped too, where timers are deferred and re-armed. A message is therefore lost with
no `SessionEnded` ever raised, violating the session model's own invariant — delivered, or the
session ends — that every resend clause above is built on. Concretely: suspend B briefly across
a broadcast, resume; session-majority-ack URB delivers everywhere except B, which is correct
throughout and never delivers. Uniform agreement fails, permanently, inside the simulator's own
model. This is the documented cardinal sin, committed by the simulator rather than a layer.

**Fix:** end the suspended node's sessions (the postmortem's stale-write teardown — the honest
model of "unreachable"), or buffer in-session deliveries for suspended nodes as timers are
deferred. Independently, defer `ScopeEnd` for suspended nodes — it is a local notification, not
a network message. Add at least one session_* test that suspends; none does today.

### 4. `restart()` conflates crash-recovery with suspend-resume

`crates/recon-sim/src/sim.rs:244`

Resuming a *suspended* node re-runs `on_init`/`on_recovery` on a protocol whose volatile state
was preserved — replaying a startup branch on a process that never lost anything. Currently
masked by idempotent inits and the PFD's `armed` guard, but semantically wrong, and it feeds
defect 5.

**Fix:** split the API — `resume(node)` for suspended (re-arm deferred timers, no startup
branch), `restart(node)` for crashed only.

## Significant — contract text or model wrong, damage currently latent

### 5. A resumed node permanently accuses every live peer

`crates/recon-protocols/src/perfect_failure_detector.rs:179-187`, with defect 4

While suspended, deliveries to the node are dropped and its `Tick` deferred. On resume,
`on_init`'s `armed` guard returns before `assume_alive`, so `last_heard` is stale by the whole
suspension; the deferred tick then accuses everyone, and detection is permanent. The module's
stall analysis (`:44-48`) covers only *being* accused, never *accusing*, and the suspension test
checks A, B, C's views but never D's own.

**Fix:** once `resume` exists (defect 4), refresh `last_heard` there — or via a resume-scope
event. At minimum, document the accusing-side stall and pin the behavior with a test.

### 6. Session URB's docstring quotes the wrong algorithm

`crates/recon-protocols/src/session_uniform_reliable_broadcast.rs:25-27`

The quoted added clause is `forall (s,m) ∈ pending such that q ∉ ack[m] do trigger
⟨ beb, Broadcast | … ⟩`. The code implements an *unconditional*, *directed* (`SendTo`) resend —
and the module's own inline comment (`:211-223`) proves the quoted filtered variant deadlocks.
The sibling majority-ack module quotes it correctly. In a repo whose method is reading code
against the quoted page, the quote asserting the deadlocking variant is a defect, not a typo.

**Fix:** replace the quote with the sibling's.

### 7. `RB2 [always]` over-claims in session reliable broadcast

`crates/recon-protocols/src/session_reliable_broadcast.rs:25`

`delivered` is volatile, so a restarted recipient re-delivers a late relay. The repo's own
`docs/scope-annotated-modules.md` (Corollary 7.2) forbids `[always]` for this configuration, and
the sibling correctly tags `URB2 [incarnation]` for the identical mechanism.

**Fix:** retag `[incarnation]`; add a crash/restart-recipient test (none exists).

### 8. Logged URB's `Pending` append is mis-ordered

`crates/recon-protocols/src/logged_uniform_reliable_broadcast.rs:227-255`

The sbeb re-broadcast effect is emitted before the `Record::Pending` append, and the early
delivery `return` skips the deferred append entirely — unreachable today only because N=1
pre-inserts via `on_cmd`. The invariant "everything in in-memory `pending` has a durable Pending
record" holds by accident of control flow. `store.rs` states the write-then-send contract in
code-order terms, and `Cx` explicitly supports eager sinks, under which this process's ack could
escape with no durable pending.

**Fix:** append `Record::Pending` at the point of insertion into `pending`, before the ack block
and before emitting the rebroadcast.

### 9. Stale-session deliveries arrive after `SessionEnded`

`crates/recon-protocols/src/session_link.rs:102-121`, `crates/recon-sim/src/sim.rs` `end_session`

`end_session` keeps a random prefix of in-flight messages scheduled at future times but fires
`ScopeEnd` at `now`, and resets `last_delivery` — so old-session deliveries can land after
`Ended`, after the successor's `Established`, even interleaved behind new-session traffic. The
wire carries no epoch, so the layer above cannot attribute a delivery to a session interval —
which is what a scope boundary is *for* — and any layer that resends on `Established` (as
`conditional-guarantees.md` prescribes) invites cross-epoch duplicates.

**Fix:** flush the kept prefix before scheduling the `ScopeEnd` (a real transport delivers
nothing on a connection after surfacing its termination), or tag the wire with the epoch and
document that stale deliveries may trail the ending.

### 10. `docs/conditional-guarantees.md` §scope-composition is stale

The section still says the scope mechanism is "not implemented yet" and describes scopes
composing upward through per-layer mappers. The implementation routes the concrete
`SessionEvent` downward through `on_scope_end` with no scope mapper in `with_child`, and
delivers `Established` — a scope *beginning* — through a handler named `on_scope_end`. The code
is internally consistent and correctly typed; the governing doc no longer describes it.

**Fix:** rewrite the section from the code; rename the handler (`on_scope_event`) or extend
Definition 2 to cover beginnings.

## Minor

11. `crates/recon-sim/src/sim.rs:200` — `crash`'s doc comment is fused onto
    `crash_on_next_write` (the new fn was inserted between doc and `fn`); `crash` is now
    undocumented.
12. `TraceEvent::WriteLost` fires even when the doomed write *landed* (`keep = true`). Rename to
    `DiedWriting`, or split the landed/lost outcomes.
13. `crates/recon-sim/src/sim.rs` `ensure_session` — a self-send creates a `(me, me)` session
    and the `[(a,b),(b,a)]` loop schedules `Established{peer: me}` twice per node; session URB
    then runs `resend_to(self)` twice. Masked by dedup above; skip `a == b` or dedup the loop.
14. `crates/recon-protocols/src/stubborn_broadcast.rs:47-49` — `Cmd::Stop` is unusable:
    `Broadcast` mints N internal `SendId`s never returned to the caller. Surface them, or
    document that `Stop` is unreachable API.
15. `crates/recon-protocols/src/stubborn_link.rs:117-123` — re-`Send` on a live `SendId`
    silently replaces the prior transmission, whose SL1 lapses. State the uniqueness
    precondition on `Cmd::Send`, or `debug_assert` it.
16. `crates/recon-protocols/src/stubborn_link.rs:20-31` — the pseudocode quote elides the book's
    `⟨ sl, Init ⟩` clause. The departure is documented at `arm()`, but the quote reads as
    complete.
17. `crates/recon-protocols/src/perfect_failure_detector.rs` — heartbeats bypass the book's `pl`
    layer. Equivalent under the synchronous config, but it is a fourth departure missing from
    the "three departures" list; outside synchronous mode loss forges silence directly.
18. `crates/recon-protocols/src/logged_link.rs:142-145,211` — `contains` is a linear scan and
    each indication clones the whole log: O(n²) *work*. The module's cost note claims linear
    write cost — true but incomplete by `docs/bounded-space.md`'s own standard, which counts
    work as well as space.
19. `crates/recon-protocols/src/session_link.rs:66-69` — `epochs` is never cleared on `Ended`,
    so `epoch()` reports a dead session as "believed current".
20. `session_majority_ack_uniform_reliable_broadcast.rs` is the only session module with no
    tagged property block; and session URB's `URB1/URB4 [always]` tags carry no pointer to the
    synchrony and reachability conditions they depend on, while the sibling documents the
    permanent-split failure those conditions imply.

## Test gaps (consolidated)

- No recovery test broadcasts new work after restarting (would catch defect 1).
- `crash_on_next_write` is exercised only in `recon-sim`'s own suite — never against
  `logged_link` or `logged_uniform_reliable_broadcast`, whose write-before-indicate claims it
  exists to check. `the_log_is_durable_before_the_announcement_even_across_a_crash` crashes on a
  timing window that may not straddle the append.
- No session_* test uses `suspend` (defect 3 is unexercised).
- No session-RB test crashes and restarts a recipient (defect 7).
- Flooding consensus: the `Flood::Decided(_) => {}` discard arm (DECIDED from an accused sender
  at a still-undecided process) is unreachable by the current suite — dropping the `p ∈ correct`
  guard would likely still pass; and future-round proposal buffering is exercised only
  incidentally by crash cascades.
- The PFD resumed-node accusation (defect 5) is unpinned: the suspension tests never check the
  suspended node's own view.
- `Sim::deliver_session_events()` is an opt-in nothing compile-checks: a driver that forgets it
  silently disables the entire bridge/resend path of the session layers.

## Verified clean

Every transcription matches its quoted pseudocode (verified against the indexed book text for
Algorithms 2.1, 2.3, 2.5, 3.1–3.5, 3.8, 5.1), apart from the elisions noted above. The classic
transcription bug — an `upon exists …` guard not re-evaluated when one of its inputs changes —
is avoided everywhere: both URBs re-check on the message and crash paths, session URB also on
the scope path, flooding consensus on the message and crash paths. Ack bookkeeping, delivery
guard order, and the deliberate omission of durable `ack` in logged URB (recovery re-broadcasts
pending; acks re-accumulate) are sound and non-vacuously tested. The simulator's fair-loss model
satisfies fair-loss, finite duplication, and no creation; synchronous mode enforces its bound
and zeroes the fault knobs at delivery time; session suffix loss is a correct per-direction
prefix-keep in send order; `FaultyStore` suppresses all writes after the fatal one and the
discard-effects-on-death path keeps write-before-indicate honest under torn writes; crash is
genuine amnesia with storage surviving, and the init/recovery branch matches the book's. Typed
composition drops, reorders, and duplicates nothing; mis-wiring is a compile error.
