## Context

See `proposal.md` — Why. Three facts from the change immediately before this one shape it.

`Effect::Store(D)` is used ten times and every use is `cx.store(self.<whole_state>.clone())`. There
is no read at all: recovery works because the driver pushes the whole value.

The simulator relies on initialisation and recovery completing inside their handlers, in two places
— `Sim::new` runs `on_init` before the queue is ever drained, and `restart` runs the handler
synchronously before returning — and nothing states it. A protocol part-way through loading its
durable state that received a message would act on it believing it had recorded nothing.

`docs/bounded-space.md` measures the cost of the blob rewrite and names the fix.

## Goals / Non-Goals

**Goals:**

- One synchronous interface for reading, writing and appending, so a protocol can ask storage a
  question and use the answer in the same breath.
- Recovery that stays synchronous, so the invariant above survives without a recovering state.
- The two logged protocols converted to appending, so the interface is shaped by real use.

**Non-Goals:**

- Truncation or compaction. Nothing needs them, and a log that is never truncated is honest about
  what a transcription costs.
- Bounding these protocols' *memory*. That needs per-sender ordering from the link and is a
  different change.
- Giving a child its own store. Still forbidden, for a new reason; see below.
- Any new protocol.

## Decisions

**Synchronous is not durable, and everything rests on the distinction.** `append` and `set` return
before anything is on a disk; what they promise is that a later read *in the same incarnation* sees
them. Durability stays deferred and stays the driver's business, exactly as a write to a page cache
returns long before `fsync`. Without this separation a synchronous interface would either be a lie
or would have to block, and blocking is what constraint 2 exists to prevent.

With it, storage is the same kind of thing as `cx.now()` and `cx.rng()`: state the driver supplies
so it can be made virtual and seeded. A protocol still performs no IO and is still a deterministic
function of its inputs.

**Reading is why the effect could not stay.** An effect is one-way. A read whose answer arrived as
a later event would work — `SetTimer` is already a request answered by `on_timer` — but it would end
recovery-inside-the-handler, and that is load-bearing. Given that reads must be synchronous, having
writes remain effects would leave two mechanisms for one concern; so the write moves too. The move
is a rename at the six call sites and buys nothing there. It buys `append` and `read_from`.

*Alternative considered:* keep `Effect::Store` and add only `append`, with no reads. Rejected — the
whole point of appending is that the state is too large to hand over whole, and a protocol that
cannot read cannot recover such a state.

**The ordering rule stays positional, via a marker.** It currently works because `Effect::Store`
sits in the effect stream and the driver holds everything after it. A synchronous write mutates the
driver-held store *and* pushes a marker into that stream: the value travels by the handle, the
position by the marker. Same guarantee, and the write is still visible at the call site — which was
the original reason for choosing an effect over a snapshot method, and is worth keeping.

*Alternative considered:* hold everything emitted during an event in which any write occurred.
Simpler, and wrong: it would hold effects emitted *before* the write, which depend on nothing.

**The compile-time check is kept, and falls out.** A protocol declares a metadata type and an entry
type. One that keeps nothing declares them uninhabited, and then `set` and `append` take an argument
nobody can construct — so a write is a build error exactly as `Durable = Infallible` makes it one
today. Reads become vacuous rather than forbidden, which is harmless and simpler than gating them.

**A storing child is still forbidden, for a different reason.** Previously there was no mapping from
a child's durable value into a parent's. Now the problem is scoping: a parent and child sharing one
store would collide on the metadata slot and interleave in the log. Giving a child a scoped view is
a real design — a lens over the parent's metadata, a namespaced log — and nothing needs it. Recorded
rather than built, as before.

**Both protocols append their record and keep their index in memory.** They already mirror durable
state in memory and write through; that does not change and should not. What changes is that the
mirror is no longer written back in full. The metadata slot holds what is small and rewritten — a
position, a count — and the log holds what accumulates.

**Nothing-dispatched-during-startup becomes a stated requirement.** It is true today by accident.
Writing it into the simulation specification turns an arrangement that could be disturbed into a
property that can be checked, and it is the reason the rest of this design works.

## Risks / Trade-offs

- **A synchronous read is honest only while the record is mirrored in memory.** For a log larger
  than memory it is a real disk read and a real block, which is what constraint 2 forbids. → Stated
  as a bound in the module documentation rather than discovered at the first large log. Every
  consumer here holds its index in memory anyway.
- **The ordering marker is a driver obligation, not a type.** Same risk as before, and the same
  answer: the simulation specification states the observable form, including that effects emitted
  *before* a write are not held, and the suite asserts both directions.
- **"One append per message" is asserted from protocol internals** and a refactor could falsify it
  quietly. → The trace distinguishes rewriting from appending, so the claim is checked against the
  trace and not against a field.
- **This supersedes an interface archived hours earlier**, and a reader may reasonably wonder
  whether the next one will last longer. → The proposal says so plainly, and the honest answer is
  that `Effect::Store` lasted exactly as long as it had one kind of consumer.
- **Converting two protocols at once risks a mistake in the interface being found twice.** → The
  interface and the simulator land first, exercised by a protocol written for the core suite, before
  either real protocol is touched.
