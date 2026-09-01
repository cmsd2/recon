## ADDED Requirements

### Requirement: A run can be described as a value and executed from it

A run SHALL be expressible as data — a configuration and its seed, a membership, a sequence of steps
each with the time it occurs, and a horizon to run to — and the simulator SHALL execute such a
description. Every fault and command the simulator accepts imperatively SHALL be expressible as a
step.

Executing the same description SHALL produce the same run, by the determinism this capability
already requires.

#### Scenario: A described run and an imperative one agree

- **WHEN** the same commands and faults are applied at the same times, once by calling the
  simulator's methods and once by executing a description
- **THEN** the two runs produce the same trace

#### Scenario: A description executes identically every time

- **WHEN** one description is executed twice
- **THEN** the two runs produce the same trace

### Requirement: A failing scenario can be reduced to a smaller one that still fails

Given a scenario and a predicate over the run it produces, the simulator SHALL search for a smaller
scenario whose run still satisfies that predicate, and SHALL return one from which no further
reduction it attempts can be made.

Smaller SHALL mean fewer steps, a shorter horizon, a smaller membership, or a simpler fault — the
reductions that make a counterexample readable.

The result SHALL satisfy the predicate. A reduction that is not re-run and re-checked is a guess,
and returning a scenario that does not fail would be worse than returning the original.

The search SHALL itself be deterministic, so that a reduction can be reproduced and reported.

#### Scenario: A scenario with irrelevant steps loses them

- **WHEN** a scenario contains faults and commands that the predicate does not depend on
- **THEN** the returned scenario no longer contains them

#### Scenario: The horizon shrinks to when the predicate first holds

- **WHEN** a scenario runs far beyond the point at which its predicate becomes true
- **THEN** the returned scenario's horizon is near that point rather than the original

#### Scenario: What is returned still fails

- **WHEN** a scenario is reduced
- **THEN** running the returned scenario satisfies the predicate

#### Scenario: Reducing twice gives the same answer

- **WHEN** the same scenario and predicate are reduced twice
- **THEN** the two results are identical

#### Scenario: An already-minimal scenario is returned unchanged

- **WHEN** no reduction the search attempts leaves the predicate holding
- **THEN** the original scenario is returned, rather than something that no longer fails

### Requirement: A reduced scenario is reported as something that can be run again

The reduced scenario SHALL be renderable as source that reconstructs it, so that the end of a
reduction is a test rather than a description to transcribe by hand.

#### Scenario: The rendering reconstructs the scenario

- **WHEN** a scenario is rendered and the rendering is compiled and executed
- **THEN** it produces the same run as the scenario it was rendered from
