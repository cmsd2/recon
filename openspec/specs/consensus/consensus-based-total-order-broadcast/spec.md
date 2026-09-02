# consensus/consensus-based-total-order-broadcast Specification

## Purpose

Cachin, Guerraoui & Rodrigues, Algorithm 6.1: a totally ordered sequence of entries, agreed by
running one consensus instance per round over what reliable broadcast has delivered but not yet
ordered. The crash-stop member of the pair behind the log port.

## Requirements

### Requirement: Every process sees the same sequence

Processes SHALL deliver entries in the same order. Where two processes have each delivered entries
at position `i`, those entries SHALL be equal.

This is the whole of what a totally ordered log claims, and unlike linearizability it is checkable
directly from the histories without searching for a witness.

#### Scenario: Two processes agree on their common prefix

- **WHEN** two correct processes have each delivered a sequence of entries
- **THEN** the shorter is a prefix of the longer

#### Scenario: The order does not depend on who appended

- **WHEN** several processes append concurrently
- **THEN** every process delivers those entries in one order, which need not be the order they were
  appended in

### Requirement: An entry appended by a correct process is eventually delivered everywhere

If a correct process appends an entry, every correct process SHALL eventually deliver it.

#### Scenario: An append reaches every correct process

- **WHEN** a correct process appends an entry and the run continues
- **THEN** every correct process eventually delivers it

#### Scenario: Nothing is delivered that was not appended

- **WHEN** a process delivers an entry
- **THEN** some process appended it

#### Scenario: No entry is delivered twice

- **WHEN** a process delivers an entry
- **THEN** it does not deliver that entry again

### Requirement: The order is agreed by consensus, one instance per round

The order SHALL be established by proposing the set of unordered entries to a consensus instance,
and by deterministically sorting what that instance decides. A new instance SHALL be used for each
round, and a round SHALL NOT begin while the previous one is undecided.

Sorting deterministically is what makes the decision enough: consensus agrees on a *set*, and every
process must turn that set into the same sequence without further communication.

#### Scenario: A round proposes what is not yet ordered

- **WHEN** a process has entries delivered by the broadcast beneath and not yet ordered, and no round
  is outstanding
- **THEN** it proposes them to a new consensus instance

#### Scenario: A decided set becomes an ordered run of entries

- **WHEN** a consensus instance decides a set of entries
- **THEN** every process delivers them in the same order, derived from the set alone

#### Scenario: Rounds do not overlap

- **WHEN** a round is outstanding
- **THEN** no further round is begun until it decides

### Requirement: The state is unbounded, and this is a transcription

The state SHALL be permitted to grow with the number of entries handled. The set of unordered
entries and the sequence of delivered entries both grow without bound, as the page has them.

This capability is a **transcription**: it renders the algorithm faithfully enough to be read against
the page and inherits the book's omissions, of which garbage collection is one. That is correct of a
transcription and disqualifying of an implementation, and `docs/bounded-space.md` requires the
distinction to be stated per module rather than assumed. Bounding either collection would weaken a
guarantee to a scope, and belongs to a change with a proposal.

#### Scenario: The module states its own space bound

- **WHEN** a reader consults the module's documentation
- **THEN** it says the state is unbounded and that the module is a transcription
