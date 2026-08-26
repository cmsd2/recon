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

**A write is durable when it returns, and everything rests on that.** The alternative — return
early and let durability catch up — was tried and deleted. A protocol is a synchronous state
machine: once its handler returns, the sends it emitted are in the driver's hands and there is no
later point at which the driver could wait for a write on their behalf. The return of the write
call is the only synchronisation point there is. Anything weaker lets a process be seen to have
promised something it has no record of, which is the bug this whole layer exists to prevent.

So a write blocks, as `fsync` does. That is a cost in the driver, not a violation of constraint 2:
the protocol still performs no IO of its own, still reads no clock, and is still a deterministic
function of its inputs. Storage is the same kind of thing as `cx.now()` and `cx.rng()` — state the
driver supplies so it can be made virtual and seeded.

*Alternative considered:* a marker in the effect stream recording the position of each write, with
the driver holding everything emitted after it until the write lands. It gives the same guarantee
and buys nothing: the driver waits either way, and it costs an effect variant, a held-effect queue,
and a rule a driver can silently get wrong.

**Reading is why the effect could not stay.** An effect is one-way. A read whose answer arrived as
a later event would work — `SetTimer` is already a request answered by `on_timer` — but it would end
recovery-inside-the-handler, and that is load-bearing. Given that reads must be synchronous, having
writes remain effects would leave two mechanisms for one concern; so the write moves too. The move
is a rename at the six call sites and buys nothing there. It buys `append` and `read_from`.

*Alternative considered:* keep `Effect::Store` and add only `append`, with no reads. Rejected — the
whole point of appending is that the state is too large to hand over whole, and a protocol that
cannot read cannot recover such a state.

**The ordering rule stops being a rule.** It currently works because `Effect::Store` sits in the
effect stream and the driver holds everything after it — an obligation a driver can fail to meet
while every test still passes. With writes durable on return there is nothing to order: anything a
handler emits after a write is emitted after the write has landed, because the call did not return
until it had.

**The interrupted-write fault has to be armed.** That is the price. Previously a crash during a
write happened by timing; now there is no window, so a test says `crash_on_next_write(node)` and
the simulator kills the process inside its next write. Whether that write landed is drawn from the
seed, later writes in the same handler do not land at all, and everything the dead handler went on
to emit is discarded — a crash loses volatile state, so nothing decided on the strength of that
write can escape. The recovering process has no way to tell which case it is in, which is exactly
the fault that finds bugs in fail-recovery code.

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
- **A blocking write is a real cost in a real driver**, and a protocol that writes on every message
  pays it on every message. → True, and the right place to feel it. The alternative hid the cost
  without removing it. Batching belongs in the driver, where it can see more than one protocol.
- **The interrupted-write fault is now opt-in**, so a suite that never arms it never exercises the
  one case where a write does not land. → The simulation specification requires the arming
  mechanism and both outcomes across seeds; the core and simulator suites assert them.
- **"One append per message" is asserted from protocol internals** and a refactor could falsify it
  quietly. → The trace distinguishes rewriting from appending, so the claim is checked against the
  trace and not against a field.
- **This supersedes an interface archived hours earlier**, and a reader may reasonably wonder
  whether the next one will last longer. → The proposal says so plainly, and the honest answer is
  that `Effect::Store` lasted exactly as long as it had one kind of consumer.
- **Converting two protocols at once risks a mistake in the interface being found twice.** → The
  interface and the simulator land first, exercised by a protocol written for the core suite, before
  either real protocol is touched.

## Notes

Three questions this change was meant to answer, answered:

- **Was one interface for reading, writing and appending the right consolidation?** Yes. The two
  halves are not symmetric — `Meta` is replaced, `Entry` accumulates — but they are the same
  concern and a protocol needs both in the same handler. Splitting them into two handles would have
  meant two lifetimes on `Cx` and two mappers on every composition, for nothing.
- **Was keeping the uninhabited-type check worth two associated types?** Yes, and it cost less than
  expected: thirteen protocols declare both `Infallible` in two lines and are otherwise untouched,
  and a write in one of them is a build error rather than a review note. The awkwardness is that
  `NoStore` — what a child is handed — has to name `Infallible` twice as well.
- **Does deferring a scoped child store still look right?** Yes. Nothing composed so far has a
  storing child, and the two shapes a scoped view could take — a lens over the parent's metadata, a
  namespaced sequence — differ enough that guessing now would likely pick the wrong one. Constraint
  4 in the letter: two or three real consumers before the abstraction.

One thing found rather than designed: the fault must stay armed across handlers that write nothing.
Arming it consumed the next *handler* at first, which the startup handler ate, and the test that
should have caught a lost write passed instead. It is armed until a write actually happens.
