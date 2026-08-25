## Why

Reliable broadcast guarantees agreement only among *correct* processes: if a process delivers a
message and then crashes, the survivors may never deliver it, and they are permanently
inconsistent with a process that acted on it before dying. Where the delivery had an external
effect — a reply sent, a record written — that divergence is not recoverable by anything above.

Uniform reliable broadcast closes it. Its agreement property quantifies over *any* process,
correct or faulty: if a message is delivered by anyone at all, every correct process eventually
delivers it too. This is the last rung of the broadcast chapter and the strongest guarantee
obtainable before consensus.

## What Changes

- **Uniform reliable broadcast**, Cachin, Guerraoui & Rodrigues Module 3.3 and Algorithm 3.4
  ("All-Ack"), which delivers only once every process believed correct has acknowledged seeing the
  message.
- **The first protocol that owns two children.** It uses best-effort broadcast *and* the perfect
  failure detector, which are peers rather than a stack.
- **The first typed multiplexing on the wire.** Two children both send, so this layer's message
  type must distinguish a broadcast payload from a heartbeat. It is worth noting what this is not:
  the previous attempt multiplexed with `format!("{}/upb", key)` and a string-keyed registry, and
  a typo became a silently undelivered message. Here it is an enum, checked at compile time, and it
  appears at the first layer that actually needs it rather than as infrastructure built in advance.
- **The first delivery condition that is not an event.** Algorithm 3.4's last clause fires on a
  predicate over state — *some pending message is now acknowledged by every correct process* —
  rather than on an arriving message or a timer. It must be re-evaluated whenever the state it
  reads changes.

Not in this change: the majority-ack variant of Algorithm 3.5, logged or fail-recovery broadcast,
consensus, and anything to do with transport.

## Capabilities

### New Capabilities

- `broadcast/uniform-reliable-broadcast`: Broadcast in which a message delivered by *any* process,
  including one that immediately crashes, is eventually delivered by every correct process.

### Modified Capabilities

None.

## Impact

- **Code.** One new module in `recon-protocols`. No change to `recon-core` or `recon-sim` is
  anticipated; if one proves necessary that is worth recording, since the last change needed the
  simulator and this one is expected not to.
- **A timing assumption is inherited, and stays prose.** URB depends on the failure detector, which
  is correct only while the network is synchronous. Its specification will say so — as the book
  does when it labels an algorithm fail-stop rather than fail-silent — and it will *not* be
  expressed with the scope annotation of `docs/scope-annotated-modules.md`. Under that document's
  own well-formedness condition it cannot be: URB has no way to observe synchrony breaking, since
  the failure would reach it as the detector making a mistake, indistinguishable from the detector
  being right. An assumption a module depends on but cannot detect is not a scope.
- **The macro question comes due.** Two children mean two composition helpers. If their bodies read
  as duplicates of one another, that is the second instance the decision recorded in the
  reliable-broadcast notes was waiting for.
