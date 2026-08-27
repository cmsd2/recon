## Context

See `proposal.md` — Why. Three things about the present shape matter to the approach.

**Only two protocols own a timer.** `stubborn_link` and `perfect_failure_detector` call
`set_timer`. The other fourteen have a `Timer` enum, a `type Timer`, a mapper at every composition
call and an unwrapping handler solely to relay. So the bulk of this change is deletion, and the risk
is concentrated in two files rather than sixteen.

**The driver only knows the top protocol.** `Sim` holds one protocol per node and calls its
handlers. It has no handle on the layers inside, so it cannot deliver an expiry directly to whoever
registered it — the re-wrapping is currently what routes an expiry downward. Removing the types
removes the routing, and something must replace it.

**Reproducibility is a hard constraint.** Constraint 3 of `docs/postmortem.md` §5: a failing run
must replay from its seed. Identities must therefore come from somewhere deterministic and
order-dependent, not from a global counter or anything shared across threads.

## Goals / Non-Goals

**Goals:**

- A timer's identity says nothing about where in a composition it was registered.
- A layer that registers no timer declares nothing about timers.
- A layer can tell an expiry it is waiting on from one it has superseded.
- Every existing guarantee, test and seed-reproducibility property survives unchanged.

**Non-Goals:**

- Cancellation. Handles make it expressible; nothing needs it yet, and adding an unused
  `cancel_timer` would be building for a consumer that does not exist.
- Changing which layer composes over which. Parameterisation is a separate change.
- Delivering an expiry only to its registrant. That is the better design and it needs routing the
  driver cannot do today; see Decisions.

## Decisions

### The handle is a newtype over a counter, and the driver owns the counter

`TimerId(u64)`, allocated by `Cx::set_timer` from a counter the driver lends to the context and
shares down the whole composition. Deterministic, ordered by registration, and unique to a run.

*Alternative considered — a process-global atomic counter.* Simplest to plumb, and it breaks
reproducibility: two runs in one test binary would allocate different identities, so a trace
comparison across runs would fail. Rejected outright; the seed is the whole point of the simulator.

*Alternative considered — a counter per protocol instance.* Would avoid threading anything through
`Cx`, but two layers of one composition would each start at zero and each would accept the other's
expiry. This is not hypothetical: it is exactly what a per-call counter in the test helper does, and
it is how the failure was found.

### An expiry is offered to every layer, and each recognises its own

The driver calls the top protocol; each composing layer passes the expiry to each of its children.
A layer that registered timers compares against what it holds; a layer that registered none passes
it on and does nothing else.

Depth is at most four here, so the cost is a handful of calls per expiry — cheaper than the
allocation the same event already causes.

*Alternative considered — route directly to the registrant.* What the actor spike does, and
strictly better: no layer sees another's expiry, so no layer can act on one. It requires the driver
to know which layer registered which identity, and in an owned-child composition the driver holds
only the outermost protocol. Building that map means either the context recording a path as a
registration passes outward — which reintroduces per-layer identity, the thing being removed — or a
registry, which is the documented anti-pattern. Rejected as materially larger, and recorded here
because it is where this design would go if composition changed.

*Consequence, accepted:* a layer that registers a timer and omits the comparison will act on
another layer's expiry. The specification states the obligation and a test pins it. This is a real
footgun that the typed design did not have, and it is the price of the type not carrying routing.

### A layer holds the handle it registered, not a flag

Both timer-owning protocols currently track `armed: bool`. They hold `Option<TimerId>` instead,
which is what makes the comparison possible and what makes a superseded expiry recognisable rather
than merely indistinguishable.

### `Effect::map` loses its timer mapper rather than mapping identity

A timer effect passes through composition untouched. Keeping a mapper that is always the identity
function would leave every call site supplying an argument that does nothing, and would leave the
type parameter it exists to serve.

### The test helper gains a sibling rather than changing shape

`step` keeps its signature and starts identities at zero; a new `step_with` takes a caller-owned
source. Ninety-odd call sites drive a single protocol and are unaffected; the handful driving a
composition by hand move to `step_with`. Changing `step` itself would touch every call site to serve
a few.

Tests that fire a timer must name the identity the protocol registered. They learn it from what the
protocol emitted rather than assuming a literal — which is what a test should always have done, and
was invisible while the type was the identity.

## Risks / Trade-offs

- **A layer written later forgets the comparison and acts on another layer's expiry.** → The spec
  states the obligation and a test pins it: a layer with a timer outstanding, given another's
  expiry, does nothing. The test is the guard; nothing in the type system is. Recorded honestly as
  the cost of the design rather than as a solved problem.

- **The change is broad and largely mechanical, so a scripted edit can silently delete something
  adjacent.** This already happened once while exploring: a regex matching an `on_timer` body ran to
  the next closing brace and removed the handler after it, in a one-line-body case. → Do the
  fourteen relaying modules by hand or with per-file review, and diff each file against its original
  before moving on. The suite caught that instance; it should not be relied on to catch the next.

- **`step` and `step_with` differing in a subtle way is itself a footgun.** A test using `step` on a
  composition is wrong in a way that may not fail. → `step`'s documentation states what it is for
  and what it cannot do. A stronger option — making `step` refuse a composed protocol — is not
  expressible.

- **The public `Protocol` trait breaks.** Any implementation outside this repository must change. →
  There are none; the crates are unpublished. Recorded because the change would be much more
  expensive later.

## Migration Plan

Adding the handle type is separable. Everything else is one step, and it is worth being exact about
why, because an earlier draft of this plan claimed otherwise and the claim did not survive contact
with the code.

`Cx::set_timer` exists only to emit `Effect::SetTimer`. Its signature therefore cannot change before
that variant does — there would be no token to put in the effect. And the moment the effect carries a
handle, the driver hands a handle to `on_timer`, so `Protocol::Timer` is unusable and every protocol
that declares one must change in the same breath. The trait member, the effect, the context method,
all sixteen protocols and the simulator's dispatch move together.

1. Add the handle type. Additive, revertible, nothing else moves.
2. Everything else, in one step: `set_timer` returns a handle; `Effect::SetTimer` carries it and
   `Effect` loses its type parameter; `Protocol::Timer` goes and `on_timer` takes the handle;
   `Event`, `TraceEvent` and `Trace` lose theirs; the two timer-owning protocols hold handles; the
   fourteen relaying layers are stripped; the simulator owns the identity source. **The workspace
   does not build until this step completes**, so it is verified at its end rather than part-way
   through.
3. The behaviours the handle makes possible — a superseded expiry ignored, identities distinct
   across a composition, reproducibility including the handles the trace names.
4. Add the caller-owned identity source for tests driving a composition by hand; move those tests.
5. Add the test that pins a layer ignoring another layer's expiry.

Rollback: step 1 is independently revertible. From step 2 the change is one unit, and reverting means
reverting the whole of it.

## Open Questions

- Whether the trace should record the *registration* of a timer as well as its firing. The specs do
  not require it and nothing needs it; it would make a stale-expiry claim checkable from the trace
  alone rather than from a protocol's own state. Deferrable without changing the approach.
