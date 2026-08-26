## 1. The interface in the core

- [x] 1.1 Add a storage handle to the context offering, synchronously: read and replace a metadata
      value, append an entry, read entries from a position, and ask for the end position; verify
      from the core suite that what is appended is readable back within the same event
- [x] 1.2 Replace the single durable type with a metadata type and an entry type; verify that a
      protocol declaring both uninhabited cannot construct a write, as a build-time check beside
      the existing scope one, and that reading from it is vacuous rather than an error
- [x] 1.3 Remove the store effect and make a write durable when it returns, so the ordering rule
      stops being a driver obligation; verify a write that returned survives any later crash, and
      that a send with no write before it costs nothing
- [x] 1.4 Change recovery to be told it is recovering rather than handed a value; verify a recovery
      handler reads its state and acts on it within the handler
- [x] 1.5 Update every protocol to declare the two types — all but two declare them uninhabited —
      and verify the whole suite passes unchanged

## 2. The interface in the simulator

- [x] 2.1 Give each process a metadata slot and an append-only log, both part of the seeded state;
      verify what was written and appended is readable after a restart, in order
- [x] 2.2 Add a way to arm the interrupted-write fault, since durable-on-return leaves no window
      for one to happen by timing; verify that a process killed inside a write emits nothing the
      handler decided afterwards, and that a later write in the same handler never lands
- [x] 2.3 Extend the interrupted-write fault to appends; verify across seeds that an outstanding
      append may or may not survive, that a completed one always does, and that what survives is
      always a prefix — never a sequence with a hole in it
- [x] 2.4 Distinguish rewriting from appending in the trace; verify a claim about write cost can be
      checked from the trace rather than from protocol state
- [x] 2.5 Verify a run involving writes, appends, crashes and recoveries reproduces from its seed

## 3. The invariant that makes this safe

- [x] 3.1 Verify nothing is dispatched to a process between entering its initialisation or recovery
      handler and that handler returning: restart a process with messages already in flight and
      assert none was handled before recovery returned
- [x] 3.2 Verify a recovery handler that reads and acts on what it finds is not interrupted, so
      that a protocol may hold state it has not yet loaded

## 4. Logged perfect links, converted to appending

- [x] 4.1 Convert the durable record from a rewritten set to an appended log with a metadata slot;
      verify every existing property in the suite still holds, unchanged
- [x] 4.2 Verify recording one message costs one append and rewrites nothing, from the trace
- [x] 4.3 Verify the write cost is linear: log-deliver many messages and assert the number of
      entries appended equals the number log-delivered
- [x] 4.4 Verify recovery reads the record within the handler, and that a retransmission arriving
      immediately afterwards is recognised rather than log-delivered again — the property this
      protocol exists for, now depending on the read having completed
- [x] 4.5 Update the module's space statement: unbounded, but appended once rather than rewritten

## 5. Logged uniform reliable broadcast, converted to appending

- [x] 5.1 Convert pending and log-delivered from rewritten sets to appended entries with a metadata
      slot, leaving acknowledgements volatile as before; verify every existing property still holds
- [x] 5.2 Verify recording one message costs one append, from the trace
- [x] 5.3 Verify recovery reads what survived within the handler, re-announces it, and re-broadcasts
      what was pending — with nothing dispatched in between
- [x] 5.4 Verify acknowledgements are still never written, and are still rebuilt by re-broadcasting
- [x] 5.5 Update the module's space statement

## 6. Recording it

- [x] 6.1 Update `docs/bounded-space.md`: the quadratic write cost is historical for these two, the
      remaining growth is the record itself and the in-memory index, and per-sender ordering is what
      would bound the second
- [x] 6.2 Update the README's core section — the effect vocabulary is three again, and storage is
      supplied like time and randomness — and refresh the test counts
- [x] 6.3 Record as notes: whether one interface for reading, writing and appending was the right
      consolidation; whether keeping the uninhabited-type check was worth the two associated types;
      and whether the storing-child restriction still looks like the right thing to defer
