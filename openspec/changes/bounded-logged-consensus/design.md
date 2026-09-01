## Context

`logged_epoch_consensus` and `logged_epoch_change` sit on `stubborn_broadcast` and `stubborn_link`,
which retransmit forever because nothing calls `Stop`. Both modules answer what they are delivered,
and what they are delivered arrives again every interval. Each answer is a new `SendId`, so the
stubborn link's outstanding set grows by `N` every interval, forever, and so does the send rate.

## Goals / Non-Goals

Goals: bound the work of both modules by membership; test that bound everywhere it is claimed;
remove the composition boilerplate. Non-goals: `Stop` (the outstanding set is still unbounded across
*epochs* in `logged_epoch_change`, which has no ending); `◇P`; departing from the book further than
the one line each fix needs.

## Decisions

### The reply is stubborn, so one is enough

A follower that has answered `READ` with `STATE` has a stubborn transmission carrying that answer
until this instance is aborted. A second `STATE` to a redelivered `READ` carries the same content on
a second transmission. So: `state_sent` and `accept_sent` flags, one reply each. This holds across
the leader crashing and recovering — the recovered leader re-proposes, and the follower's *original*
reply, still being retransmitted, is what reaches it. That is precisely what stubborn links are for.

*Alternative — reuse one `SendId` per (peer, kind) and let `Send` replace.* Also bounded, and it
would let the reply's content change. Nothing here needs it to: `STATE` reports what was accepted
at the time of the read, and a follower that accepts afterwards is reporting to a leader who has
already moved past reading.

### A NACK once per distinct announcement per peer

`nacked: BTreeMap<NodeId, u64>` — the highest timestamp refused per peer. A repeat has a timestamp
no greater than the last, and is not refused again. Bounded by membership. The book's `sl` is
stubborn too, so the one NACK sent reaches the leader; refusing again adds a transmission and no
information.

### `Child<P>` in `recon-core`

Every composite module has the same eleven lines per child: take the inbox out, borrow the child,
call `with_child_consuming`, drain, put the inbox back. Sixteen copies. `Child<P>` owns the protocol
and its inbox; `run` performs the composition call and returns the filled inbox by value, so the
parent is free to handle indications — including by calling `run` again — and `reclaim` puts the
allocation back. `run_durable` is the same over a `Slot`. Two lines replace eleven, and the
allocation-stable property `alloc_probe` checks is preserved because the `Vec` is reused.

*Alternative — a macro.* Rejected for the reason constraint 4 gives: the boilerplate is small
enough that a struct removes it without hiding control flow.

### `Timing`, `slot!`, `Sim::at`, `tests/common`

Mechanical. Three positional `Duration`s become a struct with named fields, so a swapped argument
is a compile error; `slot!(Parent, field)` writes the two projections every slot writes the same
way; `Sim::at(n)` panics with the node's name instead of `unwrap`'s line number; the eight suites
added last change stop each redefining `A..E`, `ALL` and the three timing functions.

## Risks / Trade-offs

- **Rewriting sixteen modules with no behavioural change is a lot of diff for a suite to guard.**
  → The migration is mechanical and the suites are the guard; every one passes before and after.
- **One reply per epoch means a follower whose reply was lost… is not a case.** The reply is
  stubborn; loss is what it exists for. The case that would break this is a leader whose `states`
  is volatile *and* whose link to the follower forgets the transmission — that is the follower
  crashing, and a crashed follower's `state_sent` is volatile too, so it answers again on recovery.

## Migration Plan

Core helpers first, then the two fixes with their growth tests, then the migration module by
module with the suite run after each, then the remaining tests, then docs.
