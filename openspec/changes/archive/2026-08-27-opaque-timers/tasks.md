## 1. The handle

- [x] 1.1 Add the opaque timer handle type to `recon-core` and export it, and verify
      `cargo build --workspace` is clean with nothing yet using it

## 2. Replacing the type with the handle

This group lands in one piece. `Cx::set_timer` exists only to emit `Effect::SetTimer`, so its
signature cannot change before the effect does; once the effect carries a handle the driver hands a
handle to `on_timer`, so `Protocol::Timer` is unusable from that moment and every protocol follows.
**The workspace does not build until the group is finished**, so its verifications are at the end
rather than per task.

- [x] 2.1 Change `Cx::set_timer` to take a duration and return the handle, drawing identities from a
      driver-owned source shared down the whole composition
- [x] 2.2 Point `Effect::SetTimer` at the handle, drop the timer type parameter from `Effect`, and
      remove the timer mapper from `Effect::map` — a timer has nothing in it for a parent to re-wrap
- [x] 2.3 Remove `Protocol::Timer`, change `on_timer` to take the handle, and drop the timer
      parameter from `Event` and from the composition helpers
- [x] 2.4 Convert the stubborn link and the failure detector to hold the handle they registered
      instead of `armed: bool`, acting only on an expiry that matches
- [x] 2.5 Strip the `Timer` enum, `type Timer`, mapper argument and unwrapping handler from each of
      the fourteen relaying layers **by hand or with per-file review**, diffing each file against its
      original before moving on — a scripted edit has already silently removed an adjacent handler
      once
- [x] 2.6 Make each composing layer pass an expiry to every one of its children
- [x] 2.7 Move the run's identity source into the simulator, carry the handle on the trace's timer
      entry, and drop the timer parameter from `TraceEvent` and `Trace`
- [x] 2.8 Verify `cargo build --workspace --all-targets` is clean, and that no module under
      `crates/recon-protocols/src` still mentions `type Timer` or `enum Timer`
- [x] 2.9 Verify `cargo test --workspace` passes, with any test that changed shape rather than
      behaviour accounted for

## 3. What the handle makes possible

- [x] 3.1 Verify the stubborn link ignores an expiry it has superseded, by registering,
      re-registering, then firing the first and observing that nothing is retransmitted
- [x] 3.2 Verify the failure detector does the same, and accuses nobody on a superseded expiry
- [x] 3.3 Verify two layers of one process registering timers receive different handles
- [x] 3.4 Verify a run with timers reproduces from its seed, by running one seed twice and comparing
      traces including the handles their timer entries name
- [x] 3.5 Verify which timer fired is readable from the trace alone, with two layers of one process
      holding timers at once

## 4. Driving a protocol by hand

- [x] 4.1 Add the helper taking a caller-owned identity source, and document what the existing
      helper is for and why it must not be used on a composition
- [x] 4.2 Move the tests that drive a composed protocol by hand onto it, and verify they fire the
      handle the protocol registered rather than an assumed value
- [x] 4.3 Verify the failure this prevents is real, by confirming a hand-driven composed test fails
      when given a per-call identity source

## 5. Pinning the obligation

- [x] 5.1 Add a test that a layer with a timer outstanding, given an expiry registered by a
      different layer, does nothing and keeps its own timer outstanding
- [x] 5.2 Verify that test is not vacuous, by confirming it fails when the layer's comparison is
      removed

## 6. What this dates

- [x] 6.1 Update `CLAUDE.md` where it describes a parent re-wrapping every child effect, and verify
      no other convention it states has been contradicted
- [x] 6.2 Check `README.md`'s suite counts against what `cargo test --workspace` prints, and correct
      them if the count moved
- [x] 6.3 Run `./scripts/check.sh` and verify it passes in full
