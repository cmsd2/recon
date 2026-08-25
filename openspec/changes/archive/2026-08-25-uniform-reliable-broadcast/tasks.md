## 1. The protocol

- [x] 1.1 Define the wire type multiplexing broadcast payloads and heartbeats as an enum, and the
      identifier carrying originator and sequence; verify both round-trip through the codec and
      that a broadcast payload and a heartbeat are distinguishable on the wire
- [x] 1.2 Compose the two children as owned fields with a helper each, and the module documentation
      quoting Algorithm 3.4 with the timing assumption stated where a reader meets it; verify the
      layer builds against both children without either knowing about the other
- [x] 1.3 Implement broadcast, relay on first receipt, and acknowledgement tracking; verify unit
      tests of the handlers' effects, including that a repeat receipt records the acknowledgement
      but does not relay again
- [x] 1.4 Implement the delivery condition as a function called wherever its inputs change, and
      crash indications shrinking the correct set; verify a message becomes deliverable when the
      last outstanding acknowledgement arrives, and separately when the process being waited on is
      detected as crashed

## 2. Its guarantees

- [x] 2.1 Assert validity and no duplication over the simulator in synchronous mode, with the
      detector configured from the network's own bound
- [x] 2.2 Assert no creation, including that a message reaching a process by relay is attributed to
      its originator
- [x] 2.3 Assert that delivery is withheld until every correct process has acknowledged, and that a
      crash of the process being waited on unblocks it
- [x] 2.4 Assert uniform agreement in the case that defines this rung: a process delivers and then
      crashes immediately, and every survivor still delivers — across many seeds
- [x] 2.5 Assert uniform agreement through a partition and its healing

## 3. That the tests distinguish this rung

- [x] 3.1 Verify reliable broadcast run through the same deliver-then-crash scenario does violate
      uniform agreement, and that uniform reliable broadcast survives the seed that breaks it — as
      the reliable-broadcast suite does against best-effort broadcast
- [x] 3.2 Verify the agreement assertions are not vacuous: assert minimum delivery counts, and
      confirm a run in which nothing is delivered would fail
- [x] 3.3 Verify the dependency on accurate detection is real: withdraw the timing assumption by
      running the same scenario on a lossy network, and confirm the guarantee can break — so the
      synchronous mode is shown to be load-bearing rather than incidental

## 4. What it cost

- [x] 4.1 Read the two composition helpers against each other and decide whether they justify
      generation, recording the decision either way with the evidence; this is the layer the
      reliable-broadcast notes named as what would reopen the macro question
- [x] 4.2 Record whether `recon-core` or `recon-sim` needed anything for a layer with two children
      and a state-predicate delivery condition, and confirm no transport or async runtime entered
      the tree; deliver as notes in the change
