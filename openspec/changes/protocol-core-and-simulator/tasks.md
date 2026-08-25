## 1. Workspace foundations

- [x] 1.1 Create the Cargo workspace on a current edition with the crate layout for core,
      simulator and protocols; verify `cargo build` and `cargo test` both succeed on an empty tree
- [x] 1.2 Add dependencies — seeded RNG, serde with a binary codec, `thiserror` — and verify
      `cargo tree` shows no async runtime and no networking crate
- [x] 1.3 Define the monotonic time type used in every signature that touches time, with
      construction from an arbitrary value; verify unit tests covering ordering and arithmetic
- [x] 1.4 Add a lint or CI check forbidding `HashMap`/`HashSet` in the protocol and simulator
      crates; verify it fails on a deliberately introduced `HashMap` and passes once removed

## 2. Protocol core

- [x] 2.1 Define the `Protocol` trait — handlers for command, message and timer events — and the
      effect vocabulary of send, indicate and timer; verify a trivial echo protocol compiles
      against it
- [x] 2.2 Implement the context that receives effects and supplies time and seeded randomness;
      verify a protocol drawing randomness twice from the same seed makes the same choice
- [x] 2.3 Add the test helper that runs one event against a scratch context and returns the
      emitted effects; verify assertions of the form "this event yields exactly these effects"
      read cleanly in a test
- [x] 2.4 Establish the composition pattern by which a parent owns a child and re-wraps its
      effects; verify with a two-layer toy stack whose child effects surface correctly re-wrapped
- [x] 2.5 Define per-layer error types with `thiserror`; verify no error path constructs an
      `io::Error` and every variant retains its underlying cause

## 3. Simulator

- [x] 3.1 Implement the virtual clock and the scheduled delivery queue with deterministic ordering
      of events sharing a timestamp; verify a timer far in the future fires without real waiting
- [x] 3.2 Implement the multi-process harness that runs a named set of processes in one thread;
      verify a scenario runs to completion opening no sockets
- [x] 3.3 Implement fault injection for loss, duplication, reordering and delivery delay; verify
      each knob changes the trace in the expected direction at the configured rate
- [x] 3.4 Implement partitions between named groups, including healing; verify no delivery crosses
      a partition while it holds and delivery resumes after it is removed
- [x] 3.5 Implement trace recording of sends, delivery outcomes, timer fires and indications with
      virtual time and originating process; verify a trace can be examined without touching
      protocol internals
- [x] 3.6 Implement seed-driven determinism end to end; verify the same seed yields byte-identical
      traces across runs and two different seeds are each individually reproducible
- [x] 3.7 Implement the opt-in codec-check mode that round-trips every delivery; verify it detects
      a deliberately broken round-trip and that the default path performs no encoding
- [x] 3.8 Model a crash as the loss of volatile state — fresh protocol and no pending timers on
      restart — with suspension as the opt-in alternative that preserves state; verify a restarted
      process re-delivers a message it had already delivered, and that a suspended one does not

## 4. Stubborn link

- [x] 4.1 Implement retransmission at a configured interval with an instruction to stop; verify
      unit tests showing repeated transmission and cessation on stop
- [x] 4.2 Assert stubborn delivery over the simulator; verify a message sent between correct
      processes under heavy loss is delivered, and again after a partition heals
- [x] 4.3 Assert no creation over the simulator; verify every delivery in the trace corresponds to
      an earlier send by the named sender

## 5. Perfect link

- [x] 5.1 Implement deduplication by message identifier over the stubborn link, composed as a
      child; verify the wire header carries exactly one identifier and nothing else
- [x] 5.2 Assert reliable delivery and no duplication over the simulator; verify every sent message
      is delivered exactly once under configured loss and duplication
- [x] 5.3 Assert that separately sent messages with identical content are both delivered; verify
      duplicate suppression does not swallow a genuine resend
- [x] 5.4 Unit-test the perfect link in isolation with a stand-in payload type; verify it compiles
      and passes without any layer above it existing

## 6. Best-effort broadcast

- [x] 6.1 Implement fan-out over perfect links, composed as a child, including self-delivery;
      verify the layer contributes no wire fields of its own
- [x] 6.2 Assert best-effort validity over the simulator; verify every correct process delivers a
      message broadcast by a correct sender, including when others have crashed
- [x] 6.3 Assert no duplication and no creation over the simulator; verify each process delivers
      each broadcast exactly once and only for messages actually broadcast
- [x] 6.4 Verify that a sender crashing partway through a broadcast produces no property violation

## 7. Verification of the method itself

- [x] 7.1 Run the full three-layer stack under combined faults; verify all stated properties of all
      three layers hold simultaneously across many seeds
- [x] 7.2 Add positive assertions that fault injection actually occurred — losses present at a
      configured loss rate, duplicates present when configured; verify the suite fails if the
      network silently stops injecting faults
- [x] 7.3 Add a guard that properties are not vacuously satisfied — assert minimum delivery counts
      alongside each absence-of-violation property; verify the suite fails against a protocol
      stubbed to deliver nothing
- [x] 7.4 Demonstrate the reproduce-from-seed workflow; verify a deliberately introduced defect
      reports a seed, and re-running that seed reproduces the identical failure

## 8. Recording what was learned

- [ ] 8.1 Record the composition boilerplate actually observed across the two re-wrap boundaries —
      what repeated, how many lines, and whether it was mechanical; deliver as notes in the change
      so the eventual macro decision rests on measurement rather than anticipation
- [ ] 8.2 Confirm the change opened no sockets and added no async runtime; verify by inspecting the
      dependency tree and searching the tree for socket and runtime references
- [ ] 8.3 Update `CLAUDE.md` with any standing rules this change establishes — at minimum the
      ordered-map requirement from task 1.4; verify the file reflects them
