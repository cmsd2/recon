# Contract audit — defects and fixes

The audit was taken on 2026-08-26 against the working tree containing the synchronous-store
(`log-storage`) change. Scope: every module in `recon-protocols` read against its quoted
Cachin–Guerraoui–Rodrigues pseudocode and stated properties; `recon-core` and `recon-sim` read
against the invariants in `docs/conditional-guarantees.md` and `docs/scope-annotated-modules.md`.

**All twenty defects are addressed, and every test gap is closed but one.** What follows is the
record: what was wrong, what was done, and — where a fix was documentation rather than mechanism
— why. Each entry names the test that now holds the fix in place; every one of those was checked
to fail with its fix reverted, so none of them is decorative.

Two things are deliberately *not* fixed and are recorded as such under **Left open**.

## Critical — a stated guarantee could actually break

### 1. Logged URB reused `BroadcastId`s after recovery — **fixed**

`seq` was volatile, so a recovered process re-minted `BroadcastId { origin: me, seq: 1 }` for
something new. Replay then made `pending[(me,1)]` last-write-wins across two distinct payloads,
acks for the old counted toward the new, the delivered-by-id guard let at most one through, and
different processes could log-deliver *different payloads under the same id*. No-creation,
validity and uniform agreement all failed, in the one module whose model expects a recovered
process to keep participating.

`on_recovery` now recomputes `seq` as the greatest over replayed records originating here. That
is sound because every own broadcast appends its `Pending` record in the handler that emits it, so
a torn write discards that handler's sends and no broadcast escapes without a record. The module
documents the counter as part of the id-keyed departure that created the obligation.

Test: `a_recovered_process_broadcasting_something_new_does_not_reuse_an_identifier`. It broadcasts
*new* work after restarting, which no recovery test did — they all resumed pre-crash broadcasts.

### 2. Logged link had the same volatile-`seq` hazard — **fixed**

A restarted sender re-minted `MsgId (me, 1), (me, 2), …`, and a recipient whose durable log
already held those ids discarded the distinct new payloads for ever while the sender's stubborn
link retransmitted them for ever. Unlike defect 1 the sender cannot recompute: its log holds
messages *received*.

The send counter is now the module's `Meta` — one small value, rewritten before the message it
names goes out. `type Meta` changed from `()` to `u64`, and `on_init` writes the counter at zero,
which is also what makes the init/recovery branch real. The write-cost note now says one metadata
rewrite per send and why that is still linear.

Test: `a_restarted_sender_does_not_reuse_an_identifier_the_recipient_has_logged`.

`perfect_link.rs` has the same shape and is legitimately out of scope — crash-stop, where a
crashed process is not correct — but it now says so, and names the mismatched-pair hazard (a
durable set keyed by a volatile counter) that makes the logged link different.

### 3. Suspension silently absorbed loss inside a live session — **fixed**

`suspend` did not end sessions, but a `Deliver` to a suspended node was dropped while its session
stayed up, so a message was lost with no `SessionEnded` ever raised. The cardinal sin, committed
by the simulator.

A suspension is now a *stall*: timers, deliveries and scope events that come due while a process
is away are **held** in `deferred` and re-dispatched, in the order they came due, by `resume`.
Nothing addressed to a suspended process is dropped. `partition` models unreachability and `crash`
models failure; `suspend` models being descheduled, and each is now a distinct thing.

Tests: `a_delivery_due_during_a_suspension_arrives_on_resume`,
`a_suspension_holds_deliveries_outside_a_session_too`,
`a_crash_discards_deliveries_held_during_a_suspension`, and — the case the audit named —
`a_stalled_process_misses_nothing_because_its_session_never_ended`, the first `session_*` test to
spend the `suspend` knob. With the fix reverted, B never delivers, permanently.

### 4. `restart()` conflated crash-recovery with suspend-resume — **fixed**

Resuming a suspended node re-ran `on_init`/`on_recovery` over volatile state that was never lost.

Split: `resume(node)` for a suspension — releases what was held, runs no startup branch — and
`restart(node)` for a crash. Each panics on the other's process rather than doing the wrong thing
quietly.

Tests: `resuming_is_not_restarting`, `restarting_a_suspended_process_is_a_mistake_that_says_so`,
`resuming_a_crashed_process_is_a_mistake_that_says_so`.

## Significant — contract text or model wrong, damage latent

### 5. A resumed node permanently accuses every live peer — **documented and pinned, not fixed**

Deferral (defect 3) gives a stalled process back everything it missed, but not the clock. A
process descheduled for longer than `timeout` comes back holding a due tick and a `last_heard`
older than the timeout for every peer, and the measurement is *not wrong* — it genuinely heard
nothing for that long. So it accuses everyone, permanently.

This is the timing assumption failing rather than the implementation, in the place it is easiest
to forget: Δ bounds the network, and a synchronous system bounds process scheduling too. The
module's stall analysis covered only being accused; it now covers both sides and says why.

The mechanism fix — a detector that notices far more than `period` elapsed between ticks and
treats that round as unmeasured — is a real departure from the page and is left as a proposal.
See **Left open**.

Tests: `a_stalled_process_accuses_its_peers_when_it_comes_back` and
`a_stall_shorter_than_the_timeout_costs_the_stalled_process_nothing`, which together make it a
statement about the timeout rather than about suspension.

### 6. Session URB's docstring quoted the wrong algorithm — **fixed**

The quoted added clause was the filtered, broadcast variant; the code implements an
unconditional, directed resend, and the module's own inline comment proves the filtered one
deadlocks. The quote now matches the code, and names both departures from the obvious version —
unconditional, and directed — with pointers to where each is argued.

### 7. `RB2 [always]` over-claimed in session reliable broadcast — **fixed**

`delivered` is volatile, so a restarted recipient re-delivers a late relay; the repo's own
Corollary 7.2 forbids `[always]` for that configuration. Retagged `[incarnation]`, with the
corollary's argument spelled out.

Test: `rb_no_duplication_is_scoped_to_an_incarnation`, which finds a seed where a relay kept
across the recipient's crash arrives after it has forgotten, and delivers twice.

### 8. Logged URB's `Pending` append was mis-ordered — **fixed**

The re-broadcast was emitted before the `Record::Pending` append, and an early `return` skipped the
deferred append entirely. The invariant held by accident of control flow, and under an eager sink
this process's ack could escape with no durable pending. The append now happens at the point of
insertion, in the handler's own text.

### 9. Stale-session deliveries arrived after `SessionEnded` — **fixed**

`end_session` kept a random prefix of in-flight messages at their future times while firing the
ending at `now`, so old-session deliveries could land after `Ended`, after the successor's
`Established`, even behind new-session traffic — with no epoch on the wire to tell them apart.

The kept prefix is now flushed at the instant of the ending, in send order, before the ending is
announced. That exposed a second problem it had been hiding: a flushed delivery lets the peer
reply, and the reply re-established the session *in the same instant*, so a layer saw
`Established(2)` before `Ended(1)`. A session may therefore no longer re-open in the instant it
closed — reconnection takes time, and the sweep opens the next one.

Test: `nothing_is_delivered_on_a_session_after_it_has_ended`.

### 10. `docs/conditional-guarantees.md` §scope-composition was stale — **fixed**

The section still described the mechanism as unbuilt and had scopes composing upward through
per-layer mappers. What was actually built routes the concrete `SessionEvent` *downward* with no
mapper, because a scope is a fact about the transport underneath the whole stack and every layer
that cares cares about the same fact — a mapper would rename something nobody renames. The section
is rewritten from the code, and says what *Bridges / Propagates* turned into instead.

The handler was also renamed `on_scope_end` → `on_scope_event` (with `Event::ScopeEvent` and
`Scheduled::ScopeEvent`): `Established` is a scope *beginning*, it is the only event a resend can
succeed on, and a handler named for endings that receives beginnings is a lie the compiler cannot
catch. Definition 2 in `docs/scope-annotated-modules.md` gains the note that abutting intervals do
not imply one event marks both boundaries, and that BRIDGE's stitching obligation is discharged at
the successor's beginning.

## Minor — all fixed

11. `crash`'s doc comment was fused onto `crash_on_next_write`, leaving `crash` undocumented. Both
    now have their own, and `crash`'s says it discards what a suspension was holding.
12. `TraceEvent::WriteLost` fired even when the doomed write landed. Renamed `DiedWriting`, and
    `Trace::writes_lost` → `deaths_in_writes`; the doc now says the outcome is deliberately not
    recorded, because not knowing until you read your storage back is the whole content of the
    fault.
13. `ensure_session` created a `(me, me)` session on a self-send, and the `[(a,b),(b,a)]` loop
    announced `Established` twice per node. `a == b` now returns early.
14. `stubborn_broadcast::Cmd::Stop` was unusable: `Broadcast` minted `N` internal `SendId`s and
    returned none of them, so the space claim ("bounded by membership and by what is outstanding")
    rested on an escape hatch nobody could reach. The caller now names the broadcast —
    `Cmd::Broadcast { id: BroadcastId, msg }` — and one name retires the whole fan-out.
    Test: `stopping_a_broadcast_retires_every_transmission_it_became`.
15. Re-`Send` on a live `SendId` silently replaced the prior transmission, whose SL1 lapsed with
    no indication. The precondition is stated on `Cmd::Send` and `debug_assert`ed.
16. The stubborn link's pseudocode quote elided the book's `⟨ sl, Init ⟩` clause while reading as
    complete. Restored, and the lazy-arming departure is now in a departures list rather than only
    at `arm()`.
17. The PFD's heartbeats bypass the book's `pl` layer — a fourth departure missing from the
    "three departures" list. Added, with why it is equivalent under the synchronous config and
    what it costs outside one.
18. `logged_link::Log` was a `BTreeSet` of pairs, so deduplication was a linear scan: `O(n²)` work
    against a cost note claiming linear. Now a `BTreeMap` keyed by identifier. The remaining `O(n)`
    per arrival is the clone of the set the *indication* carries, which is the book's interface —
    documented, and it goes away with the same change that bounds the record.
19. `session_link::epochs` was never cleared on `Ended`, so `epoch()` reported a dead session as
    current — the opposite of what the layer exists to say.
    Test: `a_dead_session_is_not_reported_as_current`.
20. `session_majority_ack_uniform_reliable_broadcast` had no tagged property block; session URB's
    `[always]` tags named no conditions. Both now carry blocks stating what each property is scoped
    by, and session URB names the permanent-split failure its two conditions imply when both fail
    at once.

## Test gaps — closed

- **New work after recovery.** `a_recovered_process_broadcasting_something_new_does_not_reuse_an_identifier`
  (defect 1). Resuming exercises replay; this exercises what replay must restore.
- **`crash_on_next_write` against the logged protocols.** It was spent only in the simulator's own
  suite, never against the modules whose write-before-indicate claims it exists to check.
  Now `dying_inside_the_write_never_leaves_a_promise_without_a_record` (logged link) and
  `dying_inside_the_write_never_log_delivers_without_a_record` (logged URB); both assert nothing
  was announced from the doomed handler, both require the seed to produce landed *and* lost across
  the range, and both check the run still finishes correctly either way.
- **A `session_*` test that suspends.** `a_stalled_process_misses_nothing_because_its_session_never_ended`.
- **A session-RB test that crashes a recipient.** `rb_no_duplication_is_scoped_to_an_incarnation`.
- **The PFD's accusing side.** `a_stalled_process_accuses_its_peers_when_it_comes_back`.
- **Flooding consensus's `Flood::Decided(_)` discard arm.** `a_decided_from_an_accused_sender_is_discarded`
  builds a real DECIDED from a run, hands it to a process that has accused its sender, and asserts
  it is thrown away — with the same message at a process that accused nobody deciding, so the
  assertion cannot pass on a protocol that ignored DECIDED entirely. Dropping the `p ∈ correct`
  guard now fails it.
- **`Sim::deliver_session_events()` is an opt-in nothing compile-checks.**
  `forgetting_deliver_session_events_silently_disables_the_whole_bridge` pins what forgetting
  costs: sessions open and close, and no layer is told. The method's documentation says so too.
  Still opt-in — see **Left open**.

## Left open

Two items, both because the fix is a design change rather than a correction:

- **A detector that discounts its own stall** (defect 5). Noticing that far more than `period`
  elapsed between consecutive ticks and treating that round as unmeasured is a real technique and
  a real departure from Algorithm 2.5. It changes what PFD2 promises, so it is a change with a
  proposal. The behaviour is documented and pinned meanwhile.
- **Making the session bridge impossible to forget.** `deliver_session_events` cannot be made
  automatic on stable Rust: the `P::Scope: From<SessionEvent>` bound cannot be tested at
  construction, and a protocol with an uninhabited `Scope` legitimately never calls it. A
  constructor that installs the mapper (`Sim::with_sessions`) would make the right thing the
  obvious thing without making the wrong thing impossible. Not done; the hazard is pinned.

One lesser gap remains unclosed: flooding consensus's future-round proposal buffering is still
exercised only incidentally, by crash cascades, rather than by a test that aims at it.

## Verified clean

Unchanged from the audit, and re-checked after these fixes. Every transcription matches its quoted
pseudocode (verified against the indexed book text for Algorithms 2.1, 2.3, 2.5, 3.1–3.5, 3.8,
5.1). The classic transcription bug — an `upon exists …` guard not re-evaluated when one of its
inputs changes — is avoided everywhere: both URBs re-check on the message and crash paths, session
URB also on the scope path, flooding consensus on the message and crash paths. Ack bookkeeping,
delivery guard order, and the deliberate omission of durable `ack` in logged URB are sound and
non-vacuously tested. The simulator's fair-loss model satisfies fair-loss, finite duplication and
no creation; synchronous mode enforces its bound and zeroes the fault knobs at delivery time;
session suffix loss is a correct per-direction prefix-keep in send order; `FaultyStore` suppresses
all writes after the fatal one and the discard-effects-on-death path keeps write-before-indicate
honest under torn writes; crash is genuine amnesia with storage surviving. Typed composition drops,
reorders and duplicates nothing; mis-wiring is a compile error.
