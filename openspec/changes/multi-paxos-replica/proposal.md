## Why

`multi_paxos_synod` agrees on a command for a slot, and nothing can use it. The caller has to choose
slot numbers itself, which is the replica's job; a decision reaches only the process whose commander
counted the majority; and there is no ordered sequence anywhere, so the module satisfies no port and
the shared total-order suite cannot be pointed at it. Change 1 built the consensus core and left the
thing that turns it into a replicated log.

This is that change, the second of the three the Synod proposal named. It builds §2.1 of the source
— the replica of Figure 1 — over the existing core, and satisfies `TotalOrderLog` so that the suite
every other ordered log here is held to runs against Multi-Paxos too. That is the point of the port:
where two implementations differ becomes visible rather than asserted, and this adds a third
implementation whose difference from the other two is that it pays one consensus for a *stable
leader* rather than one per entry.

It also discharges a debt. `docs/bounded-space.md` lists `consensus_based_total_order_broadcast` and
`logged_uniform_total_order_broadcast` as the two worst rows in the audit — one consensus instance
per round, in lock-step, with `unordered`, `delivered` and the whole family of instances growing with
entries handled. Multi-Paxos is what replaces them. This change does not yet bound anything, so it
joins them in the audit rather than replacing them there; **change 3 is what makes it the answer**
rather than a fourth transcription. What this change buys now is the shape: a stable leader that
keeps phase one across slots, so an entry costs one round trip instead of a full consensus.

## What Changes

- **`multi_paxos_replica.rs`** — §2.1 of the source, transcribed, with Figure 1 quoted above the
  implementation. Seven variables become five: `slot_in`, `slot_out`, `requests`, `proposals` and
  `decisions`, with `leaders` fixed for the run and `state` deliberately absent (see below).
  Invariants R1–R5 are stated and tested.
- **The replica composes the Synod core as a child**, and nothing else. It has no link, no
  broadcast and no wire of its own: `Cmd::Propose { slot, command }` is a function call into the
  child, and `Ind::Decision` is what comes back. That is possible only because of the modification
  below.
- **Satisfies `TotalOrderLog`**, so `tests/total_order_log.rs` runs against a third implementation.
  `append` is Figure 1's `⟨request, c⟩`, `read` serves the applied prefix from this process's own
  copy, and `Ordered { position, from, value }` is what `perform` announces.
- **A command carries its originator and a request identifier** — the source's `c = ⟨κ, cid, op⟩`.
  Both are needed rather than decorative: `from` is what the port's `Ordered` reports, and `cid` is
  what keeps two appends of the same value from being deduplicated into one by the
  already-decided check in `perform`.
- **The fourth liveness violation from Liu, Chand and Stoller (2019), applied.** Change 1 recorded
  it for this change to find: if no decision arrives for a slot, every replica stops applying from
  that slot, `slot_out` stops moving, `WINDOW` fills, `slot_in` stops advancing, and the whole
  system wedges. The fix is for a replica to re-propose for that slot after a timeout.
- **`WINDOW` is implemented as R5 states it**, as the cap on how far `slot_in` may run ahead of
  `slot_out`. Its *reason* — that a configuration decided in slot `s` takes effect at `s + WINDOW`
  — belongs to reconfiguration, which this change does not build, so the guard is kept and the
  reason is stated as deferred rather than implied.

### The modification to the Synod capability, and why it is one

§4.4 of the source describes the deployment this repository already has: "each machine that runs a
replica also runs a leader… the replica can send a proposal for a particular slot to its local
leader, say λ, rather than broadcasting the request to all leaders. If λ is passive, monitoring
another leader λ′, it forwards the proposal to λ′. If λ is active, it will start a commander."

Taking that shape makes two messages the Synod layer's rather than the replica's, and both are
already on its own figures:

- **A passive leader forwards a proposal to the leader Ω trusts.** Change 1's `on_propose` already
  remembers a proposal made while passive and commands it on adoption; what it cannot do is act for
  a process that will never be trusted. The forward is one new wire variant and one handler.
- **A commander broadcasts its decision**, which is Figure 6(a)'s own last line —
  `∀ρ ∈ replicas : send(ρ, ⟨decision, s, c⟩)`. Change 1 dropped it because there was no replica to
  address and says so in the module. There is one now.
- **A leader answers a proposal for a decided slot with the decision**, sent to the asker alone.
  The decision broadcast happens once, so a process it never reached recovers by asking, and
  re-proposal is the asking; a leader that ignored the repeat would leave it asking for ever. This
  is the leader-side half of Liu et al.'s replica fix, cited in the module where it lands.

With both, the replica needs no link and no broadcast child, and its body is exactly Figure 1. The
cost is stated rather than hidden: **proposal delivery becomes conditional on Ω**, because a
proposal now goes to one process instead of every leader, and a detector naming a process that has
died loses it until the re-proposal timeout fires. That is a new dependence on the failure detector
being right, in a repository that provokes wrong detectors on purpose, so it gets its own tests
rather than an assumption. Forwarding also needs a loop rule — a forwarded proposal is not forwarded
again — because two processes disagreeing about who leads would otherwise pass one back and forth.

### Deliberately not in scope

- **The application state machine.** Figure 1's `state`, `op(state)` and `send(κ, ⟨response, cid,
  result⟩)`. The port is a *log*: `LogInd::Ordered` says an entry took its place, and R3 — that
  state is the result of applying decisions in order — is something any layer above can do for
  itself. Both existing implementations behind the port are logs for the same reason. What is kept
  from `perform` is the half that is not about state: the already-decided check, which is what stops
  one command taking two positions.
- **Reconfiguration.** The source specifies it fully and it is still change 3's or later. `WINDOW`
  is here; `isreconfig` and a configuration in the decided sequence are not.
- **Bounding.** `decisions` is append-only, as the source has it — "there is no code that removes
  entries from this set. Doing so makes it easier to formulate invariants" — and §4.2 is what
  removes them. Change 3.
- **Durability.** Inherited from the core: crash-stop, and a process returning without its state is
  outside the model. The replica adds a second volatile identity, `cid`, whose scope is stated for
  the same reason the ballot's is.
- **Read-only commands and leases.** §4.5 needs a known bound on clock drift, and per-node clocks
  do not exist.

## Capabilities

### New Capabilities

- `consensus/multi-paxos-replica`: the replica of *Paxos Made Moderately Complex* §2.1 — how
  requests become slots, what a replica may propose, when a decision may be applied, and what the
  resulting ordered sequence guarantees

### Modified Capabilities

- `consensus/multi-paxos-synod`: three requirements change. A decision is raised at **every**
  process rather than only at the one whose commander counted the majority, which is Figure 6(a) as
  written; a passive leader **forwards** a proposal to the process the detector trusts rather than
  holding it until it leads, which is §4.4's colocation; and a leader **answers** a proposal for a
  decided slot with the decision, which is what makes the replica's re-proposal a recovery rather
  than a loop. The forward adds a dependence on the detector that the capability must state, since
  progress for a proposal now rests on it.

## Impact

- **New module** `crates/recon-protocols/src/multi_paxos_replica.rs`, and a suite beside it.
- **`multi_paxos_synod.rs`** gains two wire variants, one handler, and a change from `trusted: bool`
  to the identity of the trusted process. Its suite gains tests for the forward, the loop rule, and
  the decision broadcast.
- **`tests/total_order_log.rs`** gains a third implementation behind the shared suite. The suite is
  written against the port, so this is a registration rather than a rewrite.
- **`scripts/check-safety-tests.sh`** — the two mutations now have a third suite that must notice
  them, and R1 is the same claim as S1 seen through the log.
- **No change to `recon-core` expected.** The replica composes one child that keeps nothing durable,
  which `Child::run` already serves.
- **`README.md`** — the protocol table, the suite table and counts, the specification tree, and the
  roadmap item, which this advances from "the Synod core is built" to "there is a log".
- **`docs/bounded-space.md`** — a row for the replica, and an amendment to the Synod row, since a
  decision broadcast is new work per decision.
