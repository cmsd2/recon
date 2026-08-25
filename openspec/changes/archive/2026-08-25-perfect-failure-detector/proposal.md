## Why

Uniform reliable broadcast is the next rung, and its stronger guarantee — a message delivered by
*any* process, correct or faulty, is eventually delivered by every correct process — cannot be
built from what exists. Cachin, Guerraoui & Rodrigues give two implementations: Algorithm 3.4
requires a perfect failure detector, Algorithm 3.5 assumes a correct majority instead. The
decision was to build the failure detector, which is rung 3 of the ladder and was skipped when
the first three rungs were built out of order.

There is a second reason to build it now rather than take the majority shortcut. A perfect
failure detector is where a *timing assumption* enters the system for the first time, and the
book is explicit that P exists to encapsulate the assumptions of a synchronous system. Everything
built so far is asynchronous by construction. Making that boundary explicit — and making the
simulator able to express it — is worth more than one rung.

## What Changes

- **A perfect failure detector**, Module 2.6 and Algorithm 2.5 ("Exclude on Timeout"): heartbeats
  on a period, and a process not heard from within a bounded delay is declared crashed,
  permanently.
- **A synchronous mode in the simulator.** This is the part that does not exist and is the larger
  half of the change. P's strong accuracy — *if a process is detected, it has crashed* — is
  unachievable against the current network model: messages are lost at a configurable rate and
  latency is drawn from a range, so a live process whose heartbeats are unlucky is indistinguishable
  from a dead one. Perfect detection requires a known upper bound on delivery, which the simulator
  must be able to offer and to enforce.
- **The first delta against an existing capability.** `simulation` gains a requirement; its
  existing ones are unchanged.
- **A protocol whose output is not a message.** Every rung so far delivers payloads. This one
  produces only indications about other processes, which exercises a shape the `Protocol` trait
  has not yet been asked for.

Not in this change: uniform reliable broadcast itself, which follows immediately; the eventually
perfect failure detector of the partially synchronous model; and anything to do with transport.

## Capabilities

### New Capabilities

- `failure-detection/perfect-failure-detector`: Detects crashed processes with no false positives
  and no permanent omissions, under a synchronous system's timing assumptions.

### Modified Capabilities

- `simulation`: gains the ability to run with a bounded, enforced delivery delay and no loss, so
  that a timeout-based detector can be accurate. The existing fair-loss behaviour is unchanged and
  remains the default; this is an additional mode, not a replacement.

## Impact

- **Code.** A new module in `recon-protocols`, and a real addition to `recon-sim` — the first
  change to the simulator since it was built, and a signal worth noting: three protocols were not
  enough to settle what the network model needed.
- **A timing assumption enters the system.** Every guarantee so far holds in an asynchronous
  model. This one does not, and the boundary should be visible in the specification rather than
  buried in a configuration value. In the notation of `docs/scope-annotated-modules.md` this is a
  scope — the detector's properties hold only while the synchrony assumption does — though the
  `Scope` associated type remains unimplemented and this change does not add it.
- **Composition shape.** The detector is a peer of best-effort broadcast rather than a layer above
  it: uniform reliable broadcast will own both. That is the first protocol to own two children,
  and it is the case flagged in the previous change's notes as the one that would reopen the macro
  question.
