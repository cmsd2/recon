# Transcriptions and implementations

A standing distinction, and a rule that follows from it.

## Two kinds of protocol in this repository

**A transcription** renders an algorithm from Cachin, Guerraoui & Rodrigues faithfully enough that
the code can be read against the page. Its purpose is to establish that the algorithm is
understood and that its stated guarantees hold under simulation. Everything built so far is one.

**An implementation** is something that could run. Its purpose is to hold those guarantees while
consuming resources bounded by something other than how long it has been running.

The two are not in conflict, but they are not the same artifact, and the difference has to be
stated per module rather than assumed. The book is explicit that it omits the difference — of the
lazy probabilistic broadcast it says "garbage collection of the stored message copies is omitted
in the pseudo code for simplicity", and the uniform reliable broadcast's `ack` array is described
as indexed "by all possible messages". A faithful transcription inherits those omissions. That is
correct of the transcription and disqualifying of an implementation.

## The rule

**State must be bounded by membership, by a window, or by a configured capacity — never by the
number of messages the protocol has handled.** A collection that only ever grows is a defect in an
implementation, whatever it is in a transcription.

The same applies to *work*, which is the failure mode that is easier to miss: a periodic task
whose cost is proportional to everything ever sent is unbounded even if each individual item is
small.

## Where this repository actually stands

Every protocol above the failure detector violates the rule.

| Protocol | State | Bounded by |
|---|---|---|
| `perfect_failure_detector` | `last_heard`, `detected`, `peers` | **membership** ✅ |
| `best_effort_broadcast` | `peers` (plus its child) | **membership** ✅ |
| `stubborn_link` | `sent` | messages ever sent ❌ |
| `perfect_link` | `delivered` | messages ever received ❌ |
| `reliable_broadcast` | `delivered` | messages ever delivered ❌ |
| `uniform_reliable_broadcast` | `pending` (with payloads), `ack`, `delivered` | messages ever seen ❌ |

The stubborn link is the worst of them, and not only in space. It has a `Stop` command and
**nothing ever calls it** — the perfect link never stops retransmission, because Algorithm 2.2
never does. So `sent` grows for ever and every entry is re-sent on every tick. Measured over 500ms
with a 10ms interval, on a network with no loss where every message arrived on its first attempt:

```
 10 messages ->     510 sends on the wire,   10 outstanding retransmissions
 20 messages ->   1 020 sends on the wire,   20 outstanding retransmissions
 40 messages ->   2 040 sends on the wire,   40 outstanding retransmissions
 80 messages ->   4 080 sends on the wire,   80 outstanding retransmissions
```

Fifty-one transmissions per message, none of them needed, and the rate grows with every message
ever sent. In a simulator this is a slow test. In a running system it is a link that degrades
until it stops working.

## The mechanisms that fix each

**Stop retransmitting what has arrived.** The perfect link knows when a message was delivered —
it deduplicates on exactly that. An acknowledgement returning to the sender retires the
transmission, which is what `sl::Cmd::Stop` exists for. This alone converts the stubborn link from
unbounded to bounded-by-in-flight, and is the single highest-value fix in the repository.

**A send window.** Cap in-flight transmissions and refuse or queue beyond it, so a fast producer
cannot make the link unbounded regardless of acknowledgements.

**A deduplication window instead of a set.** Per sender, track the highest contiguous sequence
delivered plus a bounded set of out-of-order arrivals above it — what TCP does, and what the
archived `lpb.rs` was already reaching for with its `next_seq` map. This bounds `delivered` at
`membership × window`.

**Stability-based collection.** A message is *stable* once every correct process has it, and
uniform reliable broadcast already computes exactly that predicate: `correct ⊆ ack[m]` is a
stability test. Once a message is stable and delivered, `pending` and `ack` can be dropped for it
immediately. This is the one place where the transcription is already one line from the
implementation.

## Bounding changes the guarantee, and the notation already says how

A deduplication window is not free. "No message is delivered more than once" becomes "no message
is delivered more than once **within the retention window**" — a message arriving after its entry
has been evicted is delivered again.

That is a scoped guarantee in the sense of [`scope-annotated-modules.md`](scope-annotated-modules.md),
and unlike the timing assumption of uniform reliable broadcast it satisfies Definition 2a: a
window's boundary is **observable by the module that owns it**. A protocol knows when it evicts.
It can therefore react, report, and be tested against the boundary — all the things a synchrony
assumption cannot support.

So windowing, not synchrony, is the likely genuine second consumer for the `Scope` associated
type. That decision stays deferred, but it now has a plausible claimant.

## How this should be enforced

Not by review. The other invariants of this project are mechanical checks because the failures are
silent at runtime, and this one is silent for weeks.

- **Every module states its space bound** in its documentation, in the same place its departures
  from the book are recorded: bounded by membership, by a window, or unbounded and therefore a
  transcription.
- **Every implementation has a test that runs a growing number of messages and asserts its state
  does not grow with them.** That is the mechanical form of this rule, and it is the same shape as
  the non-vacuity guards in `method.rs`: assert the property that would silently stop holding.
- **A transcription is labelled, not fixed.** Converting one is a change with a proposal, because
  bounding usually weakens a guarantee to a scope, and that belongs in a specification rather than
  in a commit that was meant to be a cleanup.
