## ADDED Requirements

### Requirement: A synchronous mode with a bounded delivery delay

The simulator SHALL offer a mode in which every message between connected, uncrashed processes is
delivered within a known upper bound and none is lost. The bound SHALL be readable by a test, so
that a protocol depending on it can be configured consistently with the network it runs on.

This mode is additional. The fair-loss behaviour remains the default, and the existing
configuration of loss, duplication, reordering and latency is unchanged.

#### Scenario: Delivery within the bound

- **WHEN** a run is configured synchronous with bound Δ and a message is sent between two
  connected, uncrashed processes
- **THEN** it is delivered, and the delay between sending and delivery does not exceed Δ

#### Scenario: No loss in synchronous mode

- **WHEN** a run is configured synchronous
- **THEN** no message between connected, uncrashed processes is dropped

#### Scenario: Crashes and partitions still apply

- **WHEN** a run is configured synchronous and a process is crashed, or two processes are
  partitioned
- **THEN** messages to the crashed process and across the partition are still not delivered, so
  the mode constrains timing without removing failures

#### Scenario: The default remains asynchronous

- **WHEN** a run is configured without requesting synchronous mode
- **THEN** loss, duplication and latency behave exactly as before
