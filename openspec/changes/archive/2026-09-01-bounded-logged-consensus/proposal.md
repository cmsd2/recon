## Why

The two logged consensus modules do work proportional to how long they have been running. Measured
in steady state with nothing faulty, `logged_epoch_consensus` sends 12.6k, 28.6k, 44.6k, 60.6k, 76.6k
messages in successive 400 ms windows — a rate growing linearly in time, so a total growing
quadratically. The cause is that the stubborn broadcast beneath redelivers `READ` and `WRITE` every
interval, and every redelivery is answered with a *fresh* stubborn transmission. `logged_epoch_change`
has the same shape latent for stale announcements. The volatile Paxos is flat at 1,400 per window.

This is the failure `docs/bounded-space.md` names — work bounded by time rather than membership — in
modules whose documentation claims a membership bound. It went unnoticed because the seven modules
added by `single-instance-paxos` carry no growth test, which `CLAUDE.md` already requires of any
module claiming to be an implementation.

Separately, the composition boilerplate that every composite module repeats has reached sixteen
copies, and constraint 4 said to extract it after two or three.

## What Changes

- **A follower answers a stubborn announcement once.** The reply travels by a stubborn link and is
  itself retransmitted until retired, so a second reply to a redelivered `READ` or `WRITE` adds a
  transmission and no information. Algorithm 5.9 replies on every delivery; the departure is stated
  in the module. Same for `logged_epoch_change`'s `NACK`: one per distinct announcement per peer,
  bounded by membership.
- **Every module claiming a membership bound gets a growth test**: the same run twice as long sends
  the same amount per window.
- **`crash_on_next_write` is spent** in the two logged modules that lacked it — on the epoch write
  and on the decision write.
- **The disputed-leadership non-vacuity is read from the trace** — two processes each sent a
  leader-only message, and the earlier one was still acting after the later one began — rather than
  from who each process follows when the run ends.
- **Ergonomics, no behaviour change**: `recon_core::Child<P>` bundles a child with its indication
  inbox and replaces the sixteen hand-written `through_*` functions; `slot!` builds a `Slot` from a
  field name; `Timing` replaces three positional `Duration`s; `Sim::at` replaces
  `protocol(n).unwrap()`; the new suites share a `tests/common`.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `consensus/logged-epoch-consensus`: work is bounded by membership — a redelivered announcement is
  not answered again
- `consensus/logged-epoch-change`: work is bounded by membership — a redelivered stale announcement
  is not refused again

## Impact

`recon-core` gains `Child`, `slot!`; `recon-sim` gains `Sim::at`; `recon-protocols` gains `Timing`
and every composite module is rewritten over `Child` with no behavioural change (the suites are the
guard). Constructor signatures of the four leader-driven modules change from three `Duration`s to a
`Timing`. `docs/bounded-space.md` and `README.md` are dated by the space claims.
