## Purpose

Tells a process which other processes have crashed, with no false positives and no permanent
omissions. It is where a timing assumption first enters the system: perfect detection is only
possible when message delivery has a known upper bound.

## ADDED Requirements

### Requirement: Strong completeness

Every process that crashes SHALL eventually be detected as crashed by every correct process, and
SHALL remain detected thereafter.

#### Scenario: A crashed process is detected

- **WHEN** a process crashes and the run continues long enough for the detection bound to elapse
- **THEN** every process that has not crashed indicates that process as crashed

#### Scenario: Detection is permanent

- **WHEN** a process has been detected as crashed
- **THEN** it is never subsequently reported as correct, and the indication is not repeated

#### Scenario: Several crashes are all detected

- **WHEN** more than one process crashes
- **THEN** every surviving process eventually detects all of them

### Requirement: Strong accuracy

A process SHALL be detected as crashed only if it has actually crashed.

#### Scenario: A slow but live process is not accused

- **WHEN** every process remains correct and the run proceeds under the timing assumption the
  detector requires
- **THEN** no process is ever detected as crashed

#### Scenario: A suspended process is not treated as crashed once it resumes

- **WHEN** a process is suspended for less than the detection bound and then resumes
- **THEN** it is not detected as crashed

### Requirement: The detector's guarantees are conditional on bounded delivery

The detector SHALL state the timing assumption under which its properties hold: that a message
sent between correct processes is delivered within a known bound. Where that assumption does not
hold, strong accuracy SHALL NOT be claimed.

#### Scenario: Accuracy is lost when the assumption is withdrawn

- **WHEN** the same detector is run against a network that loses messages or exceeds the bound
- **THEN** a correct process may be detected as crashed, and this is a violation of the assumption
  rather than of the implementation

### Requirement: Detection is reported once per process

The detector SHALL raise exactly one indication per crashed process to the layer above.

#### Scenario: A single indication per crash

- **WHEN** a process crashes and remains crashed
- **THEN** exactly one crash indication naming it is raised by each surviving process
