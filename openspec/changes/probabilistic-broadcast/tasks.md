## 1. Reading the page before writing the module

- [x] 1.1 Transcribe Algorithm 3.9 into the eager module's docstring from Cachin, Guerraoui &
      Rodrigues §3.8, and verify the relay sits **outside** the `m ∉ delivered` guard as the page has
      it — this looks like a defect, has been reported as one, and the quoted indentation is what
      settles it. **Done and verified against the book itself**, which is indexed locally
      (`Reliable and Secure Distributed Programming - Cachin.pdf`), not against a recollection
- [x] 2.1b Add `fair_loss_link`, the link Algorithm 3.9 actually names, and default the gossip to
      it. The book says `Uses: FairLossPointToPointLinks`, verified against the indexed text; this
      change had reached for a perfect link, which retransmits until delivery and so masks the
      probabilistic guarantee the abstraction exists to provide. Not in the original task list —
      surfaced by the termination failure and by the observation that gossip over a link that does
      not lose is gossip with nothing to do. Verify the eager suite passes over it

- [ ] 1.1b Transcribe the lazy algorithm, which the book splits into **Algorithms 3.10 and 3.11** —
      a gossip half and a recovery half — not the single 3.10 this change first assumed. Retrieving
      the full text of both is outstanding; see the note at the end of this file
- [ ] 1.2 Record the α direction in the lazy module: stored when `random([0,1]) > α`, so α is the
      probability of *not* storing, and verify the reading against page 100's `α = 0` example, which
      is what breaks the tie against the book's own prose on page 99
- [x] 1.3 Read `archive/recon-gossip/src/{upb,lpb}.rs` for its data structures only, and verify no
      claim about it enters this work unchecked against the page — three of four bugs once reported
      in it were false positives produced by exactly that method

## 2. Eager probabilistic broadcast

- [x] 2.1 Add the module with its request, indication and wire types, composing over the link port
      with a default, and verify `cargo build --workspace --all-targets` is clean
- [x] 2.2 Implement broadcast and receipt per Algorithm 3.9 — deliver on first receipt, relay to a
      random fanout with a decremented rounds-to-live — and verify a broadcast in a small membership
      with generous fanout reaches everyone on a named seed
- [x] 2.3 Verify a relay addresses exactly the fanout number of peers and never the whole membership,
      in a membership larger than the fanout
- [x] 2.4 Verify gossip terminates: a single broadcast, a run continued well past delivery, and
      transmissions ceasing rather than continuing for as long as the run. **Was blocked and is not
      any more.** It cannot be observed over a perfect link — the stubborn link beneath retransmits
      everything it has ever sent, measured at 2,415 → 26,415 sends over ten times the settling
      period — and the reason was that the default link was wrong. Algorithm 3.9 says
      `Uses: FairLossPointToPointLinks`; see 2.1b
- [x] 2.5 Verify no duplication and no creation — a message arriving many times by different paths
      delivers once, and every delivery corresponds to an earlier broadcast
- [x] 2.6 Verify the peer choice is reproducible, by running one seed twice and comparing the traces

## 3. The retention window, before the sweep

- [x] 3.1 Bound the deduplication state by a configured window, reclaiming on insert rather than by
      a pass over the set, and verify state stays bounded when a process handles far more messages
      than the window holds
- [x] 3.2 Verify reclamation cost does not grow with the run — the work on any single event is
      independent of how many messages have been handled. This is the one defect in the previous
      implementation that survived scrutiny, so it is checked rather than assumed
- [x] 3.3 Verify the scope the window imposes: a message re-arriving after its record has expired is
      delivered a second time, which is the stated guarantee rather than a violation
- [x] 3.4 State the space bound and the scoped no-duplication guarantee in the module docstring, and
      verify the claim there matches what the tests above pin

## 4. Evidencing the probabilistic guarantee

- [x] 4.1 Add the coverage sweep: many seeds, a threshold stated in the test with the fanout, rounds
      and membership it is derived from, and verify the derivation is written down rather than a
      number read off a run
- [x] 4.2 Add the non-vacuity half asserting coverage is **not** total, and verify it fails when the
      fanout is widened to the whole membership — a configuration that always reaches everyone is no
      longer exercising this abstraction
- [x] 4.3 Verify a run below the threshold reproduces from its seed, so a failure can be examined
      rather than only counted
- [x] 4.4 Measure the sweep's wall-clock cost against `alloc_probe.rs`, currently the slowest suite,
      and settle the seed count and membership against that measurement rather than by guess

## 5. Lazy probabilistic broadcast

- [ ] 5.1 Add the module over the eager one, wrapping its wire type, and verify the existing eager
      suite still passes unchanged
- [ ] 5.2 Add per-sender sequence numbers and gap detection, and verify a sequence number ahead of
      the expected one prompts a request for the intervening numbers while an in-order one prompts
      nothing
- [ ] 5.3 Verify a request is addressed to a subset of peers rather than broadcast to the membership
- [ ] 5.4 Hold a message that arrives ahead of a gap, and verify it is neither delivered at that
      moment nor discarded, and that closing the gap releases it in sequence order
- [ ] 5.5 Implement the store that answers requests, with its configurable fraction, and verify a
      peer holding a requested message returns it, and that the maximum setting stores everything
- [ ] 5.6 Add the timeout that moves past an unrepairable gap, and verify delivery resumes rather
      than stalling for ever, and that the skipped message is not delivered if it arrives afterwards

## 6. The lazy module's own bounds and its reason to exist

- [ ] 6.1 Bound the stored copies and the pending messages by the retention window, and verify both
      stay bounded when the process handles far more messages than the window holds
- [ ] 6.2 Verify a request for a message older than every window is answered as unavailable and the
      requester moves past the gap, rather than searching without bound
- [ ] 6.3 Add the recovery comparison — the same seeds, membership and loss rate with recovery and
      with gossip alone — and verify coverage is higher with recovery
- [ ] 6.4 Verify that comparison is not vacuous, by confirming the configuration is one in which
      gossip alone fails on some runs, so there is something for recovery to improve

## 7. What this dates

- [ ] 7.1 Update `docs/bounded-space.md`, which records that everything above the failure detector is
      an unbounded transcription, and verify its audit reflects the first two bounded implementations
- [ ] 7.2 Add a protocol table row and a suite row to `README.md`, and verify the counts and the
      claimed statuses against what `cargo test --workspace` prints and what the modules say
- [ ] 7.3 Check whether `CLAUDE.md` needs the probabilistic-assertion convention stated alongside its
      non-vacuity rule, and add it only if the convention proves to be more than these two suites
- [ ] 7.4 Run `./scripts/check.sh` and verify it passes in full
