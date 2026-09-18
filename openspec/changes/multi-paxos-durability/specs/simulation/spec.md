## ADDED Requirements

### Requirement: Storage reports how many writes it has acknowledged

A process's storage SHALL report the number of writes — replacements and appends together — that
have returned to the process. The count SHALL rise by exactly one per write that returned, SHALL be
part of the run's deterministic state, and SHALL survive a restart along with what was written. A
write interrupted by a crash SHALL count if and only if it took effect.

The count is what a process announces to its peers on recovery, so that a peer can tell whether it
has seen a write the process no longer has.

#### Scenario: The count survives a restart

- **WHEN** a process writes several times, crashes, and restarts with its state
- **THEN** the count it reads is the number of writes that had returned

#### Scenario: An interrupted write counts only if it took effect

- **WHEN** a process is armed to die inside its next write and restarts, across many seeds
- **THEN** in every run the count equals the number of writes whose effect is present, whichever
  way the seed decided

### Requirement: A restart can lose storage, as a fault, and the trace says so

The simulator SHALL offer two restarts besides the ordinary one. One returns a process with nothing
in storage, as if its disk were new. The other returns it with the last `n` acknowledged writes
gone and the store otherwise complete, as a disk that acknowledged what it had not kept. Both SHALL
be recorded in the trace, distinguishably from an ordinary restart and naming what was lost.

The ordinary restart SHALL keep everything acknowledged; the build feature that wipes storage on
every restart remains, for the guard that asks every existing test at once whether it depends on
storage.

These faults are held to the same invariant as every simulator capability: the run cannot lose the
writes without a process raising the event that says so. The simulator cannot raise it — nothing in
a store can know what it has forgotten — so the requirement on the simulator is that the loss is in
the trace, and the requirement that a protocol detects it is the protocol's.

#### Scenario: An empty restart takes the first-start branch

- **WHEN** a process that has written is restarted with nothing in storage
- **THEN** its initialisation entry point runs, its acknowledged-write count is zero, and the trace
  records that the restart lost everything

#### Scenario: A truncated restart drops exactly the last writes

- **WHEN** a process that has written `k` times is restarted with `n` writes truncated
- **THEN** it recovers with the first `k − n` writes present and the rest absent, its count is
  `k − n`, and the trace records that `n` were lost

#### Scenario: A truncated restart is indistinguishable from the process's own vantage

- **WHEN** a process is restarted truncated
- **THEN** what it reads is a valid earlier state — whole values, a prefix of the sequence — and
  nothing it can read says anything was lost

#### Scenario: An ordinary restart loses nothing

- **WHEN** a process is restarted the ordinary way
- **THEN** every acknowledged write is present and the count is unchanged
