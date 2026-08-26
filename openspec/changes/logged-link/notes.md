# What the exercise showed

Notes written after the fact, per task 6.4.

## The design survived three algorithms; the gap it had was found by a question, not a test

The effect-plus-ordering-rule shape held up. `Effect::Store(D)`, a `Durable` associated type, and a
driver obligation that everything emitted after a store waits for the write — three protocols
needed nothing more, and the placement of `store` in each reads against the page.

What it was missing was found in review rather than by any test: **the constructor is the common
prefix of the startup branch, not one side of it.** The book has `⟨ Init ⟩` and `⟨ Recovery ⟩` with
exactly one firing, and `Init` performs `store(∅)`. That write must not happen on recovery — it
would overwrite exactly what was being retrieved — and a constructor cannot host it anyway, because
it runs in both cases and emits nothing.

The first version of this change quietly dropped the initial store and called that a
simplification. It was not: it was deleting the only instance of first-start-only logic rather than
providing a home for it, and it would have failed at Ω, whose epochs must be monotonic across
incarnations. A process that writes an initial epoch and crashes before anything else must recover
that epoch; without the initial write, storage is empty and the restart is indistinguishable from a
first start.

`on_init` closes it, and the audit that followed found the same workaround in four more places.

## The audit found `Start` was `Init` all along

Five modules documented `⟨ Init ⟩` as "not a separate event". They were not describing a design
decision; they were describing the absence of somewhere to put startup effects.

The clearest case is the perfect failure detector. Module 2.6's `Init` arms the first timer and
sends the first heartbeats — effects — so it had been rendered as a `Cmd::Start` command. It is now
`on_init`, and that protocol has **no commands at all**: `type Cmd = Infallible`, checked by the
compiler. There is nothing to ask a failure detector for; detection begins when the process does.
Three protocols that carried a `Start` only to forward it lost it too, and every
`command(n, Cmd::Start)` disappeared from the suites.

The general lesson: a workaround written once is a decision, and written five times is a missing
feature. Grepping for the phrase that documented it was what surfaced that.

## What the crash-during-write fault found

Less than expected, and the reason is worth recording. Both logged protocols are idempotent by
construction — every branch is guarded by "is this already in the log?" — so losing a write is
indistinguishable from never having received the message, and the retransmission beneath fixes both.
`the_log_is_durable_before_the_announcement_even_across_a_crash` runs forty seeds and every one ends
with the message log-delivered exactly once, by one route or the other.

That is a real result rather than a weak test: it says these algorithms do not depend on knowing
whether their last write landed, which is the property the fault exists to check. An algorithm that
*did* depend on it — Ω incrementing an epoch, say — would fail here, and the fault will earn its
keep then.

What the fault did find is the **coalescing** behaviour: a store issued while an earlier one is
still outstanding replaces it, so a run log-delivers more messages than it completes writes. Safe
only because the durable value is the whole log rather than a delta — a later value always contains
an earlier one. A protocol storing deltas could not be composed with this simulator, and the module
says so.

## Two consumers were enough to shape the primitive; a third would have changed it

The proposal warned that the second consumer proves less than it looks, because the book's logged
protocols do not stack: each keeps its own log over stubborn links or stubborn broadcast. That held
— logged uniform reliable broadcast exercises the primitive independently rather than composing on
the logged link's indication.

What it did prove is worth having. Two independent users disagreeing about anything would have
shown up, and they did not: both want one durable value written in full, both want a recovery event
that can emit effects, and both are content with the ordering rule as a driver obligation.

The third consumer would change things, and the shape of the change is already visible. **A parent
cannot compose a storing child** — `with_child` takes a durable mapper and `absurd` is the only
function anyone can write, because a parent's durable state contains its own fields as well as its
child's. Every protocol here has children that store nothing, so it costs nothing; the first one
that does not will need a real design — a slot per participant, a parent that drives its child's
writes explicitly, or a path-indexed store. Left undesigned deliberately.

## What is worse now

The bounded-space position. These are the first protocols whose unbounded state is **on disk**, and
the cost is worse than in memory: the durable value is stored in full, so a protocol that
log-delivers `n` messages writes `O(n²)` bytes over its life. `docs/bounded-space.md` names the
mechanism that would fix it — a delivered *cursor* rather than a delivered *set* — and why it is a
change with a proposal rather than a cleanup: it costs per-sender ordering and weakens the
guarantee to a scope.
