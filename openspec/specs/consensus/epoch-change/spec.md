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

#### Scenario: Epochs stop changing once leadership settles

- **WHEN** the leader detector settles on one correct process
- **THEN** every correct process eventually starts the same final epoch and no further one

#### Scenario: Processes may be in different epochs meanwhile

- **WHEN** the leader detector has not yet settled
- **THEN** correct processes may be in different epochs, and this capability is satisfied

### Requirement: An epoch starts only when leadership changes

A new epoch SHALL be started only in response to a change in the trusted leader, and not on a timer
or on ordinary message traffic.

An epoch change costs the layer above an abort and a restart, so an epoch beginning for any other
reason is pure loss.

#### Scenario: A steady leader produces no new epochs

- **WHEN** the trusted leader is unchanged over a long run
- **THEN** no further epoch is started
