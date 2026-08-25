## 1. The scope mechanism

- [x] 1.1 Add the scope associated type and its handler to `Protocol`, and declare the uninhabited
      scope on all six existing protocols; verify each gains one line and no handler, and that the
      whole suite passes unchanged
- [x] 1.2 Verify a scope ending cannot be constructed for a protocol that declares it has none —
      a compile-fail test, or an equivalent argument recorded against the uninhabited type
- [x] 1.3 Extend composition so a parent can map a child's scope ending into its own, or absorb it;
      verify a toy two-layer stack where the parent bridges, and one where it propagates

## 2. The session network model

- [x] 2.1 Add a session configuration to the simulator, with a session per unordered pair and an
      epoch; verify a run reports the current epoch for a pair and that it starts consistent at
      both ends
- [x] 2.2 Implement reliable ordered delivery within a session; verify over the trace that every
      message is delivered exactly once and never before one sent earlier on the same pair
- [x] 2.3 Implement session ends on explicit break, partition and crash, discarding an unknown
      suffix of what was in flight; verify each cause ends the session and that messages in flight
      are lost
- [x] 2.4 Verify the suffix is genuinely unknown: across many seeds the number lost varies, and
      both "everything in flight" and "nothing in flight" occur — a model that always did one would
      pass a loose test while modelling nothing
- [x] 2.5 Implement a new session at a higher epoch once communication is possible, with ordering
      restarting; verify epochs increase and that post-break ordering is independent of what was lost
- [x] 2.6 Record session establishment, ends and suffix losses in the trace, distinguishable from
      ordinary delivery; verify a property can be asserted over them without touching protocol state
- [x] 2.7 Verify determinism in the new mode: the same seed produces a byte-identical trace, and
      differing seeds explore differing schedules — the queue change must not weaken this
- [x] 2.8 Verify the fair-loss default is untouched: the existing simulation suite passes unmodified
      and still shows loss, duplication and reordering

## 3. The session link

- [x] 3.1 Implement a link whose guarantees come from the session, holding an epoch per peer and
      nothing per message; verify its state does not grow as the number of messages sent grows
- [x] 3.2 Assert reliable ordered delivery and no duplication within a session, and no creation
      across a whole run
- [x] 3.3 Assert that a session ending is reported to the layer above with the peer and the new
      epoch, and that a lost suffix is never reported as delivered
- [x] 3.4 Assert delivery resumes normally in the new session, and that no delivered message was
      sent in a different session from the one it was delivered in

## 4. What it cost

- [x] 4.1 Record what the scope mechanism cost the five protocols that do not use it, and whether
      the uninhabited declaration was as cheap in practice as it was on paper
- [x] 4.2 Record what per-pair ordering cost the delivery queue, and whether determinism needed
      anything beyond the existing seeding; deliver as notes in the change
