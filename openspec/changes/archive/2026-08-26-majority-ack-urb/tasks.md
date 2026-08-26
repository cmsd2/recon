## 1. Majority-ack over best-effort broadcast

- [x] 1.1 Transcribe Algorithm 3.5 from `uniform_reliable_broadcast`: the same algorithm with
      `candeliver` replaced by the majority predicate and the failure detector, its heartbeats, its
      timer arm, its wire arm, `correct` and `Cmd::Start` all removed; quote the changed function
      and state the status and the assumption it now rests on. Verify a five-process run with no
      faults delivers everywhere
- [x] 1.2 Drop the wire enum, the message type now being the broadcast child's directly; verify
      from the trace that every message sent is a broadcast payload and no heartbeat appears
- [x] 1.3 Verify the predicate's boundary exactly: with one relayer short of a majority nothing is
      delivered, and with one more it is — the originator's own relay counting like any other.
      **Pinned at an even membership as well as five**: with an odd `N`, `2k > N` and `2k >= N` are
      the same predicate, so the off-by-one that would let exactly half suffice is invisible there
- [x] 1.4 Register the module in `crates/recon-protocols/src/lib.rs`; verify `./scripts/check.sh`
      passes

## 2. The four guarantees, without a detector

- [x] 2.1 Verify validity: a correct sender delivers its own broadcast, and a minority crashing
      does not prevent it, stating how many correct processes remain
- [x] 2.2 Verify uniform agreement including when a process delivers and then crashes
- [x] 2.3 Verify no duplication and no creation, with deliveries attributed to the originator
      rather than a relayer
- [x] 2.4 Verify uniform agreement holds with **no timing assumption at all** — the asynchronous
      default with loss, latency jitter and reordering, where the all-ack version's detector would
      accuse the living
- [x] 2.5 Verify no start command is required: broadcast into a fresh run with no prior request
      made of this layer and confirm delivery

## 3. The contrast with all-ack

- [x] 3.1 Run the schedule that breaks the all-ack version's uniform agreement against this one,
      with five processes split three and two, and verify it does not split
- [x] 3.2 Verify that assertion is not vacuous: assert the majority side delivered before asserting
      that no two processes disagree, so a run that delivered nothing cannot pass
- [x] 3.3 Verify the difference is attributable: no process is ever excluded, because no
      failure-detection message is ever sent and no set of believed-correct processes exists
- [x] 3.4 Verify the minority side catches up once the partition heals, so blocking was a pause
      rather than a permanent divergence

## 4. Majority-ack over session links

- [x] 4.1 Apply the same predicate over `session_best_effort_broadcast`, keeping the resend clause
      and dropping the detector; verify a five-process run with sessions holding delivers
      everywhere
- [x] 4.2 Verify the wire no longer multiplexes and no failure-detection message is sent, this
      layer having one child
- [x] 4.3 Verify validity and uniform agreement across session endings and re-establishment, over
      several seeds with repeated breaks
- [x] 4.4 Verify the resend path: a session ending loses a suffix, re-establishment sends what the
      peer missed, and nothing is attempted on the ending itself
- [x] 4.5 Verify a resend goes only to the peer whose session returned
- [x] 4.6 Verify a peer that never returns needs no accusation: with a majority still reachable,
      the pending messages are delivered and no judgement is made about the absent peer
- [x] 4.7 Verify a peer absent for far longer than any timeout the all-ack version would have used
      is not treated as a stranger when it returns — it receives what it missed and delivers it,
      with no exclusion to undo
- [x] 4.8 Verify both session reports still reach the layer above, distinguishably

## 5. Where the assumption fails

- [x] 5.1 Verify a minority partition delivers nothing new rather than delivering something the
      majority will never deliver
- [x] 5.2 Verify the majority side continues to deliver throughout that partition
- [x] 5.3 Verify that with half or more of the processes crashed the layer blocks rather than
      diverges, and that this is a different failure from the all-ack version's split — assert the
      absence of any inconsistent delivery, not merely the absence of progress

## 6. Recording it

- [x] 6.1 Add both rungs to the README's protocol tables with their status and space bound, and say
      what the trade is; verify the relative links still resolve and the test counts are current
- [x] 6.2 Record as notes in the change: whether the majority versions really did come out smaller,
      what the five-process requirement cost, and whether "no detector" proved assertable from the
      trace rather than from the struct
