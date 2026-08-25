## Why

Best-effort broadcast makes no promise when the sender crashes partway through: some processes
deliver, others do not, and they disagree permanently. `docs/conditional-guarantees.md` reads the
ladder as each rung bridging one more class of failure, and best-effort broadcast is the rung that
bridges nothing new — which is precisely why this one exists.

Reliable broadcast is also the milestone `docs/postmortem.md` §5.6 names as the point at which the
composition model is proven: five composed protocols, and the first whose guarantee is genuinely
about surviving another process's crash rather than about the network.

## What Changes

- **A fourth rung**, `ReliableBroadcast`, composed over best-effort broadcast.
- Implements Cachin, Guerraoui & Rodrigues Algorithm 3.3, *Eager Reliable Broadcast* — the
  fail-silent variant, which uses no failure detector. The alternative, Algorithm 3.2 *Lazy
  Reliable Broadcast*, requires a perfect failure detector, and that rung of the ladder has not
  been built. Eager relays every message on first delivery instead; more traffic, no dependency.
- **The first rung to add a wire field of its own.** The three existing protocols share one
  header; reliable broadcast must carry the original sender with each relayed message, because a
  relayer is not the sender and the recipient must still attribute it correctly.
- **The second transforming layer**, and therefore the second data point for the macro question
  deferred in `notes-composition-boilerplate.md` of the previous change.

Not in this change: uniform reliable broadcast, failure detectors, the scope annotation's
`Scope` associated type, and anything to do with transport.

## Capabilities

### New Capabilities

- `broadcast/reliable-broadcast`: Broadcast that guarantees agreement — if any correct process
  delivers a message, every correct process eventually delivers it, including when the original
  sender crashed partway through sending.

### Modified Capabilities

None.

## Impact

- **Code.** One new module in `recon-protocols`, composed over the existing best-effort broadcast.
  No change to `recon-core` or `recon-sim` is anticipated; if one proves necessary, that is a
  finding worth recording, since it would mean the core was underspecified by three protocols.
- **Wire format.** Reliable broadcast's messages nest inside best-effort broadcast's, which nest
  inside the perfect link's. Two headers where there was one. This is the depth the design
  predicted would begin accumulating here.
- **Verification.** The simulator's crash already loses volatile state, so the sender-crash case
  that agreement exists to handle is directly expressible.
