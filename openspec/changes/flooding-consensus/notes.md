# What the exercise showed

Notes written after the fact, per task 5.2.

## The split was easier to provoke than expected, and that is the finding

A partition of four processes into two pairs, inside synchronous mode, splits the decision on the
first seed tried. Each side accuses the other of crashing, each completes its own rounds, and each
decides the minimum of its own half's proposals. No searching over seeds was needed, no careful
interleaving, no crash — every process is correct throughout, and both sides decide.

That is worth stating plainly because the algorithm is *correct*. It is a faithful transcription of
Algorithm 5.1, and Algorithm 5.1 is proved correct in the fail-stop model. What the ease of the
split measures is how narrow that model is: the whole of agreement rests on one assumption, and the
assumption fails the moment the network does something a real network does routinely.

## The two failure modes separated cleanly, and in opposite directions

- **Accuracy costs safety.** `a_false_suspicion_splits_the_decision` and the three tests around it.
  Two correct processes decide differently, permanently, with nothing in the layer that could
  detect or repair it.
- **Completeness costs liveness.** Visible as the absence of its own test: every termination test
  in the suite requires `Cmd::Start` to have armed the detector, and a round after a crash cannot
  complete until the crash is reported. Withhold the report and the run simply blocks. Nobody is
  wrong; nothing happens.

They did not overlap in testing, and the reason they cannot is structural. A safety failure needs
`correct` to be *too small*; a liveness failure needs it to be *too large*. One schedule cannot
produce both at the same process at the same time.

## Stabilisation arrives too late, and saying why correctly matters

The first draft of this design said the split persists because this detector's accusations are
irrevocable — Module 2.6 has `Crash` and no `Restore`. That is true and it is the wrong reason. It
makes the result look like an artefact of running `P` outside its model, and implies that an
eventually perfect detector would fix flooding consensus.

It would not. The model is not one in which the correct set decays; it is one of eventual
stability, which `docs/scope-annotated-modules.md` already names Assumption F and identifies with
the partial-synchrony global stabilisation time. `◇P` would *withdraw* both false suspicions after
the heal and every process would again be held correct by every other — and both decisions would
still stand, because a decision is irrevocable and both were taken before stability returned.
`the_split_outlives_the_system_stabilising` asserts that form, deliberately, rather than the weaker
"no accusation is ever withdrawn" that would not survive the substitution.

This is the whole of what separates this rung from the leader-driven family. Flooding consensus
commits during instability and therefore has nothing left for Assumption F to rescue. A
quorum-based algorithm declines to commit until no conflicting decision is possible, and so it
does.

## A near-miss worth recording: the accidental polling loop

The decision guard is a standing condition over state, so it must be re-evaluated when `correct`
shrinks and not only when a message arrives. It is called from both child paths for that reason.

The first test of that property passed **with the detector path's call removed**. The reason is
that the stubborn link retransmits for ever, and its retransmit timer re-enters the broadcast child
often enough to re-evaluate the guard by accident. Under this stack the correct call is invisible;
the algorithm limps along on a polling loop it does not know it has.

Two consequences. The test now drives the protocol directly with `step` rather than through a run,
and asserts that the crash indication *in that step* emits the next round's broadcast — which fails
when the call is removed, as verified by making the change and watching it fail. And the module
says why, because the accident disappears under a link that does not retransmit. The session link
is exactly such a link.

The general lesson is the one `tests/method.rs` already makes: a test that exercises a property
through a long stack may be measuring the stack rather than the property. Removing the code the
test is meant to protect, and confirming the test fails, is cheap and was worth doing twice here.

## What this implies for the rungs after it

- **`◇P` needs `Restore`, and `Restore` is the hard part.** Every layer above must cope with being
  told it was wrong. The existing detector's `Crash` is consumed by exactly one line in each
  parent — `correct.remove(&node)` — and a `Restore` would have to put it back, which changes a
  monotone set into a non-monotone one. Every guard written against `correct` needs re-reading in
  that light.
- **Stable storage is the missing primitive.** `Sim::crash` genuinely loses volatile state, and `Ω`
  and epoch consensus both write things down that must survive an incarnation. There is nowhere to
  write them.
- **The `Propose`/`Decide` vocabulary held up** and cost nothing to establish. `Cmd::Start` before
  `Cmd::Propose` is the same shape uniform reliable broadcast already had, so a layer above can
  drive either without knowing which is beneath.
