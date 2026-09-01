## 1. Core helpers

- [x] 1.1 Add `recon_core::Child<P>` — a protocol with its indication inbox — with `run` and
      `run_durable` returning the filled inbox by value and `reclaim` putting it back, and verify
      in `core_contract` that a composition through it emits exactly what the hand-written form does
- [x] 1.2 Add `slot!(Parent, field)` and use it for the existing slots
- [x] 1.3 Add `Sim::at(node) -> &P`, panicking with the node's name
- [x] 1.4 Add `recon_protocols::Timing` and take it in the four leader-driven constructors

## 2. The bound

- [x] 2.1 Reply once per `READ` and once per `WRITE` in `logged_epoch_consensus`; state the
      departure and why the stubborn reply makes it safe, including across a leader's recovery
- [x] 2.2 Refuse each distinct announcement once per peer in `logged_epoch_change`, bounded by
      membership; state the departure
- [x] 2.3 Add a growth test to each of the seven modules `single-instance-paxos` added — send count
      per window flat across a run several times longer than the work takes — and confirm the two
      logged consensus ones fail before the fix and pass after

## 3. Tests the last change owed

- [x] 3.1 Spend `crash_on_next_write` on `logged_epoch_change`'s epoch write: both outcomes across
      seeds, no `StartEpoch` from the doomed handler, the epoch reached regardless
- [x] 3.2 Spend it on `logged_leader_driven_consensus`'s decision write, armed once a process has
      accepted and not yet decided; both outcomes; the recovery re-announce path is the one that
      makes the "landed" outcome visible
- [x] 3.3 Read disputed leadership from the trace in `leader_driven_consensus`: two processes each
      sent a leader-only message, and the earlier was still originating messages after the later
      began. Replace the end-of-run `leaders_seen` proxy in both tests that use it.
      **The reading as written came out at 0 in 40 runs.** An epoch's leader acts for a few
      milliseconds and a rival emerges after a detector timeout, so leaders almost never originate
      messages at the same instant. The reading that matches the safety argument is: a rival's first
      `READ` precedes the old epoch's `DECIDED` reaching every process — some process may still be
      asked to accept the old write while the new leader reads. That is what both suites now check,
      from the trace, in the volatile and the logged Paxos alike

## 4. Migration

- [x] 4.1 Rewrite every composite module over `Child<P>`, one at a time, running its suite after
      each; no behavioural change, and `alloc_probe` still passes
- [x] 4.2 Move `A..E`, `ALL`, `BOUND` and the timing functions of the eight new suites into
      `tests/common/mod.rs`

## 5. What this dates

- [x] 5.1 `docs/bounded-space.md`: the two logged consensus rows lose their warning for *work*,
      keep it for the outstanding set, and the measurement above is recorded
- [x] 5.2 `README.md`: suite counts, and the composition description
- [x] 5.3 `CLAUDE.md`: the composition convention names `Child<P>`; the space-bound practice says
      the growth test is a *send rate* test as well as a state test
- [x] 5.4 `./scripts/check.sh` passes in full
