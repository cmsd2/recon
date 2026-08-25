# The macro question, settled

Task 4.1. The reliable-broadcast notes concluded *no macro, extract a private helper per
composing protocol*, and named the thing that would reopen it: **a layer that owns several
children**, on the grounds that several helpers with the same body would be the evidence
generation was waiting for. This is that layer.

**The conclusion stands, and the reason is sharper than before.**

## The evidence

`with_beb` is 23 lines, `with_detector` is 20. Their scaffolding is identical: take the inbox,
clear it, run the child through `with_child_consuming` with a message mapper and a timer mapper,
drain what came back, restore the inbox, re-check the delivery condition. A macro could generate
every line of that.

What it could not generate is the part in the middle:

```rust
// with_beb
for ind in inbox.drain(..) {
    let beb::Ind::Deliver { from, msg: Data { id, payload } } = ind;
    self.on_beb_deliver(from, id, payload, cx);
}

// with_detector
for ind in inbox.drain(..) {
    let pfd::Ind::Crash { node } = ind;
    self.correct.remove(&node);
}
```

One accumulates acknowledgements and relays; the other shrinks the set of processes still
believed correct. Between them they *are* Algorithm 3.4's two `upon` clauses. The scaffolding is
the boilerplate; these four lines are the protocol.

## Why two children strengthened the case against a macro rather than weakening it

The prediction was that two helpers differing only in which child they call would justify
generation. They do not differ only in that. They differ in exactly the place the algorithm lives,
and a macro parameterised over that place would generate four lines of scaffolding around a body
the author writes anyway — while hiding the two things a reader most needs to see:

- **why the layer must defer at all.** It cannot hold `&mut self` while a child runs, so
  indications are collected and processed after the child call returns. That constraint is visible
  in eight lines of ordinary Rust and would vanish behind a derive.
- **that both helpers end by re-checking the delivery condition.** `check_deliverable` is called
  from both because Algorithm 3.4's last clause is a predicate over state that either child can
  change — `ack` growing, or `correct` shrinking. A generated helper would have to be told to do
  that, which means writing it, which means the macro has bought nothing.

## The shape that did emerge

Across six protocols the `impl Protocol` blocks are 17 to 39 lines, and the composing ones are the
*shortest*:

| Protocol | `impl Protocol` | Children |
|---|---:|---|
| `reliable_broadcast` | 17 | 1 |
| `perfect_link` | 20 | 1 |
| `best_effort_broadcast` | 27 | 1 |
| `stubborn_link` | 28 | 0 (leaf) |
| `uniform_reliable_broadcast` | 31 | 2 |
| `perfect_failure_detector` | 39 | 0 (leaf) |

The two leaves are the longest, because a leaf has nowhere to put its logic but the handlers. A
composing layer's handlers are one line each and its substance sits in named methods —
`on_beb_deliver`, `check_deliverable`, `can_deliver` — which read as the algorithm's own
vocabulary. That is the outcome the project wanted, reached without a code generator.

## What would reopen it now

Honestly: little. Three children would add a third helper differing in the same way. The remaining
candidate is a layer owning *many instances of one child* — consensus with one instance per log
slot — where the helper would be parameterised by instance rather than by child, and the bodies
really might coincide. That is a different shape from this one and worth re-examining then, but it
is no longer the open question this note was written to close.

**Recommendation: close it.** Re-open only with evidence from a per-instance layer, not from
another layer with a few distinct children.
