# What the sixth rung cost

Task 4.2.

## `recon-core` needed nothing. `recon-sim` needed nothing.

The previous change needed a substantial addition to the simulator, which was a signal that three
protocols had not settled what the network model required. This one — the first layer with two
children, a multiplexed wire, and a delivery condition that is a predicate over state rather than
an event — needed no change to either crate.

The `Protocol` trait absorbed all three novelties without alteration:

- **Two children** compose through the same `Cx::with_child_consuming` a single child uses. Nothing
  in the core knew or cared that there were two.
- **Multiplexing** is just a wider `Msg` enum, with the two children's mappers being variant
  constructors. `Wire::Broadcast` and `Wire::Detector` are ordinary `fn` pointers, which is what
  the composition API already takes.
- **The state-predicate delivery condition** is a private method called where its inputs change.
  It needed no new effect, no new event, and no scheduling.

## One friction worth recording

`BebMsg<P>` was first written as `<BestEffortBroadcast<Data<P>> as Protocol>::Msg`, which reads
better but is only well-formed where `Data<P>: Clone` — so the bound propagated into the wire enum
and every use of it. Writing the concrete type instead fixes that, at the cost of restating a
choice that belongs to the child.

The compromise kept is a compile-time assertion that the two agree:

```rust
const _: () = {
    fn _beb_msg_is_what_we_say_it_is<P: Clone>(m: BebMsg<P>)
        -> <BestEffortBroadcast<Data<P>> as Protocol>::Msg { m }
};
```

If best-effort broadcast ever changes what it puts on the wire, this stops compiling. It is a
small thing, but it is the kind of drift that would otherwise be found by a decoding failure at
runtime — which is the class of bug this project exists to avoid.

## Constraint 1 still holds

29 crates, no async runtime, no networking crate, no socket type and no `.await` anywhere in the
tree. `scripts/check-no-transport.sh` passes, as it has since it was written.

Six rungs of the ladder now run entirely in a single test process: stubborn link, perfect link,
best-effort broadcast, reliable broadcast, perfect failure detector, uniform reliable broadcast.
The first attempt reached none of them in seventeen months.

## Two tests that needed fixing, and what they showed

**Reliable broadcast does not split in synchronous mode.** The distinguishing test was first
written as broadcast-then-crash-the-sender, which never splits when nothing is lost: best-effort
broadcast sends to everyone in one step, so a crash cannot catch it partway. The scenario that
does separate the rungs is the book's own Figure 3.3 — the processes that deliver crash before
their relays escape — engineered with a partition that heals only after both deliverers are gone.

**A partition is how the timing assumption actually fails.** The test that withdraws synchrony was
first written with packet loss, and never produced a violation: with perfect links retransmitting
forever, every process eventually receives and eventually delivers. A partition is different. It
makes delivery unbounded, so the detector accuses processes that are alive and merely unreachable,
`correct` shrinks wrongly, and a process delivers something the far side will never see. That is
now the test, with a companion asserting the mechanism directly — that a partitioned detector does
accuse the unreachable side.

Both were found by tests failing rather than by reading.
