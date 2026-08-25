## 1. The simulator's synchronous mode

- [x] 1.1 Add a synchronous configuration carrying a delivery bound, which disables loss and
      duplication and fixes delivery within that bound; verify a run reports the bound back so a
      protocol can be configured from the same value
- [x] 1.2 Verify no message between connected, uncrashed processes is dropped in this mode, and
      that every delivery's delay is within the bound, asserted over the trace
- [x] 1.3 Verify crashes and partitions still prevent delivery in this mode — it constrains timing,
      not failure
- [x] 1.4 Verify the default is unchanged: an unconfigured run still loses, duplicates and jitters
      exactly as the existing simulation suite expects, with that suite passing untouched

## 2. The detector

- [x] 2.1 Define the heartbeat message and the module documentation quoting Algorithm 2.5, with the
      timing assumption stated where a reader meets it; verify the message round-trips through the
      codec
- [x] 2.2 Implement heartbeat emission on a period and timeout-based exclusion; verify unit tests
      of the handlers' effects, including that a heartbeat received before the timeout rearms it
- [x] 2.3 Verify a crash indication is raised exactly once per crashed process and never retracted

## 3. Its guarantees

- [x] 3.1 Assert strong completeness in synchronous mode: every crashed process is detected by
      every survivor within a stated multiple of the bound, including when several crash
- [x] 3.2 Assert strong accuracy in synchronous mode: with every process correct, no crash is ever
      indicated however long the run continues, across many seeds
- [x] 3.3 Verify a process suspended for less than the detection bound is not accused, and one
      suspended for longer is — the boundary the timeout actually tests
- [x] 3.4 Verify accuracy is lost when the assumption is withdrawn: the same detector on a lossy
      network does accuse a correct process. This confirms the synchronous mode is load-bearing
      rather than incidental, in the manner of the previous change's method tests

## 4. What it cost

- [x] 4.1 Record what the first change to `recon-sim` since it was built required, and whether the
      `Protocol` trait needed anything for a protocol whose output is indications about processes
      rather than delivered payloads; deliver as notes in the change
