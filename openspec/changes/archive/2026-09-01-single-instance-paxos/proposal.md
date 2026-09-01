## Why

`flooding_consensus` is the only consensus here, and it is fail-stop: it needs a *perfect* failure
detector, so a single false suspicion splits the decision permanently — which its own suite
demonstrates rather than hides. That is the algorithm the book presents first and the one nobody
deploys.

Paxos is the one people deploy, and the reason is precisely the property flooding consensus lacks:
it stays **safe** when the leader detector is wrong. Two processes can both believe they lead, in
overlapping epochs, and no two processes decide differently. Building it is what turns this
repository from a transcription of Chapter 3 into something that has met the algorithm the field
actually runs.

It is also the first thing here that needs an abstraction the repository does not have. Everything
built so far rests on a perfect failure detector; Paxos rests on Ω, an *eventual* leader detector
that is allowed to be wrong for a while. Adding it is what makes the fail-noisy model reachable at
all.

## What Changes

Cachin, Guerraoui & Rodrigues decompose Paxos into three abstractions rather than presenting it as
one algorithm, and this change follows that decomposition, then repeats it for the fail-recovery
model.

- **An eventual leader detector (Ω)**, derived from the perfect failure detector already here by
  Algorithm 2.8's monarchical construction: `leader := maxrank(Π \ suspected)`, the **highest**-ranked
  process not suspected. (An earlier draft of this proposal said lowest; the page says `maxrank`.)
- **Epoch-change** (Module 5.5, Algorithm 5.5): a sequence of epochs, each with a timestamp and a
  leader, advancing when the trusted leader changes.
- **Read/write epoch consensus** (Module 5.6, Algorithm 5.6): the quorum core — a leader reads the
  state a majority holds, writes its value to a majority, and decides. Abortable, and it returns
  its state when aborted so the next epoch can begin from it.
- **Leader-driven consensus** (Algorithm 5.7): the two above tied together, implementing uniform
  consensus. This is Paxos.
- **The fail-recovery versions of all three** (Algorithms 5.8, 5.9, 5.10–5.11): the same algorithms
  with the acceptor state written through `Cx::storage`, so a process that crashes and restarts
  rejoins without violating what it already promised. This half is what a deployment runs.

**The suite's headline obligation is safety under a lying detector.** The synchrony assumption is
withdrawn so the detector accuses correct processes, two leaders coexist in overlapping epochs, and
agreement is asserted anyway — with a non-vacuity half confirming the split leadership genuinely
occurred. Testing Paxos where the detector behaves is testing nothing that flooding consensus does
not already give.

Explicitly **not** in this change: multi-Paxos, a replicated log, or any notion of a second
consensus instance. One instance, one decision. No transport, and no new simulator fault — the
existing `suspend`/`resume` and synchrony knobs are expected to produce an inaccurate detector,
because `perfect_failure_detector`'s own suite already turns accuracy off that way.

**This is a large change** — eight modules where the last two changes added one and two. The
fail-noisy half stands alone and is the prerequisite for the logged half, so it is sequenced first
and the change can be stopped between them without leaving anything half-built.

## Capabilities

### New Capabilities

- `failure-detection/eventual-leader-detector`: Ω — eventually some correct process is trusted by
  every correct process, and it may be wrong before that.
- `consensus/epoch-change`: a sequence of epochs, each with a timestamp and leader, monotonically
  increasing, agreed eventually but not immediately.
- `consensus/epoch-consensus`: an abortable single-shot consensus within one epoch, which returns
  its state on abort so a later epoch can respect what it decided.
- `consensus/leader-driven-consensus`: uniform consensus from the two above — Paxos.
- `consensus/logged-epoch-change`: the same guarantees across a crash and recovery.
- `consensus/logged-epoch-consensus`: likewise, with the accepted value and its timestamp durable
  before anything reveals them.
- `consensus/logged-leader-driven-consensus`: uniform consensus in the fail-recovery model.

The logged variants are separate capabilities rather than requirements of the volatile ones,
following this repository's existing split between `links/perfect-link` and `links/logged-link`, and
between `broadcast/uniform-reliable-broadcast` and its logged sibling.

### Modified Capabilities

None. Everything composes over ports that already exist — perfect links, stubborn links, best-effort
and stubborn broadcast, the perfect failure detector, and `Cx::storage`. No delta is invented to
make the change look larger.

## Impact

- `crates/recon-protocols`: seven new modules and their suites. The fail-noisy three compose over
  the link port; the logged three use the stubborn link and stubborn broadcast, which is what
  Algorithms 5.9 and 5.11 name.
- `crates/recon-sim`: expected to be unaffected. Producing an inaccurate detector uses knobs that
  already exist and are already exercised by `perfect_failure_detector`'s accuracy tests.
- `crates/recon-core`: unaffected. `Cx::storage` and `on_recovery` are what the logged half needs
  and both are in place.
- **A composition shape this repository has not used.** Leader-driven consensus does not own one
  epoch-consensus child for its lifetime — it starts a *new instance per epoch*, seeded with the
  state the previous one returned when it aborted. `CLAUDE.md` says parents own children as concrete
  typed fields, and that still holds: the field is one concrete type, replaced rather than
  registered. It is nonetheless the first layer here whose child is rebuilt while running, and the
  design says how.
- `README.md`, `docs/bounded-space.md`: a protocol row and a space bound for each module.
