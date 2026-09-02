`design.md`'s two open questions are answered while writing the modules and recorded there — which
consensus sits under Algorithm 6.1, and the fail-recovery variant's algorithm number. Neither changes
what is built.

## 1. The port

- [x] 1.1 `total_order_log.rs` on the model of `link.rs`: an implementation keeps its own `Cmd` and
      `Ind`, and the port supplies building an append, building a read, and classifying an
      indication. Nothing else about an implementation is visible
- [x] 1.2 Classification is **total**, as `Link::classify` and `Detector::classify` are: every
      indication an implementation raises maps to something the port names, so a layer above has no
      case it can only discard
- [x] 1.3 A `compile_fail` doctest that a protocol with the right shape but no declaration is not a
      log — satisfying a port is a decision, not an accident, and the link port records an earlier
      draft getting this wrong with a blanket impl
- [x] 1.4 Verify the port admits an implementation without the port naming it, and that a property
      written against the port compiles against more than one

## 2. Algorithm 6.1

- [x] 2.1 `consensus_based_total_order_broadcast.rs`, with the pseudocode **quoted from the page** —
      checked against the book rather than pasted from a search result, since the indexed text loses
      symbols: `if m ∈ delivered` is `∉`, and `upon unordered = ∅` is `≠`
- [x] 2.2 Consensus instances as a **family** keyed by round, created on demand — including for a
      round this process has not started, so a peer that is ahead can make progress. **Corrected
      from "replaced as the round advances", which was wrong**: the page indexes instances as
      `c.round` and nothing supersedes anything, and replacement would drop a peer's message once,
      unresent, leaving flooding consensus a participant short. Record which consensus sits
      underneath and why: the page says `Consensus`, which `flooding_consensus` satisfies literally,
      while `leader_driven_consensus` is uniform and survives a partition
- [x] 2.2a Discharge the conditional event handler by hand. `such that r = round` is the book's
      instruction to "buffer external events until the condition on internal variables becomes
      satisfied", and `Cx` provides no such facility — so a decision for a round not yet reached is
      **held** and acted on when `round` catches up, never discarded. `leader_driven_consensus`'s
      `pending` is the same pattern already in the repository. Verify a decision arriving early is
      not lost
- [x] 2.3 The deterministic sort. Verify two processes deciding the same set produce the same
      sequence — the key must be a total order over `(sender, entry)` that every process computes
      identically. This repository bans `HashMap` outright for the same reason, so the failure mode
      is a known one
- [x] 2.4 State the space bound in the module: **unbounded, a transcription**. `unordered`,
      `delivered` and the instance family all grow with entries handled, as the page has them, and bounding either would
      weaken a guarantee to a scope and belongs to its own change
- [x] 2.5 List the departures beside the quoted pseudocode, the read among them

## 3. The suite, written against the port

- [x] 3.1 Total order: two correct processes' sequences are prefixes of one another, and equal at
      every position both have reached
- [x] 3.2 Validity: an entry appended by a correct process is eventually delivered everywhere.
      No creation: nothing delivered was not appended. No duplication: nothing delivered twice
- [x] 3.3 The read: returns the sequence from a position; may lag an append completed elsewhere; and
      two reads anywhere in the run are prefixes of one another
- [x] 3.4 Non-vacuity, and this suite needs it more than most: assert the run **contained
      overlapping operations**, using the invocation intervals item `C` built. A total-order property
      is satisfied by a run in which nothing overlapped, which is exactly what `tests/method.rs`
      exists to reject
- [x] 3.5 Run the whole suite against Algorithm 6.1

## 4. The fail-recovery variant

- [x] 4.1 `logged_uniform_total_order_broadcast.rs` — `LoggedUniformTotalOrderBroadcast`, over
      `logged_uniform_reliable_broadcast` and `logged_leader_driven_consensus`. **Algorithm 6.12, p. 327** —
      found with `pdftotext` on the PDF itself after four index searches failed, which is the better
      tool for verbatim quoting and worth remembering. Reading the page also turned up a departure
      the index had not shown: the book assumes "the runtime environment re-instantiates all
      instances of consensus that had been dynamically initialized before the crash", and there is
      no such runtime here
- [x] 4.2 A round's proposal is durable before it is visible, and re-proposed on recovery for a round
      that had not decided — `retrieve(proposals)` and the `recovering` branch the page has
- [x] 4.3 Verify the ordered sequence survives a restart, and that a restarted process's sequence and
      a process that never failed remain prefixes of one another
- [x] 4.4 Verify a process that dies **inside** a durable write recovers holding a prefix of the
      agreed sequence, whether or not the write landed — `crash_on_next_write` is what models it
- [x] 4.5 Run the shared suite from group 3 against this implementation too. What differs between
      the pair is what survives a restart and nothing else, which is the point of having both
- [x] 4.6 State the space bound: unbounded, and unbounded **in stable storage** as well

## 5. What this dates

- [x] 5.1 `README.md`'s roadmap item `4–5`: what is built and what is not. Multi-Paxos is not in
      Cachin at all — the practical writeups are elsewhere — so item `5` needs to say which page it
      would transcribe, which is a question this change deliberately does not answer
- [x] 5.2 `README.md`'s spec tree, protocol tables, suite table and counts
- [x] 5.3 `docs/bounded-space.md`: rows for both new modules, and the correction that a consensus
      instance's outstanding set **is** released — by the child being replaced, not by `Stop` — which
      an edit of 2026-09-02 overstated
- [x] 5.4 `./scripts/check.sh` passes in full

## 6. The recovery that was not there, found by taking the network away

- [x] 6.1 An audit found 4.2 checked and not built: no `on_recovery` existed, `recovering` was
      never set true, the recovery branch in `drain_decisions` was dead code, and every durable
      record was write-only. The restart tests passed anyway, because a crash and restart in the
      same instant is rebuilt by the retransmission backlog in flight — the network's redundancy,
      not storage's. Proven by partitioning the restarted process and draining the backlog against
      the dead one first: it came back holding nothing
- [x] 6.2 The restart tests now isolate: crash, drain, partition, then restart, so what survives
      came from storage alone. The write-death test asserts the death preceded the restart — the
      earlier form counted deaths over the whole run, and the one it counted came after the
      recovery it was meant to justify. A recovered process also appends something new, per the
      convention the suite had skipped
- [x] 6.3 `on_recovery` implemented: replay the appended record — announcing each entry again, as
      the logged consensus re-announces its decision — recover the broadcast child, re-instantiate
      every consensus instance the durable record names before acting on any decision, then walk
      the decisions forward and re-propose for the round that never decided
- [x] 6.4 A shared crash property, and what it caught in **both** members: a dynamically created
      consensus instance never received `⟨ Init ⟩`, so its failure detector had no timers. No
      fault-free run notices, because deciding under the initial epoch never consults the
      detector. Survivors could not detect a crash and stalled; a recovered process, hearing no
      heartbeats, trusted itself and climbed epochs for ever with a linearly growing send rate.
      Creation now runs the instance's `on_init` before the event that provoked it
- [x] 6.5 Both suites raise `max_steps`: `run_for` stops dispatching at the budget without saying
      so, and a post-recovery append silently vanished. That silent stop is the simulator
      absorbing something without raising the event that says so, and is left here as an open
      question for the sim rather than fixed in passing
