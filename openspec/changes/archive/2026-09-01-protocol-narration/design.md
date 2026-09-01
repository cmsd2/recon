## Context

Two constraints govern, and they pull in opposite directions.

**Narration must be checkable.** A statement written beside a state change is a second statement
about it, and this repository has already been bitten by exactly that: a docstring kept quoting the
deadlocking variant of a resend clause its own inline comment said a test had replaced. A note that
nothing can verify decays the same way, faster, because nobody reads it until the night it matters.

**The algorithms must stay readable.** They are meant to read as obviously correct against the
quoted pseudocode, and a narration scheme that puts a line of instrumentation between every two lines
of algorithm defeats the point of the transcription.

The measurement that resolves most of the tension: **290 of the 298 mentions of a context in this
repository are `ProtoCx<'_, Self>`**, which absorbs a type parameter without naming it. The eight
that do not are in `cx.rs`, `child.rs` and one core test. So a typed note channel is affordable —
the churn is in the core's own signatures, not in the algorithms.

## Goals / Non-Goals

Goals: a typed note channel through `Cx`; notes in the trace so they can be checked against it;
rendering to `tracing` as events are recorded; and one module narrated, with the agreement tests
that make the claim real.

Non-goals: narrating the other twenty-five modules; a log format; anything a protocol can read back.

## Decisions

### A note goes through `Cx` and is recorded eagerly, like a write — not deferred, like an effect

Two candidate homes. `Effect::Note(N)` makes narration one more thing a protocol can do, and it would
compose exactly as a timer does — `MapSink` forwards it untouched, no mapper. But an effect is
*deferred*: it describes something the driver will do. A note describes something that has **already
happened**, at a point inside the handler, and the repository's own rule is that ordering inside a
handler is established in the handler's own text rather than by relying on the driver's buffering.
Storage is not an effect for that reason, and narration is the same shape of thing.

So `Cx` gains a note sink beside the store, and `cx.note(..)` records at the moment of the call.
This also keeps `Effect` at two parameters and leaves `EffectSink` alone.

### The vocabulary is `Protocol::Note`, and `Infallible` means silent

Exactly the `Scope` pattern, for exactly the reason given there: an uninhabited type has no values,
so a protocol declaring `Infallible` cannot construct a note, and `cx.note(..)` is uncallable rather
than merely unusual. A `compile_fail` doctest holds it.

Written out at each implementation rather than defaulted, because associated type defaults are not
stable — the same as `Scope`, which every protocol here spells out.

Rejected: **a closed enum in `recon-core`.** Core's vocabulary today is `NodeId`, `Time`, `TimerId`,
`WriteKind`, `Position`, `SessionEvent` — all driver and infrastructure. `Note::EpochStarted` would
be the first algorithm concept to live there, and the shape of the vocabulary would then be decided
by whoever edits core rather than by the module whose decision it is.

Rejected: **`&'static str` plus typed fields**, the way `tracing` models an event. Most readable at
the call site, and the one option where a typo compiles: `"epoch.enterd"` is a silently missing
event, which is the string-keyed-composition anti-pattern in miniature — the same failure mode as
`format!("{}/upb", key)`, a typo becoming an absence rather than a compile error.

Rejected: **erasure to `Box<dyn Note>`.** It would need no type parameter anywhere, which is
genuinely attractive. Against it: `TraceEvent` derives `Clone`, `PartialEq` and `Eq`, none of which
survive a trait object without hand-rolling all three; it allocates per note, and there is an
allocation probe in the suite; and type erasure at a boundary is a named anti-pattern here, even
though the specific harm recorded for it — repeated serialisation — does not apply to a value that
never crosses a wire.

### Composition passes a note through untouched — and a note's vocabulary belongs to the run

**Changed during implementation, after building the alternative and discarding it.** The plan was
`with_child` requiring `P::Note: From<C::Note>`, with `From<T> for T` covering the common case and a
hand-written `impl From<Infallible> for Note` for silent children. That was built. It does not
survive contact with the parameterised layers.

`BestEffortBroadcast<L>`, `FloodingConsensus<L>`, `EventualLeaderDetector<D>` and the rest are
generic over a *port*, so `L::Note` is unknown at the layer. Each then needed a `where` clause —
`where Infallible: From<L::Note>`, or `where L::Note: From<G::Note>` for a layer with two
port-generic children — to express something no layer actually wanted to do. Seven layers acquired a
bound relating type parameters, in service of a conversion that was the identity everywhere it was
instantiated.

What replaced it is simpler and is the repository's own existing answer to the same question.
**A note's vocabulary belongs to the run, not to a layer, exactly as a `TimerId` does.** `with_child`
hands the child the parent's own note sink, the way it already hands down the source of timer
identities. There is no mapper, no conversion, and no bound: a composed stack narrates in one
vocabulary, and `link.rs`'s and `detector.rs`'s port traits say so — `Protocol<Note = Note>` — which
is what "layers above the link name a port" already means for everything else about them.

The cost is that every module in `recon-protocols` declares `type Note = Note;`, including the
twenty-five that narrate nothing. That is a line each, and it buys the deletion of `MapNotes`, of the
`From` relation, and of seven `where` clauses.

### One shared `Note` enum in `recon-protocols`

Where the decision points are. It starts with `epoch_change`'s and grows as modules are narrated. A
reader who wants to know every decision this stack claims to make reads one file — which is a
property worth having, and the reason not to scatter a note type per module.

### A note is worth having only where it says what the trace cannot

The sharpest thing found while designing this, and it decides what gets narrated. `cx.note(Started
{ ts, leader })` beside `cx.indicate(Ind::StartEpoch { ts, leader })` is pure duplication: the trace
already holds the indication, the note adds nothing, and the two can now disagree.

What the trace cannot hold is a decision that produced no effect. `epoch_change` is full of them —
a `NewEpoch` refused because the sender is not trusted or its timestamp is not newer; a `NACK`
ignored because the candidate has already passed it; a report sent to a new leader *because*
`started_by` disagreed with it. Those are the lines a reader needs and the trace is silent on. It is
also why `epoch_change` is the module chosen to prove the mechanism.

### Checking a note against the trace, and being honest about how far that goes

Three strengths, and the tests should say which is which rather than implying one:

- **A note about an action taken** is checkable against the effect: an epoch narrated as started is
  an epoch indicated as started, with the same timestamp and leader. Strong.
- **A note about an action refused** is checkable negatively: if a process says it refused a
  `NewEpoch`, no `StartEpoch` follows from that delivery. Real, weaker.
- **Coverage** — that every traversal of a narrated decision point produces exactly one note — is
  checkable by counting notes against the deliveries that reach the point. This is the property that
  catches narration silently falling out of a clause, which is the decay mode the whole design is
  built against.

### `tracing` is a renderer over the trace, and lives only in `recon-sim`

The sim emits each event to `tracing` as it records it, with `node` and the virtual `at` as fields
rather than as an enclosing span — a span per event buys nothing, and the fields are what a
subscriber prints. Live rather than a walk over a finished trace, because a run that hangs is one
worth reading. Off unless asked for, like `enable_codec_check` and `deliver_session_events`, and
carried as a function pointer so the simulator does not acquire `Debug` bounds it otherwise lacks —
the same shape as the codec check.

Two switches, not one: `record_notes` puts what protocols say into the trace, and `enable_tracing`
renders the trace. Either is useful alone — rendering without recording shows the run as the trace
already knew it.

### Where a note sits among the effects of its own handler

A note is recorded when `cx.note` is called; a handler's effects are recorded when the driver
performs them, which is after the handler returns. So a note precedes every effect of the handler
that narrated it, whatever their order in the text. That reads correctly — decision, then write,
then send — and it is what the trace event's documentation says, rather than something a reader has
to infer.

The dependency lands in `recon-sim` alone. `recon-core` and `recon-protocols` gain nothing, so the
protocol layer still reaches for nothing ambient in the literal sense as well as the substantive one.

### Constraint 2, stated rather than smuggled

`tracing`'s dispatcher is thread-local, and a protocol emitting through it would be reaching for
something ambient. That is why protocols do not: they call `cx.note`, and the only code that touches
a `tracing` dispatcher is the simulator, which is a driver and is allowed to.

Worth stating plainly even so: **narration is output-only.** Nothing a protocol can observe reveals
whether anything is listening, so no protocol behaviour can depend on it, and a run is reproducible
whether or not anybody watched. That is the substance constraint 2 protects, and the spec requires it
rather than leaving it to be inferred.

## Risks / Trade-offs

- **`Trace` and `TraceEvent` gain a parameter**, affecting ten and five mentions across the suites,
  including two written last week. → Mechanical, and the compiler finds every one.
- **`Cx::new` gains an argument** rather than offering a note-free variant, so that a driver
  choosing not to listen passes `NoNotes` explicitly. → Deliberate: an `Option` inside would let the
  two paths differ, and the protocol's code being *identical* either way is what makes narrating
  unable to change the run.
- **A note that duplicates an effect is worse than no note**, because it can drift. → The rule above,
  and the coverage test. If a note only restates an indication, delete the note.
- **Narrating one module proves the mechanism and not the value.** → Accepted deliberately, and the
  scope was chosen with that in mind. `epoch_change` is at least the module whose silences caused a
  real diagnosis, so the demonstration is not synthetic.
- **The shared enum grows.** → Expected. It is a list of the decisions this stack claims to make, and
  a long one is informative rather than embarrassing.

## Migration Plan

1. `Protocol::Note`, `Cx`'s note channel, and the `Infallible` default — the core alone, with
   nothing narrating yet.
2. `TraceEvent::Said`, and the determinism test that narration changes nothing.
3. The `tracing` renderer in `recon-sim`, off by default.
4. The `Note` enum and `epoch_change` narrating its silences.
5. The agreement tests, at all three strengths.
6. Docs: `README.md`'s roadmap item `F`, the crate description, the suite table and counts.

## Open Questions

- **Whether the coverage test can be made general rather than per-module.** "Every traversal of a
  narrated decision point produces exactly one note" is currently a hand-written count per module.
  Something in the shape of `tests/method.rs` — a test about how narration is tested — might state it
  once. Not attempted here; noted because the second module narrated is where it will start to hurt.
- **Whether spans should nest per layer.** A note says which *decision* it is but not which layer said
  it, and today that is unambiguous only because the variants are specific. If two layers ever narrate
  the same decision, attribution needs a span per layer, which composition does not currently provide.
