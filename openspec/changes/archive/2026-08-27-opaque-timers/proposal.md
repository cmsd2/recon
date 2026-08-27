## Why

`Protocol::Timer` makes a timer's *type* encode its position in the composition. Each parent
re-wraps its children's, so a timer in the consensus stack is
`Timer::Broadcast(beb::Timer::Link(pl::Timer::Stubborn(sl::Retransmit)))`, and inserting a layer
rewraps every timer beneath it.

Three things follow, and the third is the one that blocks work.

Fourteen of the sixteen protocols carry a `Timer` enum, a `type Timer`, a mapper argument at every
composition call and an unwrapping handler — **purely to relay something they did not register**.
Only the stubborn link and the failure detector ever set a timer at all.

A timer's identity is its type, so a layer cannot tell an expiry it is still waiting on from one it
has superseded. Both protocols that own a timer track it with `armed: bool`, which can say that
something is pending but not *which* something. Cancellation is inexpressible for the same reason.

And a layer's timer type appears in the type of every layer above it, so making any child a
parameter propagates that parameter all the way up: `Timer<beb::Timer<L::Timer>>`, dragging serde
bounds and phantom parameters behind it. That is what makes the parameterisation described in
`openspec/changes/link-parameterisation` (a separate change, in another branch) materially larger
than it needs to be.

## What Changes

- **BREAKING:** `Protocol::Timer` is removed. Registering a timer returns an opaque handle instead,
  and the same handle comes back when it fires.
- `Cx::set_timer` takes only a duration and **returns** the handle. The driver owns one source of
  identities for a run, so an identity is unique across a whole run rather than within one layer.
- `Effect`, `Event`, `TraceEvent` and `Trace` each lose a type parameter, and `Effect::map` loses
  its timer mapper: a timer has nothing in it belonging to one layer, so there is nothing to
  re-wrap.
- A layer that composes children hands an expiry to each of them; a layer that registered a timer
  acts on an expiry only if it registered *that* one, and ignores the rest. This obligation is
  stated in the spec and pinned by a test rather than left to be rediscovered.
- The two protocols that own timers hold the handle rather than a flag, so a superseded expiry is
  recognised and ignored instead of acted upon.
- The test helper gains a sibling that takes a caller-owned identity source. The existing helper
  starts identities at zero on every call, which is right for one protocol driven alone and wrong
  for a stack — two layers would each be handed identity zero and each would accept the other's
  expiry.

Explicitly **not** in this change: nothing about which layer composes over which. No protocol's
guarantees change. This is the mechanism a timer is named by, and nothing else.

## Capabilities

### Modified Capabilities

- `protocol-core`: a timer is named by an opaque handle rather than by a per-protocol type; a
  protocol acts only on an expiry it registered; composition no longer translates timers.
- `simulation`: the driver owns the identity source and records the handle in the trace, so a
  claim about which timer fired can be checked against the trace rather than against protocol
  internals.

## Impact

- `crates/recon-core`: `Protocol` loses an associated type; `Cx::set_timer` changes shape; `Effect`,
  `Event` and the `step` helpers change. This is the whole of the breaking surface.
- `crates/recon-protocols`: fourteen modules lose timer plumbing; two change how they hold what
  they registered. No algorithm changes.
- `crates/recon-sim`: the run owns the identity source; `Scheduled` and the trace carry handles.
- Public API: **breaking**. Any protocol implemented outside this repository declares
  `type Timer` and takes a token in `on_timer`; both must change. Nothing else about implementing
  `Protocol` moves.
- `docs/conditional-guarantees.md` and `docs/scope-annotated-modules.md` describe scopes and ports
  rather than timers, and are not dated by this change. `CLAUDE.md`'s composition conventions
  mention re-wrapping child effects and are.
