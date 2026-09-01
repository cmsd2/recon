# consensus/epoch-change Specification

## Purpose

A sequence of epochs, each with a timestamp and a leader. It is what turns an eventual leader
detector into something an abortable consensus can be driven by: when leadership changes, the epoch
advances, and the layer above knows to start again.

## Requirements

### Requirement: Epoch timestamps increase and never repeat

Every epoch a process starts SHALL carry a timestamp strictly greater than the last it started, and
no two epochs started at one process SHALL share a timestamp.

The timestamp is what orders the algorithm above: a value written in a later epoch supersedes one
written earlier, and a repeated or decreasing timestamp would make that ordering meaningless.

#### Scenario: Timestamps advance

- **WHEN** a process starts a sequence of epochs
- **THEN** each timestamp is strictly greater than the one before it

#### Scenario: One timestamp names one leader

- **WHEN** two processes start an epoch with the same timestamp
- **THEN** they name the same leader

### Requirement: Eventually all correct processes settle on one last epoch

There SHALL be a time after which every correct process has started the same epoch, with the same
timestamp and the same correct leader, and starts no further one.

Before then, processes MAY be in different epochs, and nothing above may assume otherwise.

This SHALL hold when the detector beneath **withdraws** a suspicion as well as when it raises one.
A process that becomes trusted by others without its own trusted process changing is told nothing by
an edge-triggered leader detector, and so announces nothing; the processes that now trust it may
meanwhile have run their epochs far ahead of its own. Settling therefore SHALL NOT depend on the
leader having observed a change.

#### Scenario: Epochs stop changing once leadership settles

- **WHEN** the leader detector settles on one correct process
- **THEN** every correct process eventually starts the same final epoch and no further one

#### Scenario: Processes may be in different epochs meanwhile

- **WHEN** the leader detector has not yet settled
- **THEN** correct processes may be in different epochs, and this capability is satisfied

#### Scenario: Leadership returning to a process that never lost its own trust

- **WHEN** processes that ran their epochs ahead under other leaders come to trust a process which
  trusted itself throughout, and which is therefore in a much earlier epoch
- **THEN** they eventually all start a common epoch led by it

### Requirement: An epoch starts only when leadership changes

A new epoch SHALL be started only in response to a change in the trusted leader, and not on a timer
or on ordinary message traffic.

An epoch change costs the layer above an abort and a restart, so an epoch beginning for any other
reason is pure loss. A process whose trusted leader changes to one that is not itself, and whose
current epoch is led by some other process, MAY tell that leader where it has reached — that is a
consequence of leadership changing rather than an independent cause, and it SHALL NOT be sent while
nothing has changed.

#### Scenario: A steady leader produces no new epochs

- **WHEN** the trusted leader is unchanged over a long run
- **THEN** no further epoch is started

#### Scenario: A settled stack is silent about epochs

- **WHEN** every process is in the same epoch and the trusted leader is unchanged
- **THEN** no process tells the leader anything about where it has reached

### Requirement: A leader is told where the processes trusting it have reached

A process that trusts a leader other than itself, while in an epoch that leader did not start, SHALL
tell that leader the timestamp it has reached. A leader so told, and which trusts itself, SHALL
choose its next candidate above that timestamp.

Without this a leader can be trusted by everyone and never learn it: an eventual leader detector
raises its indication when the trusted process *changes*, so a process that trusted itself all along
is never told, never announces, and the epochs of those now trusting it stay ahead of anything it
would announce. Retransmission does not rescue it, because what it would retransmit is an
announcement its recipients have already deduplicated.

Choosing the next candidate *above* the timestamp it was told, rather than one step past its own,
SHALL also be how a leader catches up: stepping by the membership size once per refusal costs a
round trip per step, where the gap can be arbitrarily large.

#### Scenario: A leader that never observed a change still starts an epoch

- **WHEN** a process is trusted by every other process, has trusted itself throughout, and is in an
  earlier epoch than any of them
- **THEN** it starts an epoch above theirs, and they enter it

#### Scenario: The leader catches up in one step, not one step per refusal

- **WHEN** a leader is told of a timestamp far above its own
- **THEN** its next candidate is above that timestamp

#### Scenario: A stale report does not move a leader that has already passed it

- **WHEN** a leader is told of a timestamp no greater than the candidate it has already chosen
- **THEN** it announces nothing further, so that repeated reports cannot drive it without bound
