## 1. The source, quoted

- [x] 1.1 Quote §4.1 above the acceptor's storage, from the same edition the rest of the module
      quotes — **van Renesse, R. and Altinbuken, D. (2015), *ACM Computing Surveys*, 47(3)**,
      page 42:18. Verify the section number and its title against the survey before quoting it
- [x] 1.2 Quote the paper's own "worrisome effect" paragraph, and state the answer it gives:
      the record of a choice may be overwritten while the choice stands, and Invariant C2 against
      every later ballot is what carries it. Verify the module says a reader who assumes otherwise
      would take a correct run for a broken one

## 2. The acceptor

- [x] 2.1 `Pvalues<C>` becomes `BTreeMap<Slot, (Ballot, C)>`. Verify the type's own documentation
      is rewritten: it currently says the key makes Invariant A4 structural, and after this it does
      not, so say instead which invariant is enforced where — A4 by the leader's C1, and this map by
      §4.1
- [x] 2.2 `on_p2a` writes the pvalue over the slot's record. **Revised during implementation**: it
      was planned as a comparison, and a mutation showed the comparison could not fire — the
      acceptor's own promise already makes the latest acceptance the highest, since a stored ballot
      became `ballot_num` and the `b ≥ ballot_num` arm admits nothing below it. Verify the module
      states that argument rather than shipping a branch nothing can take, and that a test drives
      it: a superseded ballot arriving late must not reach the record
- [x] 2.3 Verify an acceptor commanded for one slot under three ballots holds **one** pvalue for it,
      carrying the highest of the three — driven by hand, since a seeded run need not produce three
      ballots against one slot
- [x] 2.4 Verify a `p2a` for a slot under a ballot **below** the stored one leaves the stored pvalue
      alone, and that the reply still names the ballot the acceptor holds
- [x] 2.5 `accepted_count` keeps its meaning — it is the measurement `docs/bounded-space.md` wants —
      and now counts slots rather than `⟨ballot, slot⟩` pairs. Verify its documentation says which

## 3. The wire

- [x] 3.1 `on_p1a` returns one `Pvalue` per slot. Verify `SynodMsg::P1b`'s documentation says the
      answer is one per slot and why, rather than leaving a reader with the book's set
- [x] 3.2 Verify the answer's size grows with slots accepted for and **not** with ballots seen:
      drive one slot under several ballots and assert the `p1b` carries one entry, then drive
      several slots and assert it carries one each. Assert the non-vacuity half — the run really
      contained more ballots than slots
- [x] 3.3 Verify the wire still survives encoding, and that `Pvalue` is unchanged on the wire: the
      reduction is in what is *sent*, not in the shape of what is sent

## 4. The scout

- [x] 4.1 `pvalues := pvalues ∪ r` reduces per slot as it collects, keeping the highest ballot.
      Verify a majority answering with different ballots for one slot leaves the scout holding one
      entry for it — otherwise the growth §4.1 took off the acceptor lands on the leader
- [x] 4.3 **This is where the maximum is actually taken, and it was tested by nothing.** Mutating
      the collection to keep the last arrival left the whole suite green and the safety guard
      passing; the property had been carried by the old map key's iteration order rather than by
      any test. Add a schedule in which two acceptors report different ballots for one slot and the
      lower arrives last, assert the leader commands the higher one's command, and register the
      test against `synod-ignore-pmax`
- [x] 4.2 `pmax` reads a map that is already one entry per slot. Keep it, and say in its
      documentation what it now is: the step Figure 7 names, over a set the collection already
      reduced. Verify the mutation `synod-ignore-pmax` still has a clause to remove

## 5. Safety, which is the whole risk

- [x] 5.1 `./scripts/check-safety-tests.sh` passes with **every** registered test still red under
      both mutations. This change touches the state the safety argument reads, so a test that stops
      detecting is the signal that the reduction weakened something. Investigate any that goes
      green rather than deregistering it
- [x] 5.2 Verify agreement survives the record being overwritten — the paper's own scenario, driven
      by hand: a majority accepts a proposal for a slot, a later ballot overwrites it at one of
      them so no majority still holds it, and no other proposal is ever chosen for that slot.
      Assert the non-vacuity half from acceptor state: no majority holds the chosen proposal at the
      point the assertion is made
- [x] 5.3 Verify `a_later_ballot_proposes_what_an_earlier_majority_accepted` still passes for the
      reason it names, and that the suite's checker is unaffected — it reads decisions from the
      trace rather than from acceptor state, which is why the reduction does not reach it. Say so
      in the suite where the checker documents what it cannot see

## 6. What this dates

- [x] 6.1 The module's Space section: it currently says an acceptor "keeps every pvalue it has ever
      accepted and sends the whole set in every `p1b`", and that §4.1 belongs to a later change.
      Both stop being true in this commit
- [x] 6.2 `docs/bounded-space.md`: the `multi_paxos_synod` row, and the prose naming §4.1 as
      pending. Verify the row still says the module is unbounded, because it is
- [x] 6.3 `README.md`: the protocol table's space column for the Synod row, the suite table and
      counts if the suite changed size, and the roadmap prose that names §4.1 and §4.2 together as
      what remains. Verify the counts against `cargo test --workspace`
- [x] 6.4 `README.md`'s real-world set section: this change does not admit Multi-Paxos to the set,
      because the state is still unbounded. Verify the wording does not now imply it does
- [x] 6.5 `./scripts/check.sh` passes in full
