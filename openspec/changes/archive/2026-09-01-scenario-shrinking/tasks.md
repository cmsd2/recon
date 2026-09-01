## 1. A run as data

- [x] 1.1 `Scenario` — configuration with its seed, membership, timed steps, horizon — and `Step`
      covering every fault and command the simulator accepts: commands, crash, restart, suspend,
      resume, crash-on-next-write, sever, reconnect, partition, heal, break-session
- [x] 1.2 `Sim::run_scenario`, executing a description against a protocol constructor
- [x] 1.3 Verify a described run and the equivalent imperative one produce the same trace — the
      equivalence the rest of this change rests on
- [x] 1.4 Verify one description executed twice produces the same trace

## 2. The reduction

- [x] 2.0 A reduction repairs the pairing it breaks: a `Resume` without its `Suspend`, or a
      `Restart` without its `Crash`, is a run the simulator refuses outright, so a reduction that
      did not repair would spend most of its candidates on panics. Not anticipated when this was
      planned; it cost a failing test to find
- [x] 2.1 `shrink(scenario, predicate)`, running each candidate and keeping it only when the
      predicate still holds. State in the module that a reduced scenario is a **different run** that
      also fails, not a prefix of the original: removing a step changes what every later draw takes
      from the generator
- [x] 2.2 Horizon first, by binary search to the earliest that still fails — the reduction that
      answers *when*, which is where a hand-written probe starts
- [x] 2.3 Steps by delta-debugging rather than one-at-a-time: faults interact here, and removing
      either of a crash and the partition isolating its quorum can stop the failure where removing
      both would not have been tried
- [x] 2.4 Fault detail, then membership last, since dropping a process changes quorum arithmetic and
      a bug that survives it is a much better counterexample than one that does not
- [x] 2.5 Verify termination, and that the search is deterministic — the same scenario and predicate
      reduce to the same result twice. A reduction nobody can reproduce is one nobody can check

## 3. What it must not do

- [x] 3.1 Verify the returned scenario **still satisfies the predicate**. Returning something that
      no longer fails is worse than returning the original
- [x] 3.2 Verify an already-minimal scenario comes back unchanged rather than degraded
- [x] 3.3 Verify a reduction actually reduces: a scenario padded with irrelevant faults and a long
      horizon loses them. The non-vacuity half — a shrinker that returns its input is no shrinker,
      and one demonstrated only where it cannot help proves nothing

## 4. Against a defect this project actually had

- [x] 4.1 Reintroduce one fixed defect behind a test-only switch. `logged_epoch_consensus`'s
      reply-per-redelivery is the best candidate: it is one flag, its symptom is a measurable send
      rate rather than a panic, and the original diagnosis took a hand-written probe
- [x] 4.2 Write the predicate as the property — work bounded by membership rather than by time — and
      reduce a scenario that exhibits it. Record what came out and how long it took
- [x] 4.3 Judge the result honestly against the hand-written probe that originally found it, and
      write that judgement down whichever way it goes. If the reduction is not more use than the
      probe, that belongs in the archive as much as a success would

## 5. Reporting

- [x] 5.1 Render a `Scenario` as Rust that reconstructs it
- [x] 5.2 Verify the rendering round-trips: rendered, then compiled and run, produces the same trace
- [x] 5.3 The report names the predicate used, since a reduction can legitimately land on a
      different failure than the one it started from

## 6. What this dates

- [x] 6.1 `README.md`'s roadmap: correct the claim that a shrinker "would have handed those over"
      for the three diagnoses it names — it would have handed over none of them, and the reason is
      worth keeping rather than quietly deleting. Mark `E` built and say what it actually buys
- [x] 6.2 The suite table and counts
- [x] 6.3 `./scripts/check.sh` passes in full
