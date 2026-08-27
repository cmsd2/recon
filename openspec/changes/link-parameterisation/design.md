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

**Timers are already out of the way.** An earlier draft of this design was written when
`Protocol::Timer` still existed, so a layer's timer type wrapped its children's and making the link
a parameter made every layer above it generic in the link's timer type too. That was the largest
single cost in the change. The `opaque-timers` change removed `Protocol::Timer` and landed first, so
the cost is gone: a timer is an opaque handle the driver issued, no layer's timer type mentions any
other's, and parameterising a link propagates nothing but the link.

## Goals / Non-Goals

**Goals:**

- One implementation per algorithm, composed over whichever link is supplied.
- The requirement a layer places on the layer beneath is stated once and checked by the compiler.
- Today's spellings keep working: `BestEffortBroadcast<P>` still means the stack it means now.
- A link written outside this project can carry these protocols, up to consensus.

**Non-Goals:**

- Changing any protocol's guarantees. Every guarantee in the removed `session_*` capabilities
  survives; only their duplicate implementations go.
- Changing `Effect` or the shape of composition in `recon-core` beyond stating what a parent
  requires of a child. Timers are settled and not revisited here.
- Transport. Constraint 5 is untouched; this makes the seam ready for it, no more.
- Parameterising over anything but the layer directly beneath. A layer still knows its own child's
  port and nothing further down.

## Decisions

### The port is a trait with associated request and indication types, not a fixed pair

A trait `Link` with `type Cmd` and `type Ind`, implemented by each link, with layers above bounded
on it. Not an alias for one concrete `Cmd`/`Ind` pair, because that cannot admit two links whose
indication vocabularies differ.

*Alternative considered — pin the types exactly (`Protocol<Cmd = pl::Cmd<P>, Ind = pl::Ind<P>>`).*
Simpler, and it is what the exploratory work did first: a `Link<P>` supertrait with a blanket impl,
still on the branch as this is written. It works for the perfect link and fails for the session
link, whose `Ind` has three variants, so it cannot reach the duplication this change exists to
remove. Rejected — and the existing trait is therefore replaced rather than extended, which task 1.1
now says.

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

**Amendment, from building it.** The bound turned out to be the wrong half of this to lean on.
`ScopedLink` is where a layer *declares* it needs boundaries, and the compile-fail test confirms the
declaration is enforced. But the code that acts on a boundary — uniform reliable broadcast's resend
on re-establishment — cannot itself be bounded on `ScopedLink`: it is called from the arm that
handles the child's indications, which lives in the `Link` impl, so requiring the tighter bound
there would require it of every link. What makes the resend safe is the port's own guarantee
instead: `Link::classify` returns a boundary only for a link that reports one, so over a perfect
link the path is unreachable rather than merely unused. The declaration is checked by the compiler;
the reachability is a property of the port. Task 3.2 was reworded to say so.

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

### Timers cost this change nothing, because `opaque-timers` landed first

An earlier draft of this section planned to give each layer's `Timer` a type parameter with a
default, mirroring the layer itself, and accepted the resulting noise as the price of the change.
That is no longer necessary. A timer is named by an opaque handle, so a parameterised layer's
declaration mentions its child once — as the link parameter — and nowhere else.

Recorded rather than deleted because the sequencing was the decision that mattered: parameterising
consensus was attempted before the timer change and abandoned, and succeeded on the first attempt
afterwards. A change that is hard for a reason unrelated to its subject is worth stopping to fix
first.

## Risks / Trade-offs

- **Signatures grow noisy at the top of the stack.** Consensus multiplexes two children, so its
  message type gains a parameter and its serde bounds follow. → Defaults keep call sites clean and
  the noise stays in declarations. This was much worse before `opaque-timers`, when the timer types
  gained parameters too; that half is gone.

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

1. Replace the pinned-type `Link` trait now on the branch with the port proper — associated request
   and indication types — and add its scoped extension; implement both for the existing links.
   Nothing else changes, and the suite passes untouched.
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
