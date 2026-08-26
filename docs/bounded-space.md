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
| `flooding_consensus` | `receivedfrom`, `proposals`, per round | **membership and rounds** ✅ |
| `majority_ack_uniform_reliable_broadcast` | `pending`, `ack`, `delivered` | messages ever seen ❌ |
| `stubborn_broadcast` | `peers`, and what is outstanding | **membership** ✅ |
| `logged_link` | `delivered` — **in stable storage** | messages ever log-delivered ❌❌ |
| `logged_uniform_reliable_broadcast` | `pending`, `delivered` — **in stable storage** | messages ever seen ❌❌ |

The last two carry a double mark, and the second one is the point. Growth in memory costs a
process its resident size; growth on disk costs that *and* a write proportional to the whole value
on every change, since the durable state is stored in full rather than as a delta. A protocol that
log-delivers `n` messages writes `O(n²)` bytes over its life.

**The mechanism that would fix them is a delivered *cursor* rather than a delivered *set*.** The
indication would say "everything up to sequence `n` is durable" instead of carrying a set, which is
bounded by membership. It costs per-sender ordering, which the link beneath does not currently
promise, and it weakens the guarantee to a scope in the way §"Bounding changes the guarantee"
describes. It is a change with a proposal, not a cleanup.

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

## Which abstractions would actually be deployed

The rule above matters most for code that ships, so it is worth being explicit about which abstractions
would. Some of these abstractions exist to show how a guarantee is *constructed* from nothing; in a real
deployment the transport has already constructed it.

| Abstraction | In a deployment |
|---|---|
| fair-loss link | the network itself, or the simulator standing in for it |
| **stubborn link** | **academic.** TCP and QUIC retransmit already |
| **perfect link** | **academic as written.** Within a TCP session you have PL1–PL3 for free |
| best-effort broadcast | **deployed.** Fan-out over links, state bounded by membership |
| reliable broadcast | **deployed**, once `delivered` is windowed |
| uniform reliable broadcast | **deployed**, once `pending`, `ack` and `delivered` are collected |
| perfect failure detector | **deployed only where synchrony is real.** Otherwise ◇P |

The stubborn and perfect links are how you obtain a perfect link when you have nothing but a lossy
datagram service. That is the simulator's situation and not production's. They stay — everything
above them needs a perfect link, and the simulator offers only fair-loss — but they are not what
would ship, and their unbounded state is therefore an academic defect rather than an operational
one.

**What replaces them is not a better stubborn link.** It is a *session link*: TCP or QUIC supplying
PL1–PL3 within a session, plus an event saying the session changed and an unknown suffix may have
been lost. That is the design already recorded in
[`conditional-guarantees.md`](conditional-guarantees.md), and this is the second argument for it —
the first being honesty about reconnection, and this one being that the deployable link needs
*less* state than the academic one, not more. Within a session TCP does not duplicate, so there is
nothing to deduplicate; across a session boundary the epoch event says so explicitly.

So the fix for the worst offender in the table above is not to bound it. It is to not ship it.

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
