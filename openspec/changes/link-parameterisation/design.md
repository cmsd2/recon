## Context

See `proposal.md` — Why. Three constraints shape the approach.

**Composition is static, and must stay so.** Constraint 4 of `docs/postmortem.md` §5 says parents
own children as concrete typed fields and re-wrap their effects, and the first attempt's documented
anti-patterns are string-keyed layer composition and type erasure at every boundary. So the seam
must be a type parameter with a trait bound, resolved at compile time. A registry, a `dyn` object,
or anything resolved while running would be the failure this project exists to avoid repeating.

**The two links do not speak the same vocabulary.** `perfect_link::Ind` has one variant, `Deliver`.
`session_link::Ind` has three: `Deliver`, `SessionEnded`, `SessionEstablished`. This is exactly why
the four `session_*` forks exist and why a naive port pinning `Ind` to one type cannot admit both.
It is the central design problem of this change.

**`Protocol::Timer` stays as it is.** A layer's timer type today wraps its children's —
`Timer::Broadcast(beb::Timer::Link(pl::Timer::Stubborn(..)))` — so a timer's type encodes its
position in the composition. Making the link a parameter therefore makes every layer above it
generic in the link's timer type too. This is a decision taken outside this change and is the
largest single cost in it; see Decisions and Risks.

## Goals / Non-Goals

**Goals:**

- One implementation per algorithm, composed over whichever link is supplied.
- The requirement a layer places on the layer beneath is stated once and checked by the compiler.
- Today's spellings keep working: `BestEffortBroadcast<P>` still means the stack it means now.
- A link written outside this project can carry these protocols, up to consensus.

**Non-Goals:**

- Changing any protocol's guarantees. Every guarantee in the removed `session_*` capabilities
  survives; only their duplicate implementations go.
- Changing `Protocol::Timer`, `Effect`, or the shape of composition in `recon-core` beyond stating
  what a parent requires of a child.
- Transport. Constraint 5 is untouched; this makes the seam ready for it, no more.
- Parameterising over anything but the layer directly beneath. A layer still knows its own child's
  port and nothing further down.

## Decisions

### The port is a trait with associated request and indication types, not a fixed pair

A trait `Link` with `type Cmd` and `type Ind`, implemented by each link, with layers above bounded
on it. Not an alias for one concrete `Cmd`/`Ind` pair, because that cannot admit two links whose
indication vocabularies differ.

*Alternative considered — pin the types exactly (`Protocol<Cmd = pl::Cmd<P>, Ind = pl::Ind<P>>`).*
Simpler, and it is what the exploratory branch does today. It works for the perfect link and fails
for the session link, so it cannot reach the duplication this change exists to remove. Rejected.

*Alternative considered — make the session link's extra indications a separate port.* Two ports
means two bounds means, again, two implementations of every layer above. Rejected for the same
reason.

### Scope reporting is an optional capability of the port, expressed as a second trait

`Link` carries what every link can do. A link that reports scope boundaries additionally implements
`ScopedLink`. A layer indifferent to boundaries bounds on `Link` and composes over both. A layer
whose liveness depends on being told about a re-establishment — uniform reliable broadcast, and the
majority-ack variant — bounds on `ScopedLink`, and composing it over a link that cannot report is a
compile error rather than a protocol that waits forever.

This is the whole of how four forks collapse into four parameterised modules. It also states in the
type system what `docs/conditional-guarantees.md` states in prose: *a layer that cannot bridge must
propagate*, and a layer that repairs a scope ending needs a link that raises one.

*Alternative considered — one port whose `Ind` always includes the scope variants, with a
non-session link never emitting them.* Rejected: it obliges the perfect link to declare a scope it
cannot observe, which `docs/scope-annotated-modules.md` forbids by Definition 2a and Corollary 8.1.
A link reporting a boundary it has no means to know would be asserting something it cannot.

### Type parameters carry defaults

`BestEffortBroadcast<P, L = PerfectLink<P>>`, and likewise upward. Every existing call site,
including the whole test suite, compiles unchanged, so the change is additive at the API surface
and the diff stays readable. The `session_*` type names disappear and callers naming them must
change — that is the one breaking part, and it is internal.

### The removed `session_*` modules keep their tests

Each `session_*` test suite becomes a test of the base module with a session link as the type
argument. The suites are what pin the scoped guarantees — `RB4 [session]`, the resend on
establishment, the permanent-split analysis — and losing them would lose the evidence that the
collapse preserved behaviour. They move; they are not deleted.

### Timers thread through, and this change absorbs the cost

With `Protocol::Timer` unchanged, a parameterised layer's timer type mentions its child's:
`beb::Timer<L::Timer>` and so on up the stack. Each layer's `Timer` gains a type parameter with a
default, mirroring the layer itself.

*Alternative considered — identify timers by an opaque handle routed to whoever registered them.*
That removes the ripple entirely: no layer's timer type mentions any other's, and parameterising a
link stops propagating upward at all. It is the better design and it is a `recon-core` change with
its own proposal. This change is deliberately written to be independent of it: if that lands first,
the timer parameters in this change become unnecessary and can be deleted without touching anything
else here.

## Risks / Trade-offs

- **The timer ripple makes signatures noisy, worst at the top of the stack.** Consensus already
  multiplexes two children, so its message and timer types both gain parameters and its serde
  bounds follow. → Defaults keep call sites clean, and the noise is confined to declarations. If it
  proves worse than expected in practice, the opaque-handle change removes it wholesale; sequencing
  that change first would make this one materially smaller.

- **Collapsing four modules risks losing a documented departure.** Each fork carries its own quoted
  pseudocode and departures list, and the merged module must state both the plain and the scoped
  reading. → The audit's own rule applies: a module's quoted pseudocode and departures are updated
  in the commit that dates them. Reviewing the merged docstring against both originals is a task,
  not an afterthought.

- **A wider bound may admit a link that satisfies the port but not the protocol's assumptions.** A
  link that loses messages between correct processes satisfies `Link` and breaks best-effort
  validity. → The port cannot express "does not lose messages"; nothing in a type can. The specs
  state each guarantee as conditional on what the link supplied provides, and the foreign-link tests
  demonstrate the honest case rather than implying a guarantee the port cannot carry.

- **`ScopedLink` may turn out to be the wrong axis.** Storage is a second axis already (`logged_link`
  versus `perfect_link`), and a third — a link that reports peer incarnations — is anticipated in
  `docs/conditional-guarantees.md`. Adding a trait per axis multiplies bounds. → This change adds
  exactly one axis, for a distinction that already exists in the code as four forked modules. A
  third axis should be resisted until it, too, has forked something.

## Migration Plan

1. Introduce the port and its scoped extension; implement both for the existing links. Nothing else
   changes, and the suite passes untouched.
2. Parameterise upward one layer at a time, lowest first, each with a default preserving today's
   type. The suite passes after each step.
3. Move each `session_*` suite onto its base module with a session link, and check the merged
   module's docstring against both originals.
4. Delete the four `session_*` modules and their spec directories.
5. Add the foreign-link tests, including consensus.

Rollback is per-step: every step before 4 is additive, so reverting one commit restores the previous
state. After step 4 the `session_*` modules are gone from the tree and rollback means reverting the
deletion commit.

## Open Questions

- Where the port's definition should live: `recon-core` beside `Protocol`, or `recon-protocols`
  beside the links that satisfy it. It is a protocol-level vocabulary rather than a core mechanism,
  which argues for the latter, but `protocol-core`'s spec is where composition is specified. This
  does not change the specs, the approach, or the task breakdown.
- Whether `stubborn-broadcast` and the two logged modules should be parameterised in this change or
  left concrete. They compose over the stubborn link only, and no second implementation of that port
  exists, so parameterising them would be building an abstraction before its second consumer —
  which constraint 4 warns against. The tasks assume they are left alone.
