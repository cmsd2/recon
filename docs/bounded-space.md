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

Every protocol above the failure detector violated the rule when this was written. The gossip pair
and then the leader-driven family were built the other way round, so the table is now mixed rather
than uniformly bad.

The logged consensus modules deserve a precise statement, because they were wrong when first
written and the way they were wrong is this document's subject. Their **state** was always bounded:
one rewritten record each. Their **work** was not. Over a stubborn broadcast every `READ` and `WRITE`
comes round again each interval, and Algorithm 5.9 answers every delivery — each answer a fresh
stubborn transmission that is itself retransmitted for ever. Measured in steady state with nothing
faulty, `logged_epoch_consensus` sent 12.6k, 28.6k, 44.6k, 60.6k, 76.6k messages in successive
400 ms windows; the volatile Paxos beside it sent 1,400 in each. The send *rate* grew linearly in
time, the total quadratically, and the seven modules of that change carried no test that would have
noticed.

Two guards fix it, both departures recorded in their modules: a follower answers `READ` once and
`WRITE` once — the reply is stubborn, so a second carries no information — and `logged_epoch_change`
refuses each distinct announcement once per peer. Every module claiming a membership bound now has a
test that its send rate is flat across a run several times longer than the work takes, and the two
logged consensus ones fail it without the guards.

What the stubborn children hold outstanding is still never retired, because nothing calls `Stop`.
For the consensus that is one reply per follower per epoch and the epoch's own three announcements
— bounded, and retired by `Abort`. For `logged_epoch_change`, which has no ending, it is one
transmission per *distinct* announcement or refusal, which grows with leadership changes rather
than with time. That is the residual warning in the table.

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
| `fair_loss_link` | nothing at all | **nothing** ✅ |
| `probabilistic_broadcast` | `delivered` | **a retention window** ✅ |
| `lazy_probabilistic_broadcast` | `stored`, `pending`, `next` | **a retention window, and membership** ✅ |
| `eventual_leader_detector` | `suspected`, `peers` (plus its child) | **membership** ✅ |
| `epoch_change` | `trusted`, `lastts`, `ts`, `peers` | **membership** ✅ |
| `epoch_consensus` | `states`, `accepted`, one `(valts, val)` | **membership** ✅ |
| `leader_driven_consensus` | one epoch consensus, replaced not accumulated | **membership** ✅ |
| `logged_epoch_change` | `(startts, start)` — **in stable storage**, one value rewritten; `nacked`, per peer | **membership** for state and for work; the stubborn children's outstanding set grows with distinct announcements, not with time ⚠️ |
| `logged_epoch_consensus` | `(valts, val)` and `epochdecision` — **in stable storage**, one value rewritten | **membership** for state and for work; the stubborn children hold one reply per follower per epoch ✅ |
| `logged_leader_driven_consensus` | `(ets, ℓ, decision)` and both children's records — **in stable storage**, one value rewritten | **membership** for state and for work; inherits `logged_epoch_change`'s ⚠️ |

The last two carry a double mark for their size, not for what they cost to write. Both had the
second problem and no longer do: the durable state was one blob rewritten on every change, so a
protocol that log-delivered `n` messages wrote `O(n²)` bytes over its life. Both now append one
entry per message and rewrite nothing, and their suites assert it from the trace.

That was the *work* half. The *state* half remains: the record itself still grows with every
message, in memory and on disk alike. The storage interface splits the two cases so the distinction
is visible in a protocol's types — a `Meta` value for something small that is rewritten (an epoch, a
promise), an `Entry` sequence for anything that accumulates. Choosing the first for something that
accumulates is the mistake, and it is now a choice a reader can see.

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

## The first two bounded from the start

`probabilistic_broadcast` and `lazy_probabilistic_broadcast` are the first modules above the failure
detector written as implementations rather than transcriptions. Two things about how that went are
worth recording, because this document has until now described converting a transcription rather
than avoiding one.

**The book gave no help, and said so.** Page 100: "garbage collection of the stored message copies
is omitted in the pseudo code for simplicity." So the retention mechanism was a design decision with
no page to check it against — which is exactly the position this document warns about, met head-on
rather than deferred.

**The shape matters more than the bound.** The previous implementation of these algorithms
*did* collect garbage, and it was still the one defect in that code which survived scrutiny: it
expired by wall-clock age and rebuilt the whole delivered-set on every event, so receiving one
message cost time linear in everything ever received. A bound that is reclaimed by a periodic sweep
is not the same as a bounded implementation. These evict on insert, and the test that distinguishes
the two watches the collection size after every single insert — under eviction it rises to the cap
and never moves, where a sweep sawtooths.

**Bounding weakened a guarantee, and the specification says so.** `PB2` reads `[window]`: a message
re-arriving after its identifier has been evicted is delivered again. That is the scoped guarantee
this document predicts, written down in the module's own table and pinned by a test rather than
discovered later.

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
| **the gossip pair** | **the real-world set**, as of 2026-09. Bounded by a window; the second obligation — session links, and a message count checked against the work — is the next change |
| eventual leader detector, Ω | would be, over ◇P. Derived here from P, which is stronger than the book's ◇P and never re-trusts a recovered process |
| epoch-change and epoch consensus | **the book's stepping stones** to Paxos; bounded by membership and tested where the detector is wrong, kept faithful rather than tuned |
| leader-driven consensus | **not in the real-world set** — single-instance Paxos is what the book builds, and multi-Paxos is what ships. Kept as the page has it |
| the logged consensus | **not in the real-world set**: it runs over stubborn links, which are academic here. Bounded in work now, and left as the book has it |

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
