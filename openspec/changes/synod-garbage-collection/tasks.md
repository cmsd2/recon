## 1. The source, quoted

- [ ] 1.1 Quote §4.2 above the collection, from the same edition the rest of the module quotes —
      **van Renesse, R. and Altinbuken, D. (2015)**, pages 42:18–19. Verify the section number and
      title against the survey before quoting it
- [ ] 1.2 Quote the hazard in the paper's own words — "we must prevent other leaders from mistakenly
      concluding that the acceptors have not accepted any pvalues for the garbage-collected slots" —
      above the clause that prevents it, so the two are read together
- [ ] 1.3 Quote the `2 f + 1` caveat where the watermark is computed, so that a run which stops
      collecting is recognised rather than diagnosed

## 2. Measure before changing anything

- [ ] 2.1 A test that records what each process holds today over a run of many slots, at three and
      five processes. Verify the numbers grow with the slots decided, which is the claim
      `docs/bounded-space.md` makes and the thing this change falsifies
- [ ] 2.2 Verify the measurement reads the accessors the audit already asks for — `accepted_count`,
      `decided_count`, the replica's `decisions_held` — rather than adding parallel ones

## 3. The report

- [ ] 3.1 `SynodMsg::Applied { slot_out }`, sent periodically to every other process. Verify it is
      on the Synod wire and not on a wire of the replica's: the replica has none, and §4.4's
      colocation is what that buys
- [ ] 3.2 The replica tells its own child by a command, on its own timer, and the child gossips.
      Verify the replica still registers exactly one timer and compares before acting
- [ ] 3.3 `reported: BTreeMap<NodeId, Slot>`, one entry per member. Verify it is bounded by
      membership by construction rather than by pruning
- [ ] 3.4 Verify an **idle** replica still reports: a process applying nothing is the one whose
      position others most need, and a report carried on its own traffic would fall silent with it
- [ ] 3.5 Choose the report interval, and say where the number comes from. Verify it does not show
      in the cost identity — `phase_two_costs_one_exchange_per_acceptor_per_entry_...` counts by
      kind, so a new kind must not disturb it, but the identity's own claim must still be exact

## 4. The watermark, and the collection

- [ ] 4.1 The watermark is the highest slot at least `f + 1` members have applied to. Verify it uses
      the existing majority test rather than a second expression of the same idea, and that the
      module says why `f + 1` of `2 f + 1` is a majority here — the roles are co-located, so the
      replica set and the acceptor set are one
- [ ] 4.2 Collect below the watermark: the acceptor's pvalues, the leader's proposals, the decided
      record. Verify each is collected and that the module names all three, since the audit's row
      lists all three
- [ ] 4.3 Verify nothing is collected before `f + 1` members have reported. Assert it directly, with
      a run in which exactly `f` have
- [ ] 4.4 Verify the watermark only ever rises, so that collection cannot un-collect

## 5. `collected`, and the skip that safety rests on

- [ ] 5.1 The acceptor keeps `collected` and puts it in `p1b`. Verify the wire type's documentation
      says what the field is for, in the paper's terms
- [ ] 5.2 `adopted` skips every slot below the **highest** `collected` any answering acceptor
      reported. Verify the module says why the highest and not the lowest, and that taking the
      lowest would be safe but would waste the collection
- [ ] 5.3 Verify a leader proposes for no slot below the reported watermark and starts no commander
      for one — driven by hand, since a seeded run need not put a fresh proposal against a collected
      slot
- [ ] 5.4 **Verify agreement holds across a collection**: a proposal is chosen, every process's
      state for that slot is collected, a new leader takes up a higher ballot, and no different
      proposal is ever chosen for that slot. Assert the non-vacuity half from process state — the
      slot really was collected everywhere before the new ballot ran
- [ ] 5.5 Add `synod-skip-collected` to `scripts/check-safety-tests.sh`: a mutation removing the
      skip, with every test that claims agreement registered against it. Verify each registered test
      goes red. **This clause is new safety evidence and must not ship without a mutation**

## 6. The replica's retention window

- [ ] 6.1 `decisions` and `performed` bounded by a retention window. Choose what the window is
      measured in — time, as the paper has it, or slots — and say in the module which and why
- [ ] 6.2 Verify the window is **not** the collection watermark: that watermark says `f + 1`
      replicas applied up to a slot, which says nothing about a duplicate arriving above it. State
      the reason in the module, because using the watermark is the obvious wrong move
- [ ] 6.3 State the weakened guarantee in the module: a command decided twice takes one position
      *within the window*. Verify it also says what a duplicate arriving after the window would
      cause
- [ ] 6.4 Verify two distinct appends of the same value still take two positions, inside the window
      and outside it
- [ ] 6.5 Verify the ordered sequence is **not** collected, that the module says it is the data
      rather than the bookkeeping, and that it names what would bound it — snapshots, which are
      outside the paper

## 7. Bounded, and asserted as such

- [ ] 7.1 A test that state does not grow with the slots handled: run one length and a much longer
      one, and require the same bound. Verify it asserts **collection happened** and the watermark
      advanced first, or the bound holds over a run that collected nothing
- [ ] 7.2 The same for the replica's bookkeeping, with the sequence excluded and the exclusion
      stated in the test rather than assumed
- [ ] 7.3 Verify the send rate is still flat — `tests/common::assert_send_rate_flat!` — with the new
      periodic report running. The report is exactly the kind of thing that makes a rate not flat
- [ ] 7.4 Verify the `2 f + 1` caveat: with `f` crashed, collection stalls and state grows, and the
      test says this is correct rather than a defect

## 8. Safety, which is the standing risk

- [ ] 8.1 `./scripts/check-safety-tests.sh` passes, with the new mutation and both existing ones,
      and every registered test red under each. Investigate any that goes green rather than
      deregistering it
- [ ] 8.2 Verify the tests that read acceptor state still test what they claim. A collected slot is
      absent from `accepted_for` exactly as an unaccepted one is, so a test reading absence as
      "never accepted" is now wrong in the same way a leader would be
- [ ] 8.3 `agreement_survives_the_record_of_it_being_overwritten` still passes and still means what
      it says. Collection is a second way for evidence to vanish while the fact stands, and the test
      should say so
- [ ] 8.4 `cargo test --workspace` and `./scripts/check-durability-tests.sh` pass

## 9. What this dates

- [ ] 9.1 `docs/bounded-space.md`: both rows change from ❌ to bounded, and the surrounding prose
      about which abstractions are bounded. Verify the audit's own conclusions are updated, not just
      the table — a table that disagrees with the paragraph under it is worse than either
- [ ] 9.2 The modules' own status lines: both become **implementations**. Verify each states its
      bound and what it is conditional on
- [ ] 9.3 `README.md`: the protocol table's status and space columns for both modules, the suite
      table and counts, and the roadmap item that names §4.2 as what remains
- [ ] 9.4 `README.md`'s real-world set section: with state bounded, **both** halves of the
      resource-use obligation are met and Multi-Paxos joins the set. Verify the wording says what
      remains — §4.3's durability, which is a different obligation and not this one
- [ ] 9.5 `CLAUDE.md`'s real-world set membership list, which names multi-Paxos as joining "when it
      is written". Verify it now says what is actually true
- [ ] 9.6 `./scripts/check.sh` passes in full
