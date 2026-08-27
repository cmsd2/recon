## Why

`docs/conditional-guarantees.md` states the seam this project is built around: *layers above the
link may depend on its `Cmd` and `Ind` types and nothing else, so a session-aware or logged
implementation can be swapped through*. That seam is documentation. In code, every layer names a
concrete child — `BestEffortBroadcast` holds a `PerfectLink<P>`, `FloodingConsensus` holds a
`BestEffortBroadcast<Flood<P>>` — so nothing enforces the rule and nothing can use it.

The cost is already paid and visible. Needing a different link once, for the session model,
produced four forked modules — `session_best_effort_broadcast`, `session_reliable_broadcast`,
`session_uniform_reliable_broadcast`, `session_majority_ack_uniform_reliable_broadcast` — around
2,000 lines whose algorithms are the originals with the link swapped underneath. Each fork carries
its own copy of the quoted pseudocode, its own departures list and its own guards, and the
2026-08 audit found the consequence: session URB's quoted clause had gone stale against its own
code while the sibling's had not. The next link — a logged link above the broadcasts, an
application's own transport, a QUIC adapter when constraint 5 is discharged — would fork them
again.

## What Changes

- Introduce a **link port**: the request and indication types a layer above the link may depend on,
  named once, belonging to neither the layer above nor the implementation below.
- Every layer that composes over a link takes it as a **type parameter** rather than naming a
  concrete type. Defaults preserve today's spelling, so `BestEffortBroadcast<P>` still means the
  stack it means now.
- The port describes **both** kinds of link. A session link reports scope boundaries a perfect link
  never raises, so the port admits a link that reports them and a link that cannot, without the
  layer above having to be written twice.
- **BREAKING (internal):** the four `session_*` broadcast modules are removed. Their behaviour
  becomes the base modules composed over a session link, so `session-reliable-broadcast` is
  `reliable-broadcast` with a different type argument. Their guarantees survive; their duplicate
  implementations do not.
- An application may compose this library's protocols over a link this library never wrote,
  including consensus, with neither side edited.

Explicitly **not** in this change: anything about timers. The `opaque-timers` change landed first
and removed `Protocol::Timer` altogether, so a layer's timer type no longer mentions its child's.
Parameterising a link therefore propagates nothing upward but the link itself — which is what made
this change tractable, and why an earlier draft argued a timer ripple that no longer exists.

## Capabilities

### New Capabilities

- `links/link-port`: the request and indication vocabulary a layer above the link depends on, and
  the rule that it may depend on nothing else. This is the seam made checkable rather than
  documented.

### Modified Capabilities

- `protocol-core`: composition is stated over a declared port rather than over a named child, and a
  parent's requirement of its child becomes part of its interface.
- `links/perfect-link`: satisfies the link port, and says so.
- `links/session-link`: satisfies the link port in its scope-reporting form.
- `broadcast/best-effort-broadcast`: composes over any link satisfying the port, and absorbs what
  `session-best-effort-broadcast` specified.
- `broadcast/reliable-broadcast`: composes over any broadcast satisfying the port, and absorbs what
  `session-reliable-broadcast` specified.
- `broadcast/uniform-reliable-broadcast`: as above, absorbing `session-uniform-reliable-broadcast`.
- `broadcast/majority-ack-uniform-reliable-broadcast`: as above, absorbing
  `session-majority-ack-uniform-reliable-broadcast`.
- `broadcast/session-best-effort-broadcast`: removed; its requirements move to the base capability.
- `broadcast/session-reliable-broadcast`: removed; likewise.
- `broadcast/session-uniform-reliable-broadcast`: removed; likewise.
- `broadcast/session-majority-ack-uniform-reliable-broadcast`: removed; likewise.
- `consensus/flooding-consensus`: composes over any broadcast satisfying the port.

## Impact

- `crates/recon-core`: the port's definition, and the statement of what composition requires.
- `crates/recon-protocols`: every composing layer gains a type parameter; four modules are deleted
  and their tests moved onto the base modules with a session link as the type argument.
- `crates/recon-sim`: unaffected in behaviour; test stacks name their link explicitly.
- Public API: the type parameters are additive with defaults, so existing spellings compile
  unchanged. Naming a `session_*` broadcast type does not.
- `README.md`: the protocol table loses four rows and gains a column, or says instead that the
  session variants are the base protocols over a session link.
- `docs/conditional-guarantees.md`: the seam stops being aspirational and the section saying so is
  dated by this change.
